use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use shared_kernel::{crypto, error::AppError, models::{Connection, User}};
use sqlx::PgPool;

#[derive(Deserialize)]
struct GoogleTokenResponse {
    id_token: String,
}

#[derive(Deserialize)]
struct GoogleIdTokenClaims {
    sub: String,
    email: String,
}

#[derive(Deserialize, Serialize)]
pub struct GoogleConnectionOptions {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub token_endpoint: String,
}

pub async fn exchange_code(
    pool: &PgPool,
    connection: &Connection,
    code: &str,
    master_key: &[u8; 32],
) -> Result<User, AppError> {
    let decrypted = crypto::decrypt(master_key, &connection.options_encrypted)?;
    let opts: GoogleConnectionOptions =
        serde_json::from_slice(&decrypted).map_err(|_| AppError::InvalidConnection)?;

    let token_resp: GoogleTokenResponse = Client::builder()
        .timeout(std::time::Duration::from_millis(3000))
        .build()
        .map_err(|_| AppError::WebhookTimeout)?
        .post(&opts.token_endpoint)
        .form(&[
            ("client_id", opts.client_id.as_str()),
            ("client_secret", opts.client_secret.as_str()),
            ("redirect_uri", opts.redirect_uri.as_str()),
            ("code", code),
            ("grant_type", "authorization_code"),
        ])
        .send()
        .await
        .map_err(|_| AppError::WebhookTimeout)?
        .json()
        .await
        .map_err(|_| AppError::InvalidConnection)?;

    let claims = decode_id_token(&token_resp.id_token)?;
    jit_provision_or_find(pool, connection, &claims.sub, &claims.email).await
}

fn decode_id_token(id_token: &str) -> Result<GoogleIdTokenClaims, AppError> {
    let parts: Vec<&str> = id_token.split('.').collect();
    if parts.len() != 3 {
        return Err(AppError::InvalidToken);
    }
    let payload = URL_SAFE_NO_PAD.decode(parts[1]).map_err(|_| AppError::InvalidToken)?;
    serde_json::from_slice(&payload).map_err(|_| AppError::InvalidToken)
}

async fn jit_provision_or_find(
    pool: &PgPool,
    connection: &Connection,
    provider_sub: &str,
    email: &str,
) -> Result<User, AppError> {
    if let Some(user) = sqlx::query_as!(
        User,
        r#"SELECT id, tenant_id, organization_id, connection_id, email,
                  password_hash, external_provider_id,
                  user_metadata as "user_metadata!: sqlx::types::Json<serde_json::Value>",
                  app_metadata  as "app_metadata!: sqlx::types::Json<serde_json::Value>",
                  created_at, updated_at
           FROM users
           WHERE tenant_id = $1 AND connection_id = $2 AND external_provider_id = $3"#,
        connection.tenant_id, connection.id, provider_sub
    )
    .fetch_optional(pool)
    .await? {
        return Ok(user);
    }

    let user = sqlx::query_as!(
        User,
        r#"INSERT INTO users (tenant_id, connection_id, email, external_provider_id)
           VALUES ($1, $2, $3, $4)
           RETURNING id, tenant_id, organization_id, connection_id, email,
                     password_hash, external_provider_id,
                     user_metadata as "user_metadata!: sqlx::types::Json<serde_json::Value>",
                     app_metadata  as "app_metadata!: sqlx::types::Json<serde_json::Value>",
                     created_at, updated_at"#,
        connection.tenant_id, connection.id, email, provider_sub
    )
    .fetch_one(pool)
    .await?;
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared_kernel::crypto;
    use uuid::Uuid;
    use wiremock::{matchers::{method, path}, Mock, MockServer, ResponseTemplate};

    fn test_key() -> [u8; 32] { [0x42u8; 32] }

    fn make_fake_id_token(sub: &str, email: &str) -> String {
        let header = URL_SAFE_NO_PAD.encode(r#"{"alg":"RS256","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD
            .encode(serde_json::json!({"sub": sub, "email": email}).to_string());
        format!("{}.{}.fakesig", header, payload)
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn new_google_user_is_jit_provisioned(pool: PgPool) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "id_token": make_fake_id_token("g-sub-1", "alice@gmail.com") })
            ))
            .mount(&server).await;

        let tenant_id = Uuid::new_v4();
        let conn_id = Uuid::new_v4();
        sqlx::query!("INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tenant_id, "t-google", "eu-west-1").execute(&pool).await.unwrap();

        let opts = serde_json::to_vec(&GoogleConnectionOptions {
            client_id: "cid".into(), client_secret: "csec".into(),
            redirect_uri: "https://example.com/cb".into(),
            token_endpoint: format!("{}/token", server.uri()),
        }).unwrap();
        let enc = crypto::encrypt(&test_key(), &opts).unwrap();
        sqlx::query!(
            "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
            conn_id, tenant_id, "google", "oauth2", enc.as_slice()
        ).execute(&pool).await.unwrap();

        let conn = Connection {
            id: conn_id, tenant_id, name: "google".into(), strategy: "oauth2".into(),
            options_encrypted: enc, webhook_url: None, created_at: chrono::Utc::now(),
        };
        let user = exchange_code(&pool, &conn, "code-xyz", &test_key()).await.unwrap();
        assert_eq!(user.email, "alice@gmail.com");
        assert_eq!(user.external_provider_id.as_deref(), Some("g-sub-1"));
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn existing_google_user_is_returned(pool: PgPool) {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                serde_json::json!({ "id_token": make_fake_id_token("g-sub-2", "bob@gmail.com") })
            ))
            .mount(&server).await;

        let tenant_id = Uuid::new_v4();
        let conn_id = Uuid::new_v4();
        sqlx::query!("INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tenant_id, "t-google-2", "eu-west-1").execute(&pool).await.unwrap();
        let opts = serde_json::to_vec(&GoogleConnectionOptions {
            client_id: "cid".into(), client_secret: "csec".into(),
            redirect_uri: "https://example.com/cb".into(),
            token_endpoint: format!("{}/token", server.uri()),
        }).unwrap();
        let enc = crypto::encrypt(&test_key(), &opts).unwrap();
        sqlx::query!(
            "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
            conn_id, tenant_id, "google", "oauth2", enc.as_slice()
        ).execute(&pool).await.unwrap();
        sqlx::query!(
            "INSERT INTO users (tenant_id, connection_id, email, external_provider_id) VALUES ($1, $2, $3, $4)",
            tenant_id, conn_id, "bob@gmail.com", "g-sub-2"
        ).execute(&pool).await.unwrap();

        let conn = Connection {
            id: conn_id, tenant_id, name: "google".into(), strategy: "oauth2".into(),
            options_encrypted: enc, webhook_url: None, created_at: chrono::Utc::now(),
        };
        let user = exchange_code(&pool, &conn, "code-abc", &test_key()).await.unwrap();
        assert_eq!(user.email, "bob@gmail.com");

        // Verify no duplicate was created
        let count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM users WHERE tenant_id = $1", tenant_id
        ).fetch_one(&pool).await.unwrap().unwrap_or(0);
        assert_eq!(count, 1);
    }
}
