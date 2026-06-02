use shared_kernel::{error::AppError, models::Organization};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn list_organizations(
    pool: &PgPool,
    tenant_id: Uuid,
) -> Result<Vec<Organization>, AppError> {
    Ok(sqlx::query_as!(
        Organization,
        r#"SELECT id, tenant_id, name, display_name,
                  COALESCE(metadata, '{}'::jsonb) as "metadata!: sqlx::types::Json<serde_json::Value>",
                  created_at
           FROM organizations WHERE tenant_id = $1 ORDER BY name"#,
        tenant_id
    )
    .fetch_all(pool)
    .await?)
}

pub async fn delete_organization(
    pool: &PgPool,
    tenant_id: Uuid,
    org_id: Uuid,
) -> Result<(), AppError> {
    let rows = sqlx::query!(
        "DELETE FROM organizations WHERE tenant_id = $1 AND id = $2",
        tenant_id, org_id
    )
    .execute(pool)
    .await?
    .rows_affected();
    if rows == 0 { Err(AppError::UserNotFound) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::user_crud::create_organization;

    async fn seed(pool: &PgPool) -> Uuid {
        let tid = Uuid::new_v4();
        sqlx::query!("INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tid, format!("t-{}", tid), "eu-west-1").execute(pool).await.unwrap();
        tid
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn list_and_delete_organization(pool: PgPool) {
        let tid = seed(&pool).await;
        let org = create_organization(&pool, tid, "eng", Some("Engineering")).await.unwrap();
        create_organization(&pool, tid, "ops", None).await.unwrap();

        let list = list_organizations(&pool, tid).await.unwrap();
        assert_eq!(list.len(), 2);

        delete_organization(&pool, tid, org.id).await.unwrap();
        assert_eq!(list_organizations(&pool, tid).await.unwrap().len(), 1);
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn delete_nonexistent_returns_error(pool: PgPool) {
        let tid = seed(&pool).await;
        assert!(delete_organization(&pool, tid, Uuid::new_v4()).await.is_err());
    }
}
