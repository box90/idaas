use std::{net::IpAddr, str::FromStr, time::Duration};

use openidconnect::{
    ClientId, ClientSecret, IssuerUrl,
    core::{CoreIdToken, CoreIdTokenVerifier, CoreJsonWebKeySet},
};
use shared_kernel::{
    error::AppError,
    models::{Connection, OIDCCreateInput, OIDCResolvedConfig},
};
use sqlx::PgPool;

// ── SSRF protection ───────────────────────────────────────────────────────────

/// Validates that a URL is safe to fetch server-side:
/// - Must use HTTPS
/// - Host must resolve to at least one IP address
/// - Every resolved IP must be a globally routable address
///   (no loopback, private, link-local, or cloud-metadata ranges)
///
/// Skipped in test builds — wiremock uses HTTP on localhost and is inherently trusted.
async fn validate_public_https_url(url_str: &str) -> Result<(), AppError> {
    #[cfg(test)]
    {
        let _ = url_str;
        return Ok(());
    }

    #[cfg(not(test))]
    {
        let url = url_str
            .parse::<reqwest::Url>()
            .map_err(|_| AppError::InvalidConnection)?;

        if url.scheme() != "https" {
            return Err(AppError::InvalidConnection);
        }

        let host = url.host_str().ok_or(AppError::InvalidConnection)?;
        let port = url.port().unwrap_or(443);

        let addrs = tokio::net::lookup_host(format!("{}:{}", host, port))
            .await
            .map_err(|_| AppError::InvalidConnection)?;

        let mut resolved_any = false;
        for addr in addrs {
            resolved_any = true;
            if !is_globally_routable(&addr.ip()) {
                return Err(AppError::InvalidConnection);
            }
        }

        if !resolved_any {
            return Err(AppError::InvalidConnection);
        }

        Ok(())
    }
}

fn is_globally_routable(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !v4.is_loopback()
                && !v4.is_private()       // 10/8, 172.16/12, 192.168/16
                && !v4.is_link_local()    // 169.254/16  (covers cloud metadata)
                && !v4.is_broadcast()
                && !v4.is_multicast()
                && !v4.is_unspecified()
        }
        IpAddr::V6(v6) => {
            !v6.is_loopback()
                && !v6.is_unspecified()
                && !v6.is_multicast()
                // ULA fc00::/7
                && (v6.octets()[0] & 0xfe) != 0xfc
                // link-local fe80::/10
                && !(v6.octets()[0] == 0xfe && (v6.octets()[1] & 0xc0) == 0x80)
        }
    }
}

// ── resolve_oidc_config ───────────────────────────────────────────────────────

pub async fn resolve_oidc_config(
    input: &OIDCCreateInput,
) -> Result<OIDCResolvedConfig, AppError> {
    // Shared client: no redirect following (prevents SSRF via open redirect)
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| AppError::Crypto)?;

    let doc: serde_json::Value = match input.mode.as_str() {
        "discover" => {
            let issuer = input.issuer_url.as_deref()
                .ok_or(AppError::InvalidConnection)?
                .trim_end_matches('/');
            let url = format!("{}/.well-known/openid-configuration", issuer);
            validate_public_https_url(&url).await?;
            http.get(&url)
                .send()
                .await
                .map_err(|_| AppError::WebhookTimeout)?
                .json()
                .await
                .map_err(|_| AppError::InvalidConnection)?
        }
        "custom" => input.discovery_document.clone()
            .ok_or(AppError::InvalidConnection)?,
        _ => return Err(AppError::InvalidConnection),
    };

    let get = |key: &str| -> Result<String, AppError> {
        doc[key].as_str()
            .ok_or(AppError::InvalidConnection)
            .map(str::to_owned)
    };

    let config = OIDCResolvedConfig {
        issuer:                 get("issuer")?,
        authorization_endpoint: get("authorization_endpoint")?,
        token_endpoint:         get("token_endpoint")?,
        jwks_uri:               get("jwks_uri")?,
        client_id:              input.client_id.clone(),
        client_secret:          input.client_secret.clone(),
        redirect_uri:           input.redirect_uri.clone(),
    };

    // Validate all stored endpoints regardless of mode — ensures custom documents
    // cannot point token_endpoint or jwks_uri at internal addresses.
    validate_public_https_url(&config.token_endpoint).await?;
    validate_public_https_url(&config.jwks_uri).await?;

    Ok(config)
}

/// Builds the OIDC authorization redirect URL and returns `(url, csrf_state)`.
pub fn generate_oidc_redirect(
    config: &OIDCResolvedConfig,
) -> Result<(String, String), AppError> {
    let csrf_state = uuid::Uuid::new_v4().to_string();
    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope=openid%20email&state={}",
        config.authorization_endpoint,
        urlencoding::encode(&config.client_id),
        urlencoding::encode(&config.redirect_uri),
        urlencoding::encode(&csrf_state),
    );
    Ok((auth_url, csrf_state))
}

/// Exchanges an authorization `code` for an ID token, verifies it, and returns the user
/// (provisioning a new one via JIT if it does not yet exist).
pub async fn exchange_code_oidc(
    pool: &PgPool,
    connection: &Connection,
    code: &str,
    master_key: &[u8; 32],
) -> Result<shared_kernel::models::User, AppError> {
    let dec = shared_kernel::crypto::decrypt(master_key, &connection.options_encrypted)?;
    let config: OIDCResolvedConfig =
        serde_json::from_slice(&dec).map_err(|_| AppError::InvalidConnection)?;

    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| AppError::Crypto)?;

    let token_resp: serde_json::Value = http
        .post(&config.token_endpoint)
        .form(&[
            ("client_id",     config.client_id.as_str()),
            ("client_secret", config.client_secret.as_str()),
            ("redirect_uri",  config.redirect_uri.as_str()),
            ("code",          code),
            ("grant_type",    "authorization_code"),
        ])
        .send()
        .await
        .map_err(|_| AppError::WebhookTimeout)?
        .json()
        .await
        .map_err(|_| AppError::InvalidToken)?;

    let id_token_str = token_resp["id_token"]
        .as_str()
        .ok_or(AppError::InvalidToken)?;

    let jwks: CoreJsonWebKeySet = http
        .get(&config.jwks_uri)
        .send()
        .await
        .map_err(|_| AppError::WebhookTimeout)?
        .json()
        .await
        .map_err(|_| AppError::InvalidToken)?;

    let issuer =
        IssuerUrl::new(config.issuer.clone()).map_err(|_| AppError::InvalidConnection)?;
    let verifier = CoreIdTokenVerifier::new_confidential_client(
        ClientId::new(config.client_id.clone()),
        ClientSecret::new(config.client_secret.clone()),
        issuer,
        jwks,
    );
    let id_token =
        CoreIdToken::from_str(id_token_str).map_err(|_| AppError::InvalidToken)?;
    let claims = id_token
        .claims(&verifier, |_nonce: Option<&openidconnect::Nonce>| Ok(()))
        .map_err(|_| AppError::InvalidToken)?;

    let sub   = claims.subject().as_str().to_owned();
    let email = claims
        .email()
        .ok_or(AppError::InvalidConnection)?
        .as_str()
        .to_owned();

    jit_provision_or_find(pool, connection, &sub, &email).await
}

async fn jit_provision_or_find(
    pool: &PgPool,
    connection: &Connection,
    provider_sub: &str,
    email: &str,
) -> Result<shared_kernel::models::User, AppError> {
    if let Some(user) = sqlx::query_as!(
        shared_kernel::models::User,
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
    .await?
    {
        return Ok(user);
    }

    Ok(sqlx::query_as!(
        shared_kernel::models::User,
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
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::{matchers::{method, path}, Mock, MockServer, ResponseTemplate};

    fn base_input(mode: &str) -> OIDCCreateInput {
        OIDCCreateInput {
            mode: mode.into(),
            issuer_url: None,
            discovery_document: None,
            client_id: "test-client".into(),
            client_secret: "test-secret".into(),
            redirect_uri: "https://app.example.com/callback".into(),
        }
    }

    fn discovery_doc(base_url: &str) -> serde_json::Value {
        serde_json::json!({
            "issuer": base_url,
            "authorization_endpoint": format!("{}/auth", base_url),
            "token_endpoint": format!("{}/token", base_url),
            "jwks_uri": format!("{}/jwks", base_url),
            "response_types_supported": ["code"]
        })
    }

    #[tokio::test]
    async fn discover_mode_fetches_and_resolves_endpoints() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/.well-known/openid-configuration"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(discovery_doc(&server.uri()))
            )
            .mount(&server)
            .await;

        let input = OIDCCreateInput {
            issuer_url: Some(server.uri()),
            ..base_input("discover")
        };
        let resolved = resolve_oidc_config(&input).await.unwrap();

        assert_eq!(resolved.issuer, server.uri());
        assert_eq!(resolved.authorization_endpoint, format!("{}/auth", server.uri()));
        assert_eq!(resolved.token_endpoint, format!("{}/token", server.uri()));
        assert_eq!(resolved.jwks_uri, format!("{}/jwks", server.uri()));
        assert_eq!(resolved.client_id, "test-client");
        assert_eq!(resolved.redirect_uri, "https://app.example.com/callback");
    }

    #[tokio::test]
    async fn custom_mode_uses_provided_document() {
        let doc = discovery_doc("https://custom.idp.example.com");
        let input = OIDCCreateInput {
            discovery_document: Some(doc),
            ..base_input("custom")
        };
        let resolved = resolve_oidc_config(&input).await.unwrap();
        assert_eq!(resolved.issuer, "https://custom.idp.example.com");
        assert_eq!(resolved.authorization_endpoint, "https://custom.idp.example.com/auth");
    }

    #[tokio::test]
    async fn discover_mode_without_issuer_url_returns_error() {
        let input = base_input("discover");
        assert!(matches!(
            resolve_oidc_config(&input).await,
            Err(AppError::InvalidConnection)
        ));
    }

    #[tokio::test]
    async fn custom_mode_without_document_returns_error() {
        let input = base_input("custom");
        assert!(matches!(
            resolve_oidc_config(&input).await,
            Err(AppError::InvalidConnection)
        ));
    }

    #[tokio::test]
    async fn custom_mode_with_missing_required_field_returns_error() {
        let incomplete = serde_json::json!({
            "issuer": "https://idp.example.com",
            "authorization_endpoint": "https://idp.example.com/auth"
            // token_endpoint and jwks_uri missing
        });
        let input = OIDCCreateInput {
            discovery_document: Some(incomplete),
            ..base_input("custom")
        };
        assert!(matches!(
            resolve_oidc_config(&input).await,
            Err(AppError::InvalidConnection)
        ));
    }

    // ── BE-3: generate_oidc_redirect ─────────────────────────────────────────

    #[test]
    fn generate_redirect_builds_correct_url() {
        let config = OIDCResolvedConfig {
            issuer: "https://idp.example.com".into(),
            authorization_endpoint: "https://idp.example.com/auth".into(),
            token_endpoint: "https://idp.example.com/token".into(),
            jwks_uri: "https://idp.example.com/jwks".into(),
            client_id: "my-app".into(),
            client_secret: "secret".into(),
            redirect_uri: "https://app.example.com/callback".into(),
        };
        let (url, state) = generate_oidc_redirect(&config).unwrap();

        assert!(url.starts_with("https://idp.example.com/auth?"));
        assert!(url.contains("client_id=my-app"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("scope=openid"));
        assert!(url.contains(&format!("state={}", state)));
        assert!(!state.is_empty());
    }

    // ── BE-4: exchange_code_oidc ─────────────────────────────────────────────

    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use rand::thread_rng;
    use rsa::{pkcs8::EncodePrivateKey, traits::PublicKeyParts, RsaPrivateKey};
    use sqlx::PgPool;

    fn make_test_jwks_and_id_token(
        issuer: &str,
        client_id: &str,
    ) -> (serde_json::Value, String) {
        let mut rng = thread_rng();
        let priv_key = RsaPrivateKey::new(&mut rng, 2048).unwrap();
        let pub_key = priv_key.to_public_key();

        let n_bytes = pub_key.n().to_bytes_be();
        let e_bytes = pub_key.e().to_bytes_be();
        let jwks = serde_json::json!({
            "keys": [{
                "kty": "RSA",
                "use": "sig",
                "alg": "RS256",
                "kid": "test-key-1",
                "n": URL_SAFE_NO_PAD.encode(&n_bytes),
                "e": URL_SAFE_NO_PAD.encode(&e_bytes),
            }]
        });

        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "iss": issuer,
            "aud": client_id,
            "sub": "oidc-user-sub-123",
            "email": "alice@example.com",
            "exp": now + 3600,
            "iat": now,
        });
        let priv_pem = priv_key
            .to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)
            .unwrap()
            .to_string();
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("test-key-1".into());
        let token = encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(priv_pem.as_bytes()).unwrap(),
        )
        .unwrap();

        (jwks, token)
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn exchange_code_oidc_provisions_new_user(pool: PgPool) {
        let server = MockServer::start().await;
        let issuer = server.uri();
        let (jwks, id_token) = make_test_jwks_and_id_token(&issuer, "test-client");

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "id_token": id_token,
                    "access_token": "access-token-xyz",
                    "token_type": "Bearer",
                })),
            )
            .mount(&server)
            .await;

        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&jwks))
            .mount(&server)
            .await;

        let tenant_id = uuid::Uuid::new_v4();
        let conn_id = uuid::Uuid::new_v4();
        sqlx::query!(
            "INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tenant_id, format!("t-oidc-{}", tenant_id), "eu-west-1"
        ).execute(&pool).await.unwrap();

        let master_key = [0x42u8; 32];
        let resolved = OIDCResolvedConfig {
            issuer: issuer.clone(),
            authorization_endpoint: format!("{}/auth", issuer),
            token_endpoint: format!("{}/token", issuer),
            jwks_uri: format!("{}/jwks", issuer),
            client_id: "test-client".into(),
            client_secret: "test-secret".into(),
            redirect_uri: "https://app.example.com/callback".into(),
        };
        let enc = shared_kernel::crypto::encrypt(
            &master_key,
            &serde_json::to_vec(&resolved).unwrap()
        ).unwrap();
        sqlx::query!(
            "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
            conn_id, tenant_id, "keycloak", "oidc", enc.as_slice()
        ).execute(&pool).await.unwrap();

        let conn = Connection {
            id: conn_id, tenant_id, name: "keycloak".into(), strategy: "oidc".into(),
            options_encrypted: enc, webhook_url: None, created_at: chrono::Utc::now(),
        };

        let user = exchange_code_oidc(&pool, &conn, "auth-code-xyz", &master_key).await.unwrap();
        assert_eq!(user.email, "alice@example.com");
        assert_eq!(user.external_provider_id.as_deref(), Some("oidc-user-sub-123"));
    }
}
