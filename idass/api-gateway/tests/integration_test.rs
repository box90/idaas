//! End-to-end integration tests.
//! Requires: DATABASE_URL and TEST_DATABASE_URL pointing to a live PostgreSQL instance.

use mod_mgmt::user_crud;
use mod_token::TokenService;
use shared_kernel::{crypto, models::CreateUserDto};
use sqlx::PgPool;
use uuid::Uuid;

async fn pool(url: &str) -> PgPool {
    let pool = sqlx::PgPool::connect(url).await.expect("DB connect failed");
    sqlx::migrate!("../migrations")
        .run(&pool)
        .await
        .expect("migration failed");
    pool
}

/// Full database auth flow: create tenant → provision user with password →
/// authenticate → mint JWT → verify JWT claims.
#[tokio::test]
async fn database_auth_flow_end_to_end() {
    let db_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://idass:idass@localhost:5432/idass_eu_west_1".into());
    let pool = pool(&db_url).await;
    let master_key = [0x42u8; 32];

    // Seed tenant, connection, and tenant signing key
    let tenant_id = Uuid::new_v4();
    let conn_id = Uuid::new_v4();

    sqlx::query!(
        "INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
        tenant_id,
        format!("e2e-tenant-{}", &tenant_id.to_string()[..8]),
        "eu-west-1"
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query!(
        "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
        conn_id,
        tenant_id,
        "db",
        "database",
        b"placeholder".as_slice()
    )
    .execute(&pool)
    .await
    .unwrap();

    // Generate RSA key pair and store encrypted in tenant_keys
    let (priv_pem, pub_pem) = generate_rsa_pair();
    let enc_priv = crypto::encrypt(&master_key, &priv_pem).unwrap();
    sqlx::query!(
        "INSERT INTO tenant_keys (tenant_id, private_key_encrypted, public_key_pem) VALUES ($1, $2, $3)",
        tenant_id,
        enc_priv.as_slice(),
        pub_pem
    )
    .execute(&pool)
    .await
    .unwrap();

    let test_email = format!("e2e-{}@example.com", &tenant_id.to_string()[..8]);

    // Create user via mod-mgmt
    let created = user_crud::create_user(
        &pool,
        &CreateUserDto {
            tenant_id,
            organization_id: None,
            connection_id: conn_id,
            email: test_email.clone(),
            password: Some("hunter2".into()),
            external_provider_id: None,
        },
    )
    .await
    .unwrap();

    // Authenticate via mod-auth (Argon2id check)
    let authed =
        mod_auth::db_auth::authenticate_db(&pool, tenant_id, &test_email, "hunter2")
            .await
            .unwrap();
    assert_eq!(authed.id, created.id);

    // Wrong password returns error
    assert!(
        mod_auth::db_auth::authenticate_db(&pool, tenant_id, &test_email, "wrong")
            .await
            .is_err()
    );

    // Mint JWT
    let token_svc = TokenService::new(master_key);
    let connection = sqlx::query_as!(
        shared_kernel::models::Connection,
        r#"SELECT id, tenant_id, name, strategy, options_encrypted, webhook_url, created_at
           FROM connections WHERE id = $1"#,
        conn_id
    )
    .fetch_one(&pool)
    .await
    .unwrap();

    let token = token_svc
        .mint_token(
            &pool,
            &authed,
            &connection,
            "https://idass.example.com",
            serde_json::json!({}),
        )
        .await
        .unwrap();

    // Decode and verify JWT claims
    let decoding_key = jsonwebtoken::DecodingKey::from_rsa_pem(pub_pem.as_bytes()).unwrap();
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.set_issuer(&["https://idass.example.com"]);
    let data =
        jsonwebtoken::decode::<mod_token::Claims>(&token, &decoding_key, &validation).unwrap();

    assert_eq!(data.claims.sub, authed.id.to_string());
    assert_eq!(data.claims.email, test_email);
    assert_eq!(data.claims.tenant_id, tenant_id.to_string());
}

/// Exercises the 5-phase geographic migration between two live databases.
/// Requires TEST_DATABASE_URL (source) and TEST_DATABASE_URL_2 (target).
#[tokio::test]
async fn geographic_migration_end_to_end() {
    let source_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://idass:idass@localhost:5432/idass_eu_west_1".into());
    let target_url = std::env::var("TEST_DATABASE_URL_2")
        .unwrap_or_else(|_| "postgres://idass:idass@localhost:5433/idass_us_east_1".into());

    let source = pool(&source_url).await;
    let target = pool(&target_url).await;

    let tenant_id = Uuid::new_v4();
    let conn_id   = Uuid::new_v4();
    let short_id  = &tenant_id.to_string()[..8];

    // Seed tenant + connection + user in source
    sqlx::query!(
        "INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
        tenant_id, format!("migration-e2e-{}", short_id), "eu-west-1"
    ).execute(&source).await.unwrap();

    sqlx::query!(
        "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
        conn_id, tenant_id, "db", "database", b"x".as_slice()
    ).execute(&source).await.unwrap();

    sqlx::query!(
        "INSERT INTO users (tenant_id, connection_id, email) VALUES ($1, $2, $3)",
        tenant_id, conn_id, format!("migrant-{}@example.com", short_id)
    ).execute(&source).await.unwrap();

    // Execute migration
    mod_mgmt::migration::execute_geographic_migration(
        tenant_id, &source, &target, "us-east-1"
    ).await.unwrap();

    // Assert: tenant is active in target with new region
    let row = sqlx::query!(
        "SELECT status, current_region FROM tenants WHERE id = $1", tenant_id
    ).fetch_one(&target).await.unwrap();
    assert_eq!(row.status, "active");
    assert_eq!(row.current_region, "us-east-1");

    // Assert: user migrated to target
    let target_user_count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM users WHERE tenant_id = $1", tenant_id
    ).fetch_one(&target).await.unwrap().unwrap_or(0);
    assert_eq!(target_user_count, 1);

    // Assert: source PII purged (GDPR)
    let source_tenant_count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM tenants WHERE id = $1", tenant_id
    ).fetch_one(&source).await.unwrap().unwrap_or(0);
    assert_eq!(source_tenant_count, 0);

    let source_user_count: i64 = sqlx::query_scalar!(
        "SELECT COUNT(*) FROM users WHERE tenant_id = $1", tenant_id
    ).fetch_one(&source).await.unwrap().unwrap_or(0);
    assert_eq!(source_user_count, 0);
}

fn generate_rsa_pair() -> (Vec<u8>, String) {
    use rand::thread_rng;
    use rsa::{
        pkcs8::{EncodePrivateKey, EncodePublicKey},
        RsaPrivateKey,
    };
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
