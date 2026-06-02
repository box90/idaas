use shared_kernel::error::AppError;
use sqlx::{Pool, Postgres};
use uuid::Uuid;

/// Executes a 5-phase geographic migration for `tenant_id` from `source_pool` to `target_pool`.
///
/// Phase 1 — Lock: transition source tenant status from 'active' → 'read_only'.
/// Phase 2 — Export: read all tenant data inside a source transaction; record outbox event.
/// Phase 3 — Import: write all data to target in a separate transaction.
/// Phase 4 — Switch: commit target; mark tenant 'active' at target_region.
/// Phase 5 — Purge: delete source data (GDPR); commit source transaction.
pub async fn execute_geographic_migration(
    tenant_id: Uuid,
    source_pool: &Pool<Postgres>,
    target_pool: &Pool<Postgres>,
    target_region: &str,
) -> Result<(), AppError> {
    // ── Phase 1: Lock ────────────────────────────────────────────────────────
    let locked = sqlx::query!(
        "UPDATE tenants SET status = 'read_only', updated_at = NOW()
         WHERE id = $1 AND status = 'active'",
        tenant_id
    )
    .execute(source_pool)
    .await?
    .rows_affected();

    if locked == 0 {
        return Err(AppError::TenantMigrating);
    }

    // ── Phase 2: Export ──────────────────────────────────────────────────────
    let mut src_tx = source_pool.begin().await?;

    let tenant = sqlx::query!(
        "SELECT id, name, current_region FROM tenants WHERE id = $1",
        tenant_id
    )
    .fetch_one(&mut *src_tx)
    .await?;

    let orgs = sqlx::query!(
        "SELECT id, name, display_name, metadata FROM organizations WHERE tenant_id = $1",
        tenant_id
    )
    .fetch_all(&mut *src_tx)
    .await?;

    let conns = sqlx::query!(
        "SELECT id, name, strategy, options_encrypted, webhook_url FROM connections WHERE tenant_id = $1",
        tenant_id
    )
    .fetch_all(&mut *src_tx)
    .await?;

    let users = sqlx::query!(
        r#"SELECT id, organization_id, connection_id, email,
                  password_hash, external_provider_id,
                  user_metadata, app_metadata, created_at, updated_at
           FROM users WHERE tenant_id = $1"#,
        tenant_id
    )
    .fetch_all(&mut *src_tx)
    .await?;

    let keys = sqlx::query!(
        "SELECT id, algorithm, private_key_encrypted, public_key_pem, is_active, created_at
         FROM tenant_keys WHERE tenant_id = $1",
        tenant_id
    )
    .fetch_all(&mut *src_tx)
    .await?;

    sqlx::query!(
        "INSERT INTO migration_outbox (tenant_id, event_type, payload)
         VALUES ($1, 'migration_started', $2)",
        tenant_id,
        serde_json::json!({
            "target_region": target_region,
            "user_count":    users.len(),
            "org_count":     orgs.len(),
        })
    )
    .execute(&mut *src_tx)
    .await?;

    // ── Phase 3: Import ──────────────────────────────────────────────────────
    let mut tgt_tx = target_pool.begin().await?;

    sqlx::query!(
        "INSERT INTO tenants (id, name, current_region, status)
         VALUES ($1, $2, $3, 'migrating')
         ON CONFLICT (id) DO UPDATE SET status = 'migrating', current_region = EXCLUDED.current_region",
        tenant_id,
        tenant.name,
        target_region
    )
    .execute(&mut *tgt_tx)
    .await?;

    for org in &orgs {
        sqlx::query!(
            "INSERT INTO organizations (id, tenant_id, name, display_name, metadata)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (id) DO NOTHING",
            org.id,
            tenant_id,
            org.name,
            org.display_name,
            org.metadata
        )
        .execute(&mut *tgt_tx)
        .await?;
    }

    for conn in &conns {
        sqlx::query!(
            "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted, webhook_url)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (id) DO NOTHING",
            conn.id,
            tenant_id,
            conn.name,
            conn.strategy,
            conn.options_encrypted,
            conn.webhook_url
        )
        .execute(&mut *tgt_tx)
        .await?;
    }

    for user in &users {
        sqlx::query!(
            "INSERT INTO users (id, tenant_id, organization_id, connection_id, email,
                                password_hash, external_provider_id,
                                user_metadata, app_metadata, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
             ON CONFLICT (id) DO NOTHING",
            user.id,
            tenant_id,
            user.organization_id,
            user.connection_id,
            user.email,
            user.password_hash,
            user.external_provider_id,
            user.user_metadata,
            user.app_metadata,
            user.created_at,
            user.updated_at
        )
        .execute(&mut *tgt_tx)
        .await?;
    }

    for key in &keys {
        sqlx::query!(
            "INSERT INTO tenant_keys (id, tenant_id, algorithm, private_key_encrypted, public_key_pem, is_active, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (id) DO NOTHING",
            key.id,
            tenant_id,
            key.algorithm,
            key.private_key_encrypted,
            key.public_key_pem,
            key.is_active,
            key.created_at
        )
        .execute(&mut *tgt_tx)
        .await?;
    }

    // ── Phase 4: Switch ──────────────────────────────────────────────────────
    // Target commits first — if this fails, source rolls back and tenant stays read_only at source
    tgt_tx.commit().await?;

    // Write migration_committed event to target — acts as a recovery marker.
    // A background worker can detect unprocessed source outbox + this event to re-run the purge.
    sqlx::query!(
        "INSERT INTO migration_outbox (tenant_id, event_type, payload)
         VALUES ($1, 'migration_committed', $2)",
        tenant_id,
        serde_json::json!({ "source_region": tenant.current_region, "target_region": target_region })
    )
    .execute(target_pool)
    .await?;

    sqlx::query!(
        "UPDATE tenants SET status = 'active', current_region = $2, updated_at = NOW()
         WHERE id = $1",
        tenant_id,
        target_region
    )
    .execute(target_pool)
    .await?;

    // ── Phase 5: GDPR Purge ──────────────────────────────────────────────────
    // Delete in FK-dependency order (users → tenant_keys → connections → organizations → tenants)
    sqlx::query!("DELETE FROM users         WHERE tenant_id = $1", tenant_id)
        .execute(&mut *src_tx)
        .await?;
    sqlx::query!("DELETE FROM tenant_keys   WHERE tenant_id = $1", tenant_id)
        .execute(&mut *src_tx)
        .await?;
    sqlx::query!("DELETE FROM connections   WHERE tenant_id = $1", tenant_id)
        .execute(&mut *src_tx)
        .await?;
    sqlx::query!("DELETE FROM organizations WHERE tenant_id = $1", tenant_id)
        .execute(&mut *src_tx)
        .await?;
    sqlx::query!("DELETE FROM tenants       WHERE id        = $1", tenant_id)
        .execute(&mut *src_tx)
        .await?;

    src_tx.commit().await?;

    // Mark source outbox record processed — signals that purge completed successfully.
    sqlx::query!(
        "UPDATE migration_outbox SET processed = TRUE
         WHERE tenant_id = $1 AND event_type = 'migration_started' AND processed = FALSE",
        tenant_id
    )
    .execute(source_pool)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requires two live Postgres databases. Run with:
    /// TEST_DATABASE_URL=postgres://idass:idass@localhost:5432/idass_eu_west_1
    /// TEST_DATABASE_URL_2=postgres://idass:idass@localhost:5433/idass_us_east_1
    #[ignore = "requires two separate postgres databases"]
    #[tokio::test]
    async fn migration_moves_tenant_and_purges_source() {
        let source_url = std::env::var("TEST_DATABASE_URL").unwrap();
        let target_url = std::env::var("TEST_DATABASE_URL_2")
            .unwrap_or_else(|_| "postgres://idass:idass@localhost:5433/idass_us_east_1".into());

        let source = sqlx::PgPool::connect(&source_url).await.unwrap();
        let target = sqlx::PgPool::connect(&target_url).await.unwrap();

        sqlx::migrate!("../migrations").run(&source).await.unwrap();
        sqlx::migrate!("../migrations").run(&target).await.unwrap();

        let tenant_id = Uuid::new_v4();
        let conn_id = Uuid::new_v4();

        sqlx::query!(
            "INSERT INTO tenants (id, name, current_region) VALUES ($1, $2, $3)",
            tenant_id,
            format!("migrate-test-{}", &tenant_id.to_string()[..8]),
            "eu-west-1"
        )
        .execute(&source)
        .await
        .unwrap();

        sqlx::query!(
            "INSERT INTO connections (id, tenant_id, name, strategy, options_encrypted) VALUES ($1, $2, $3, $4, $5)",
            conn_id,
            tenant_id,
            "db",
            "database",
            b"x".as_slice()
        )
        .execute(&source)
        .await
        .unwrap();

        sqlx::query!(
            "INSERT INTO users (tenant_id, connection_id, email) VALUES ($1, $2, $3)",
            tenant_id,
            conn_id,
            "migrant@example.com"
        )
        .execute(&source)
        .await
        .unwrap();

        execute_geographic_migration(tenant_id, &source, &target, "us-east-1")
            .await
            .unwrap();

        // Verify: tenant active in target with new region
        let t = sqlx::query!(
            "SELECT status, current_region FROM tenants WHERE id = $1",
            tenant_id
        )
        .fetch_one(&target)
        .await
        .unwrap();
        assert_eq!(t.status, "active");
        assert_eq!(t.current_region, "us-east-1");

        // Verify: source data purged (GDPR)
        let count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM tenants WHERE id = $1",
            tenant_id
        )
        .fetch_one(&source)
        .await
        .unwrap()
        .unwrap_or(0);
        assert_eq!(count, 0);

        // Verify: PII user data is purged from source (GDPR requirement)
        let user_count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM users WHERE tenant_id = $1", tenant_id
        ).fetch_one(&source).await.unwrap().unwrap_or(0);
        assert_eq!(user_count, 0, "PII users must be purged from source");

        // Verify: user data was migrated to target
        let target_user_count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM users WHERE tenant_id = $1", tenant_id
        ).fetch_one(&target).await.unwrap().unwrap_or(0);
        assert_eq!(target_user_count, 1, "User must exist in target after migration");
    }
}
