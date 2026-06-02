use std::{collections::HashMap, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;
use shared_kernel::{
    crypto,
    error::AppError,
    models::{
        ConnectionSummary, CreateConnectionDto, CreateTenantDto, CreateUserDto,
        Organization, Tenant, User,
    },
};
use sqlx::PgPool;
use uuid::Uuid;

use crate::ports::{AuthPort, MgmtPort, TokenPort};

// ── Pool resolution helper ────────────────────────────────────────────────────

async fn pool_for_tenant(
    pools: &HashMap<String, PgPool>,
    tenant_id: Uuid,
) -> Result<PgPool, AppError> {
    for pool in pools.values() {
        let count: i64 = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM tenants WHERE id = $1",
            tenant_id
        )
        .fetch_one(pool)
        .await?
        .unwrap_or(0);
        if count > 0 {
            return Ok(pool.clone());
        }
    }
    Err(AppError::TenantNotFound)
}

// ── RealAuthService ───────────────────────────────────────────────────────────

pub struct RealAuthService {
    pub pools: Arc<HashMap<String, PgPool>>,
    pub master_key: Arc<[u8; 32]>,
}

impl RealAuthService {
    pub fn new(pools: Arc<HashMap<String, PgPool>>, master_key: Arc<[u8; 32]>) -> Self {
        Self { pools, master_key }
    }

    async fn named_connection(
        &self,
        pool: &PgPool,
        tenant_id: Uuid,
        name: &str,
    ) -> Result<shared_kernel::models::Connection, AppError> {
        sqlx::query_as!(
            shared_kernel::models::Connection,
            r#"SELECT id, tenant_id, name, strategy, options_encrypted, webhook_url, created_at
               FROM connections WHERE tenant_id = $1 AND name = $2"#,
            tenant_id,
            name
        )
        .fetch_optional(pool)
        .await?
        .ok_or(AppError::InvalidConnection)
    }
}

#[async_trait]
impl AuthPort for RealAuthService {
    async fn authenticate_db(
        &self,
        tenant_id: Uuid,
        email: &str,
        password: &str,
    ) -> Result<User, AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        mod_auth::db_auth::authenticate_db(&pool, tenant_id, email, password).await
    }

    async fn generate_oauth_redirect(
        &self,
        tenant_id: Uuid,
        connection_name: &str,
    ) -> Result<(String, String), AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        let connection = self.named_connection(&pool, tenant_id, connection_name).await?;

        match connection.strategy.as_str() {
            "oidc" => {
                let dec = crypto::decrypt(&self.master_key, &connection.options_encrypted)?;
                let config: shared_kernel::models::OIDCResolvedConfig =
                    serde_json::from_slice(&dec).map_err(|_| AppError::InvalidConnection)?;
                mod_auth::oidc::generate_oidc_redirect(&config)
            }
            _ => {
                let dec = crypto::decrypt(&self.master_key, &connection.options_encrypted)?;
                let opts: mod_auth::oauth_google::GoogleConnectionOptions =
                    serde_json::from_slice(&dec).map_err(|_| AppError::InvalidConnection)?;
                let csrf_state = uuid::Uuid::new_v4().to_string();
                let auth_url = format!(
                    "https://accounts.google.com/o/oauth2/v2/auth\
                     ?client_id={}&redirect_uri={}&response_type=code&scope=openid%20email&state={}",
                    urlencoding::encode(&opts.client_id),
                    urlencoding::encode(&opts.redirect_uri),
                    urlencoding::encode(&csrf_state),
                );
                Ok((auth_url, csrf_state))
            }
        }
    }

    async fn exchange_code_oauth(
        &self,
        tenant_id: Uuid,
        connection_name: &str,
        code: &str,
    ) -> Result<User, AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        let connection = self.named_connection(&pool, tenant_id, connection_name).await?;

        match connection.strategy.as_str() {
            "oidc" => {
                mod_auth::oidc::exchange_code_oidc(
                    &pool, &connection, code, &self.master_key,
                ).await
            }
            _ => {
                mod_auth::oauth_google::exchange_code(
                    &pool, &connection, code, &self.master_key,
                ).await
            }
        }
    }

    async fn handle_saml_acs(
        &self,
        tenant_id: Uuid,
        connection_name: &str,
        saml_response_b64: &str,
    ) -> Result<User, AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        let connection = self.named_connection(&pool, tenant_id, connection_name).await?;
        mod_auth::saml_enterprise::handle_acs_response(
            &pool,
            &connection,
            saml_response_b64,
            &self.master_key,
        )
        .await
    }
}

// ── RealTokenService ──────────────────────────────────────────────────────────

pub struct RealTokenService {
    pub pools: Arc<HashMap<String, PgPool>>,
    pub inner: Arc<mod_token::TokenService>,
}

impl RealTokenService {
    pub fn new(pools: Arc<HashMap<String, PgPool>>, inner: Arc<mod_token::TokenService>) -> Self {
        Self { pools, inner }
    }
}

#[async_trait]
impl TokenPort for RealTokenService {
    async fn mint(
        &self,
        tenant_id: Uuid,
        user: &User,
        connection_name: &str,
        issuer: &str,
        extra_claims: Value,
    ) -> Result<String, AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        // Try by name first; fall back to strategy match (e.g. "database")
        let connection = sqlx::query_as!(
            shared_kernel::models::Connection,
            r#"SELECT id, tenant_id, name, strategy, options_encrypted, webhook_url, created_at
               FROM connections
               WHERE tenant_id = $1 AND (name = $2 OR strategy = $2)
               LIMIT 1"#,
            tenant_id,
            connection_name
        )
        .fetch_optional(&pool)
        .await?
        .ok_or(AppError::InvalidConnection)?;

        let base = serde_json::json!({ "sub": user.id, "email": user.email });
        let enriched = match &connection.webhook_url {
            Some(url) => {
                mod_token::webhooks::enrich_claims(&self.inner.client, url, base).await
            }
            None => extra_claims,
        };

        self.inner
            .mint_token(&pool, user, &connection, issuer, enriched)
            .await
    }
}

// ── RealMgmtService ───────────────────────────────────────────────────────────

pub struct RealMgmtService {
    pub pools: Arc<HashMap<String, PgPool>>,
}

impl RealMgmtService {
    pub fn new(pools: Arc<HashMap<String, PgPool>>) -> Self {
        Self { pools }
    }
}

#[async_trait]
impl MgmtPort for RealMgmtService {
    async fn resolve_tenant_by_name(&self, name: &str) -> Result<Tenant, AppError> {
        for pool in self.pools.values() {
            if let Some(t) = sqlx::query_as!(
                Tenant,
                "SELECT id, name, current_region, status, updated_at FROM tenants WHERE name = $1",
                name
            )
            .fetch_optional(pool)
            .await?
            {
                return Ok(t);
            }
        }
        Err(AppError::TenantNotFound)
    }

    async fn create_tenant(&self, dto: &CreateTenantDto) -> Result<Tenant, AppError> {
        let pool = self
            .pools
            .get(&dto.region)
            .ok_or(AppError::InvalidConnection)?;
        mod_mgmt::tenant_crud::create_tenant(pool, dto).await
    }

    async fn get_tenant(&self, tenant_id: Uuid) -> Result<Tenant, AppError> {
        for pool in self.pools.values() {
            if let Ok(t) = mod_mgmt::tenant_crud::get_tenant(pool, tenant_id).await {
                return Ok(t);
            }
        }
        Err(AppError::TenantNotFound)
    }

    async fn list_tenants(&self) -> Result<Vec<Tenant>, AppError> {
        let pool = self
            .pools
            .values()
            .next()
            .ok_or(AppError::TenantNotFound)?;
        mod_mgmt::tenant_crud::list_tenants(pool).await
    }

    async fn create_user(&self, dto: &CreateUserDto) -> Result<User, AppError> {
        let pool = pool_for_tenant(&self.pools, dto.tenant_id).await?;
        mod_mgmt::user_crud::create_user(&pool, dto).await
    }

    async fn get_user(&self, tenant_id: Uuid, user_id: Uuid) -> Result<User, AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        mod_mgmt::user_crud::get_user(&pool, tenant_id, user_id).await
    }

    async fn delete_user(&self, tenant_id: Uuid, user_id: Uuid) -> Result<(), AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        mod_mgmt::user_crud::delete_user(&pool, tenant_id, user_id).await
    }

    async fn migrate_tenant(&self, tenant_id: Uuid, target_region: &str) -> Result<(), AppError> {
        let source = pool_for_tenant(&self.pools, tenant_id).await?;
        let target = self
            .pools
            .get(target_region)
            .ok_or(AppError::InvalidConnection)?
            .clone();
        mod_mgmt::migration::execute_geographic_migration(
            tenant_id,
            &source,
            &target,
            target_region,
        )
        .await
    }

    async fn list_connections(&self, tenant_id: Uuid) -> Result<Vec<ConnectionSummary>, AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        mod_mgmt::connection_crud::list_connections(&pool, tenant_id).await
    }

    async fn create_connection(&self, dto: &CreateConnectionDto) -> Result<ConnectionSummary, AppError> {
        let pool = pool_for_tenant(&self.pools, dto.tenant_id).await?;
        mod_mgmt::connection_crud::create_connection(&pool, dto).await
    }

    async fn update_connection(
        &self,
        tenant_id: Uuid,
        conn_id: Uuid,
        name: Option<String>,
        options_encrypted: Option<Vec<u8>>,
        webhook_url: Option<Option<String>>,
    ) -> Result<ConnectionSummary, AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        mod_mgmt::connection_crud::update_connection(
            &pool, tenant_id, conn_id,
            name.as_deref(), options_encrypted, webhook_url,
        ).await
    }

    async fn delete_connection(&self, tenant_id: Uuid, conn_id: Uuid) -> Result<(), AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        mod_mgmt::connection_crud::delete_connection(&pool, tenant_id, conn_id).await
    }

    async fn list_organizations(&self, tenant_id: Uuid) -> Result<Vec<Organization>, AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        mod_mgmt::org_crud::list_organizations(&pool, tenant_id).await
    }

    async fn create_organization(
        &self,
        tenant_id: Uuid,
        name: &str,
        display_name: Option<String>,
    ) -> Result<Organization, AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        mod_mgmt::user_crud::create_organization(&pool, tenant_id, name, display_name.as_deref()).await
    }

    async fn delete_organization(&self, tenant_id: Uuid, org_id: Uuid) -> Result<(), AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        mod_mgmt::org_crud::delete_organization(&pool, tenant_id, org_id).await
    }

    async fn list_users(&self, tenant_id: Uuid) -> Result<Vec<User>, AppError> {
        let pool = pool_for_tenant(&self.pools, tenant_id).await?;
        mod_mgmt::user_crud::list_users(&pool, tenant_id).await
    }

    async fn update_tenant(&self, tenant_id: Uuid, name: Option<String>) -> Result<Tenant, AppError> {
        for pool in self.pools.values() {
            if let Ok(t) = mod_mgmt::tenant_crud::update_tenant(pool, tenant_id, name.as_deref()).await {
                return Ok(t);
            }
        }
        Err(AppError::TenantNotFound)
    }
}
