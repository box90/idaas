use argon2::{Argon2, PasswordHash, PasswordVerifier};
use shared_kernel::{error::AppError, models::User};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn authenticate_db(
    pool: &PgPool,
    tenant_id: Uuid,
    email: &str,
    password: &str,
) -> Result<User, AppError> {
    let user = sqlx::query_as!(
        User,
        r#"SELECT id, tenant_id, organization_id, connection_id, email,
                  password_hash, external_provider_id,
                  user_metadata as "user_metadata!: sqlx::types::Json<serde_json::Value>",
                  app_metadata  as "app_metadata!: sqlx::types::Json<serde_json::Value>",
                  created_at, updated_at
           FROM users
           WHERE tenant_id = $1 AND email = $2"#,
        tenant_id, email
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::InvalidCredentials)?;

    let hash = user.password_hash.clone().ok_or(AppError::InvalidCredentials)?;
    let password_owned = password.to_owned();

    tokio::task::spawn_blocking(move || {
        let parsed = PasswordHash::new(&hash).map_err(|_| AppError::Crypto)?;
        Argon2::default()
            .verify_password(password_owned.as_bytes(), &parsed)
            .map_err(|_| AppError::InvalidCredentials)
    })
    .await
    .map_err(|_| AppError::Crypto)??;

    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
    use rand_core::OsRng;

    fn hash_password(pw: &str) -> String {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(pw.as_bytes(), &salt)
            .unwrap()
            .to_string()
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn valid_credentials_return_user(pool: PgPool) {
        let tenant_id = Uuid::new_v4();
        let conn_id = Uuid::new_v4();
        sqlx::query!(
            "INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tenant_id, "test-tenant", "eu-west-1"
        ).execute(&pool).await.unwrap();
        sqlx::query!(
            "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
            conn_id, tenant_id, "default-db", "database", b"x".as_slice()
        ).execute(&pool).await.unwrap();
        let pw_hash = hash_password("correct-password");
        sqlx::query!(
            "INSERT INTO users (tenant_id, connection_id, email, password_hash) VALUES ($1, $2, $3, $4)",
            tenant_id, conn_id, "user@test.com", pw_hash
        ).execute(&pool).await.unwrap();

        let result = authenticate_db(&pool, tenant_id, "user@test.com", "correct-password").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().email, "user@test.com");
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn wrong_password_returns_invalid_credentials(pool: PgPool) {
        let tenant_id = Uuid::new_v4();
        let conn_id = Uuid::new_v4();
        sqlx::query!(
            "INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tenant_id, "t2", "eu-west-1"
        ).execute(&pool).await.unwrap();
        sqlx::query!(
            "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
            conn_id, tenant_id, "default-db", "database", b"x".as_slice()
        ).execute(&pool).await.unwrap();
        let pw_hash = hash_password("correct-password");
        sqlx::query!(
            "INSERT INTO users (tenant_id, connection_id, email, password_hash) VALUES ($1, $2, $3, $4)",
            tenant_id, conn_id, "u@test.com", pw_hash
        ).execute(&pool).await.unwrap();

        let result = authenticate_db(&pool, tenant_id, "u@test.com", "wrong-password").await;
        assert!(matches!(result, Err(AppError::InvalidCredentials)));
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn unknown_user_returns_invalid_credentials(pool: PgPool) {
        let tenant_id = Uuid::new_v4();
        sqlx::query!(
            "INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tenant_id, "t3", "eu-west-1"
        ).execute(&pool).await.unwrap();

        let result = authenticate_db(&pool, tenant_id, "nobody@test.com", "pw").await;
        assert!(matches!(result, Err(AppError::InvalidCredentials)));
    }
}
