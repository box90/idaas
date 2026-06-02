pub mod webhooks;

use chrono::Utc;
use dashmap::DashMap;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use shared_kernel::{crypto, error::AppError, models::{Connection, User}};
use sqlx::PgPool;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    pub tenant_id: String,
    pub iss: String,
    pub iat: i64,
    pub exp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

pub struct TokenService {
    key_cache: DashMap<Uuid, EncodingKey>,
    master_key: [u8; 32],
    pub client: reqwest::Client,
}

impl TokenService {
    pub fn new(master_key: [u8; 32]) -> Self {
        Self {
            key_cache: DashMap::new(),
            master_key,
            client: reqwest::Client::new(),
        }
    }

    pub async fn mint_token(
        &self,
        pool: &PgPool,
        user: &User,
        _connection: &Connection,
        issuer: &str,
        extra_claims: serde_json::Value,
    ) -> Result<String, AppError> {
        let encoding_key = self.get_or_load_key(pool, user.tenant_id).await?;
        let now = Utc::now().timestamp();
        let claims = Claims {
            sub: user.id.to_string(),
            email: user.email.clone(),
            tenant_id: user.tenant_id.to_string(),
            iss: issuer.to_owned(),
            iat: now,
            exp: now + 3600,
            org_id: user.organization_id.map(|id| id.to_string()),
            extra: extra_claims,
        };
        encode(&Header::new(Algorithm::RS256), &claims, &encoding_key)
            .map_err(|_| AppError::Crypto)
    }

    async fn get_or_load_key(&self, pool: &PgPool, tenant_id: Uuid) -> Result<EncodingKey, AppError> {
        if let Some(key) = self.key_cache.get(&tenant_id) {
            return Ok(key.clone());
        }
        let row = sqlx::query!(
            "SELECT private_key_encrypted FROM tenant_keys WHERE tenant_id = $1 AND is_active = TRUE LIMIT 1",
            tenant_id
        )
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::Crypto)?;

        let pem = crypto::decrypt(&self.master_key, &row.private_key_encrypted)?;
        let key = EncodingKey::from_rsa_pem(&pem).map_err(|_| AppError::Crypto)?;
        self.key_cache.insert(tenant_id, key.clone());
        Ok(key)
    }

    pub fn invalidate_cache(&self, tenant_id: Uuid) {
        self.key_cache.remove(&tenant_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{decode, Algorithm, DecodingKey, TokenData, Validation};
    use rand::thread_rng;
    use rsa::{pkcs8::EncodePrivateKey, pkcs8::EncodePublicKey, RsaPrivateKey};
    use shared_kernel::crypto;
    use sqlx::PgPool;

    fn gen_rsa_pair() -> (Vec<u8>, String) {
        let mut rng = thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pub_key = priv_key.to_public_key();
        let priv_pem = priv_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap()
            .to_string()
            .into_bytes();
        let pub_pem = pub_key
            .to_public_key_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap();
        (priv_pem, pub_pem)
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn mint_token_produces_verifiable_jwt(pool: PgPool) {
        let master_key = [0x42u8; 32];
        let svc = TokenService::new(master_key);
        let tenant_id = Uuid::new_v4();
        let conn_id = Uuid::new_v4();
        let user_id = Uuid::new_v4();

        sqlx::query!(
            "INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tenant_id, "t-jwt", "eu-west-1"
        ).execute(&pool).await.unwrap();
        sqlx::query!(
            "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
            conn_id, tenant_id, "db", "database", b"x".as_slice()
        ).execute(&pool).await.unwrap();

        let (priv_pem, pub_pem) = gen_rsa_pair();
        let enc_key = crypto::encrypt(&master_key, &priv_pem).unwrap();
        sqlx::query!(
            "INSERT INTO tenant_keys (tenant_id, private_key_encrypted, public_key_pem) VALUES ($1, $2, $3)",
            tenant_id, enc_key.as_slice(), pub_pem
        ).execute(&pool).await.unwrap();

        let user = User {
            id: user_id, tenant_id, organization_id: None, connection_id: conn_id,
            email: "jwt@test.com".into(), password_hash: None, external_provider_id: None,
            user_metadata: sqlx::types::Json(serde_json::json!({})),
            app_metadata: sqlx::types::Json(serde_json::json!({})),
            created_at: Utc::now(), updated_at: Utc::now(),
        };
        let conn = Connection {
            id: conn_id, tenant_id, name: "db".into(), strategy: "database".into(),
            options_encrypted: b"x".to_vec(), webhook_url: None, created_at: Utc::now(),
        };

        let token = svc.mint_token(&pool, &user, &conn, "https://idass.example.com", serde_json::json!({}))
            .await.unwrap();

        let decoding_key = DecodingKey::from_rsa_pem(pub_pem.as_bytes()).unwrap();
        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_issuer(&["https://idass.example.com"]);
        let data: TokenData<Claims> = decode(&token, &decoding_key, &validation).unwrap();
        assert_eq!(data.claims.sub, user_id.to_string());
        assert_eq!(data.claims.email, "jwt@test.com");
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn second_mint_uses_cache(pool: PgPool) {
        let master_key = [0x42u8; 32];
        let svc = TokenService::new(master_key);
        let tenant_id = Uuid::new_v4();
        let conn_id = Uuid::new_v4();

        sqlx::query!(
            "INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tenant_id, "t-cache", "eu-west-1"
        ).execute(&pool).await.unwrap();
        sqlx::query!(
            "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
            conn_id, tenant_id, "db", "database", b"x".as_slice()
        ).execute(&pool).await.unwrap();

        let (priv_pem, pub_pem) = gen_rsa_pair();
        let enc_key = crypto::encrypt(&master_key, &priv_pem).unwrap();
        sqlx::query!(
            "INSERT INTO tenant_keys (tenant_id, private_key_encrypted, public_key_pem) VALUES ($1, $2, $3)",
            tenant_id, enc_key.as_slice(), pub_pem
        ).execute(&pool).await.unwrap();

        let user = User {
            id: Uuid::new_v4(), tenant_id, organization_id: None, connection_id: conn_id,
            email: "u@test.com".into(), password_hash: None, external_provider_id: None,
            user_metadata: sqlx::types::Json(serde_json::json!({})),
            app_metadata: sqlx::types::Json(serde_json::json!({})),
            created_at: Utc::now(), updated_at: Utc::now(),
        };
        let conn = Connection {
            id: conn_id, tenant_id, name: "db".into(), strategy: "database".into(),
            options_encrypted: b"x".to_vec(), webhook_url: None, created_at: Utc::now(),
        };

        svc.mint_token(&pool, &user, &conn, "https://i.example.com", serde_json::json!({})).await.unwrap();
        assert!(svc.key_cache.contains_key(&tenant_id));
        svc.mint_token(&pool, &user, &conn, "https://i.example.com", serde_json::json!({})).await.unwrap();
    }
}
