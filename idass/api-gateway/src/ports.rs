use async_trait::async_trait;
use serde_json::Value;
use shared_kernel::{
    error::AppError,
    models::{
        ConnectionSummary, CreateConnectionDto, CreateTenantDto, CreateUserDto,
        Organization, Tenant, User,
    },
};
use uuid::Uuid;

/// Authentication operations. Implementations resolve the database pool internally.
#[mockall::automock]
#[async_trait]
pub trait AuthPort: Send + Sync {
    async fn authenticate_db(
        &self,
        tenant_id: Uuid,
        email: &str,
        password: &str,
    ) -> Result<User, AppError>;

    /// Returns `(auth_url, csrf_state)`.
    async fn generate_oauth_redirect(
        &self,
        tenant_id: Uuid,
        connection_name: &str,
    ) -> Result<(String, String), AppError>;

    async fn exchange_code_oauth(
        &self,
        tenant_id: Uuid,
        connection_name: &str,
        code: &str,
    ) -> Result<User, AppError>;

    async fn handle_saml_acs(
        &self,
        tenant_id: Uuid,
        connection_name: &str,
        saml_response_b64: &str,
    ) -> Result<User, AppError>;
}

/// JWT minting. Implementations fetch the connection and apply webhook enrichment internally.
#[mockall::automock]
#[async_trait]
pub trait TokenPort: Send + Sync {
    /// `connection_name` may be a connection name or strategy (e.g. "database").
    async fn mint(
        &self,
        tenant_id: Uuid,
        user: &User,
        connection_name: &str,
        issuer: &str,
        extra_claims: Value,
    ) -> Result<String, AppError>;
}

/// Tenant and user management operations.
#[mockall::automock]
#[async_trait]
pub trait MgmtPort: Send + Sync {
    async fn resolve_tenant_by_name(&self, name: &str) -> Result<Tenant, AppError>;
    async fn create_tenant(&self, dto: &CreateTenantDto) -> Result<Tenant, AppError>;
    async fn get_tenant(&self, tenant_id: Uuid) -> Result<Tenant, AppError>;
    async fn list_tenants(&self) -> Result<Vec<Tenant>, AppError>;
    async fn create_user(&self, dto: &CreateUserDto) -> Result<User, AppError>;
    async fn get_user(&self, tenant_id: Uuid, user_id: Uuid) -> Result<User, AppError>;
    async fn delete_user(&self, tenant_id: Uuid, user_id: Uuid) -> Result<(), AppError>;
    async fn migrate_tenant(&self, tenant_id: Uuid, target_region: &str) -> Result<(), AppError>;

    // Connection management
    async fn list_connections(&self, tenant_id: Uuid) -> Result<Vec<ConnectionSummary>, AppError>;
    async fn create_connection(&self, dto: &CreateConnectionDto) -> Result<ConnectionSummary, AppError>;
    async fn update_connection(
        &self,
        tenant_id: Uuid,
        conn_id: Uuid,
        name: Option<String>,
        options_encrypted: Option<Vec<u8>>,
        webhook_url: Option<Option<String>>,
    ) -> Result<ConnectionSummary, AppError>;
    async fn delete_connection(&self, tenant_id: Uuid, conn_id: Uuid) -> Result<(), AppError>;

    // Organization management
    async fn list_organizations(&self, tenant_id: Uuid) -> Result<Vec<Organization>, AppError>;
    async fn create_organization(
        &self,
        tenant_id: Uuid,
        name: &str,
        display_name: Option<String>,
    ) -> Result<Organization, AppError>;
    async fn delete_organization(&self, tenant_id: Uuid, org_id: Uuid) -> Result<(), AppError>;

    // Extended user management
    async fn list_users(&self, tenant_id: Uuid) -> Result<Vec<User>, AppError>;

    // Tenant update
    async fn update_tenant(&self, tenant_id: Uuid, name: Option<String>) -> Result<Tenant, AppError>;
}

/// Ephemeral CSRF state storage for OAuth2 flows.
#[mockall::automock]
#[async_trait]
pub trait CsrfStore: Send + Sync {
    async fn store(&self, tenant_id: Uuid, state_val: &str, ttl_secs: u64) -> Result<(), AppError>;
    /// Returns `true` and removes the state if it exists; `false` if not found.
    async fn validate_and_consume(&self, tenant_id: Uuid, state_val: &str) -> Result<bool, AppError>;
}
