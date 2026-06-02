use shared_kernel::{error::AppError, models::{CreateTenantDto, Tenant}};
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_tenant(pool: &PgPool, dto: &CreateTenantDto) -> Result<Tenant, AppError> {
    let tenant = sqlx::query_as!(
        Tenant,
        r#"INSERT INTO tenants (id, name, current_region)
           VALUES ($1, $2, $3)
           RETURNING id, name, current_region, status, updated_at"#,
        Uuid::new_v4(), dto.name, dto.region
    )
    .fetch_one(pool)
    .await?;
    Ok(tenant)
}

pub async fn get_tenant(pool: &PgPool, tenant_id: Uuid) -> Result<Tenant, AppError> {
    sqlx::query_as!(
        Tenant,
        "SELECT id, name, current_region, status, updated_at FROM tenants WHERE id = $1",
        tenant_id
    )
    .fetch_optional(pool)
    .await?
    .ok_or(AppError::TenantNotFound)
}

pub async fn list_tenants(pool: &PgPool) -> Result<Vec<Tenant>, AppError> {
    Ok(sqlx::query_as!(
        Tenant,
        "SELECT id, name, current_region, status, updated_at FROM tenants ORDER BY name"
    )
    .fetch_all(pool)
    .await?)
}

pub async fn update_tenant(
    pool: &PgPool,
    tenant_id: Uuid,
    name: Option<&str>,
) -> Result<Tenant, AppError> {
    let current = get_tenant(pool, tenant_id).await?;
    let new_name = name.unwrap_or(&current.name);
    Ok(sqlx::query_as!(
        Tenant,
        r#"UPDATE tenants SET name = $2, updated_at = NOW()
           WHERE id = $1
           RETURNING id, name, current_region, status, updated_at"#,
        tenant_id, new_name
    )
    .fetch_one(pool)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[sqlx::test(migrations = "../migrations")]
    async fn create_and_get_tenant(pool: PgPool) {
        let dto = CreateTenantDto { name: "acme-corp".into(), region: "eu-west-1".into() };
        let created = create_tenant(&pool, &dto).await.unwrap();
        assert_eq!(created.name, "acme-corp");
        assert_eq!(created.status, "active");

        let fetched = get_tenant(&pool, created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn get_nonexistent_tenant_returns_not_found(pool: PgPool) {
        let result = get_tenant(&pool, Uuid::new_v4()).await;
        assert!(matches!(result, Err(AppError::TenantNotFound)));
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn duplicate_name_returns_db_error(pool: PgPool) {
        let dto = CreateTenantDto { name: "dup-tenant".into(), region: "eu-west-1".into() };
        create_tenant(&pool, &dto).await.unwrap();
        assert!(create_tenant(&pool, &dto).await.is_err());
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn list_tenants_returns_all(pool: PgPool) {
        create_tenant(&pool, &CreateTenantDto { name: "t-a".into(), region: "eu-west-1".into() }).await.unwrap();
        create_tenant(&pool, &CreateTenantDto { name: "t-b".into(), region: "us-east-1".into() }).await.unwrap();
        let tenants = list_tenants(&pool).await.unwrap();
        assert_eq!(tenants.len(), 2);
    }

    #[sqlx::test(migrations = "../migrations")]
    async fn update_tenant_name(pool: PgPool) {
        let dto = CreateTenantDto { name: "old-name".into(), region: "eu-west-1".into() };
        let t = create_tenant(&pool, &dto).await.unwrap();
        let updated = update_tenant(&pool, t.id, Some("new-name")).await.unwrap();
        assert_eq!(updated.name, "new-name");
    }
}
