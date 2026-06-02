use argon2::{password_hash::SaltString, Argon2, PasswordHasher};
use rand_core::OsRng;
use shared_kernel::{error::AppError, models::{CreateUserDto, Organization, User}};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_user(pool: &PgPool, dto: &CreateUserDto) -> Result<User, AppError> {
    let password_hash = match &dto.password {
        Some(pw) => {
            let pw_owned = pw.clone();
            Some(
                tokio::task::spawn_blocking(move || {
                    let salt = SaltString::generate(&mut OsRng);
                    Argon2::default()
                        .hash_password(pw_owned.as_bytes(), &salt)
                        .map(|h| h.to_string())
                        .map_err(|_| AppError::Crypto)
                })
                .await
                .map_err(|_| AppError::Crypto)??,
            )
        }
        None => None,
    };

    let user = sqlx::query_as!(
        User,
        r#"INSERT INTO users (tenant_id, organization_id, connection_id, email, password_hash, external_provider_id)
           VALUES ($1, $2, $3, $4, $5, $6)
           RETURNING id, tenant_id, organization_id, connection_id, email,
                     password_hash, external_provider_id,
                     COALESCE(user_metadata, '{}'::jsonb) as "user_metadata!: sqlx::types::Json<serde_json::Value>",
                     COALESCE(app_metadata, '{}'::jsonb)  as "app_metadata!: sqlx::types::Json<serde_json::Value>",
                     created_at, updated_at"#,
        dto.tenant_id, dto.organization_id, dto.connection_id, dto.email,
        password_hash, dto.external_provider_id
    )
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn get_user(pool: &PgPool, tenant_id: Uuid, user_id: Uuid) -> Result<User, AppError> {
    sqlx::query_as!(
        User,
        r#"SELECT id, tenant_id, organization_id, connection_id, email,
                  password_hash, external_provider_id,
                  COALESCE(user_metadata, '{}'::jsonb) as "user_metadata!: sqlx::types::Json<serde_json::Value>",
                  COALESCE(app_metadata, '{}'::jsonb)  as "app_metadata!: sqlx::types::Json<serde_json::Value>",
                  created_at, updated_at
           FROM users
           WHERE tenant_id = $1 AND id = $2"#,
        tenant_id, user_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::UserNotFound)
}

pub async fn delete_user(pool: &PgPool, tenant_id: Uuid, user_id: Uuid) -> Result<(), AppError> {
    let rows = sqlx::query!(
        "DELETE FROM users WHERE tenant_id = $1 AND id = $2",
        tenant_id, user_id
    )
    .execute(pool)
    .await?
    .rows_affected();
    if rows == 0 { Err(AppError::UserNotFound) } else { Ok(()) }
}

pub async fn create_organization(
    pool: &PgPool,
    tenant_id: Uuid,
    name: &str,
    display_name: Option<&str>,
) -> Result<Organization, AppError> {
    let org = sqlx::query_as!(
        Organization,
        r#"INSERT INTO organizations (tenant_id, name, display_name)
           VALUES ($1, $2, $3)
           RETURNING id, tenant_id, name, display_name,
                     COALESCE(metadata, '{}'::jsonb) as "metadata!: sqlx::types::Json<serde_json::Value>",
                     created_at"#,
        tenant_id, name, display_name
    )
    .fetch_one(pool)
    .await?;
    Ok(org)
}

pub async fn list_users(pool: &PgPool, tenant_id: Uuid) -> Result<Vec<User>, AppError> {
    Ok(sqlx::query_as!(
        User,
        r#"SELECT id, tenant_id, organization_id, connection_id, email,
                  password_hash, external_provider_id,
                  COALESCE(user_metadata, '{}'::jsonb) as "user_metadata!: sqlx::types::Json<serde_json::Value>",
                  COALESCE(app_metadata, '{}'::jsonb)  as "app_metadata!: sqlx::types::Json<serde_json::Value>",
                  created_at, updated_at
           FROM users WHERE tenant_id = $1 ORDER BY created_at"#,
        tenant_id
    )
    .fetch_all(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seed(pool: &PgPool) -> (Uuid, Uuid) {
        let tid = Uuid::new_v4();
        let cid = Uuid::new_v4();
        sqlx::query!("INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tid, format!("t-{}", tid), "eu-west-1").execute(pool).await.unwrap();
        sqlx::query!(
            "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
            cid, tid, "db", "database", b"x".as_slice()
        ).execute(pool).await.unwrap();
        (tid, cid)
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn create_and_get_user(pool: PgPool) {
        let (tid, cid) = seed(&pool).await;
        let user = create_user(&pool, &CreateUserDto {
            tenant_id: tid, organization_id: None, connection_id: cid,
            email: "bob@example.com".into(), password: Some("hunter2".into()),
            external_provider_id: None,
        }).await.unwrap();
        assert_eq!(user.email, "bob@example.com");
        assert!(user.password_hash.is_some());

        let fetched = get_user(&pool, tid, user.id).await.unwrap();
        assert_eq!(fetched.id, user.id);
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn multi_tenancy_isolation(pool: PgPool) {
        let (tid_a, cid_a) = seed(&pool).await;
        let (tid_b, _)     = seed(&pool).await;

        let user = create_user(&pool, &CreateUserDto {
            tenant_id: tid_a, organization_id: None, connection_id: cid_a,
            email: "secret@example.com".into(), password: None,
            external_provider_id: None,
        }).await.unwrap();

        // Tenant B cannot fetch Tenant A's user
        let result = get_user(&pool, tid_b, user.id).await;
        assert!(matches!(result, Err(AppError::UserNotFound)));
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn delete_user_removes_record(pool: PgPool) {
        let (tid, cid) = seed(&pool).await;
        let user = create_user(&pool, &CreateUserDto {
            tenant_id: tid, organization_id: None, connection_id: cid,
            email: "del@example.com".into(), password: None,
            external_provider_id: None,
        }).await.unwrap();
        delete_user(&pool, tid, user.id).await.unwrap();
        assert!(matches!(get_user(&pool, tid, user.id).await, Err(AppError::UserNotFound)));
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn create_organization_succeeds(pool: PgPool) {
        let (tid, _) = seed(&pool).await;
        let org = create_organization(&pool, tid, "eng-team", Some("Engineering")).await.unwrap();
        assert_eq!(org.name, "eng-team");
        assert_eq!(org.display_name.as_deref(), Some("Engineering"));
        assert_eq!(org.tenant_id, tid);
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn list_users_returns_all_for_tenant(pool: PgPool) {
        let (tid, cid) = seed(&pool).await;
        create_user(&pool, &CreateUserDto {
            tenant_id: tid, organization_id: None, connection_id: cid,
            email: "a@example.com".into(), password: None, external_provider_id: None,
        }).await.unwrap();
        create_user(&pool, &CreateUserDto {
            tenant_id: tid, organization_id: None, connection_id: cid,
            email: "b@example.com".into(), password: None, external_provider_id: None,
        }).await.unwrap();
        let users = list_users(&pool, tid).await.unwrap();
        assert_eq!(users.len(), 2);
    }
}
