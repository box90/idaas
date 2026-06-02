use shared_kernel::{error::AppError, models::{ConnectionSummary, CreateConnectionDto}};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn list_connections(
    pool: &PgPool,
    tenant_id: Uuid,
) -> Result<Vec<ConnectionSummary>, AppError> {
    Ok(sqlx::query_as!(
        ConnectionSummary,
        r#"SELECT id, tenant_id, name, strategy, webhook_url, created_at
           FROM connections WHERE tenant_id = $1 ORDER BY created_at"#,
        tenant_id
    )
    .fetch_all(pool)
    .await?)
}

pub async fn create_connection(
    pool: &PgPool,
    dto: &CreateConnectionDto,
) -> Result<ConnectionSummary, AppError> {
    Ok(sqlx::query_as!(
        ConnectionSummary,
        r#"INSERT INTO connections (tenant_id, name, strategy, options_encrypted, webhook_url)
           VALUES ($1, $2, $3, $4, $5)
           RETURNING id, tenant_id, name, strategy, webhook_url, created_at"#,
        dto.tenant_id, dto.name, dto.strategy, dto.options_encrypted, dto.webhook_url
    )
    .fetch_one(pool)
    .await?)
}

pub async fn update_connection(
    pool: &PgPool,
    tenant_id: Uuid,
    conn_id: Uuid,
    name: Option<&str>,
    options_encrypted: Option<Vec<u8>>,
    webhook_url: Option<Option<String>>,
) -> Result<ConnectionSummary, AppError> {
    let conn = sqlx::query_as!(
        ConnectionSummary,
        r#"SELECT id, tenant_id, name, strategy, webhook_url, created_at
           FROM connections WHERE tenant_id = $1 AND id = $2"#,
        tenant_id, conn_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::InvalidConnection)?;

    let new_name    = name.unwrap_or(&conn.name);
    let new_webhook = webhook_url.unwrap_or(conn.webhook_url.clone());

    if let Some(enc) = options_encrypted {
        Ok(sqlx::query_as!(
            ConnectionSummary,
            r#"UPDATE connections SET name = $3, options_encrypted = $4, webhook_url = $5
               WHERE tenant_id = $1 AND id = $2
               RETURNING id, tenant_id, name, strategy, webhook_url, created_at"#,
            tenant_id, conn_id, new_name, enc, new_webhook
        )
        .fetch_one(pool)
        .await?)
    } else {
        Ok(sqlx::query_as!(
            ConnectionSummary,
            r#"UPDATE connections SET name = $3, webhook_url = $4
               WHERE tenant_id = $1 AND id = $2
               RETURNING id, tenant_id, name, strategy, webhook_url, created_at"#,
            tenant_id, conn_id, new_name, new_webhook
        )
        .fetch_one(pool)
        .await?)
    }
}

pub async fn delete_connection(
    pool: &PgPool,
    tenant_id: Uuid,
    conn_id: Uuid,
) -> Result<(), AppError> {
    let rows = sqlx::query!(
        "DELETE FROM connections WHERE tenant_id = $1 AND id = $2",
        tenant_id, conn_id
    )
    .execute(pool)
    .await?
    .rows_affected();
    if rows == 0 { Err(AppError::InvalidConnection) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn seed(pool: &PgPool) -> Uuid {
        let tid = Uuid::new_v4();
        sqlx::query!("INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tid, format!("t-{}", tid), "eu-west-1").execute(pool).await.unwrap();
        tid
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn create_and_list_connection(pool: PgPool) {
        let tid = seed(&pool).await;
        let dto = CreateConnectionDto {
            tenant_id: tid,
            name: "google".into(),
            strategy: "oauth2".into(),
            options_encrypted: b"encrypted-bytes".to_vec(),
            webhook_url: None,
        };
        let created = create_connection(&pool, &dto).await.unwrap();
        assert_eq!(created.name, "google");
        assert_eq!(created.strategy, "oauth2");

        let list = list_connections(&pool, tid).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, created.id);
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn update_connection_name(pool: PgPool) {
        let tid = seed(&pool).await;
        let dto = CreateConnectionDto {
            tenant_id: tid, name: "old".into(), strategy: "database".into(),
            options_encrypted: b"enc".to_vec(), webhook_url: None,
        };
        let c = create_connection(&pool, &dto).await.unwrap();
        let updated = update_connection(&pool, tid, c.id, Some("new"), None, None).await.unwrap();
        assert_eq!(updated.name, "new");
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn delete_connection_removes_record(pool: PgPool) {
        let tid = seed(&pool).await;
        let dto = CreateConnectionDto {
            tenant_id: tid, name: "del".into(), strategy: "database".into(),
            options_encrypted: b"enc".to_vec(), webhook_url: None,
        };
        let c = create_connection(&pool, &dto).await.unwrap();
        delete_connection(&pool, tid, c.id).await.unwrap();
        assert!(list_connections(&pool, tid).await.unwrap().is_empty());
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn multi_tenancy_isolation(pool: PgPool) {
        let tid_a = seed(&pool).await;
        let tid_b = seed(&pool).await;
        let dto = CreateConnectionDto {
            tenant_id: tid_a, name: "secret".into(), strategy: "oauth2".into(),
            options_encrypted: b"enc".to_vec(), webhook_url: None,
        };
        let c = create_connection(&pool, &dto).await.unwrap();
        // Tenant B cannot see Tenant A's connections
        assert!(list_connections(&pool, tid_b).await.unwrap().is_empty());
        // Tenant B cannot delete Tenant A's connection
        assert!(delete_connection(&pool, tid_b, c.id).await.is_err());
    }
}
