use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::types::Json;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct Tenant {
    pub id: Uuid,
    pub name: String,
    pub current_region: String,
    pub status: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct Organization {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub display_name: Option<String>,
    pub metadata: Json<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct Connection {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub strategy: String,
    pub options_encrypted: Vec<u8>,
    pub webhook_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Safe API response for connections — omits options_encrypted.
#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct ConnectionSummary {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub name: String,
    pub strategy: String,
    pub webhook_url: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Payload for creating a connection. options_encrypted is pre-encrypted by the handler.
#[derive(Debug, Serialize, Deserialize)]
pub struct CreateConnectionDto {
    pub tenant_id: Uuid,
    pub name: String,
    pub strategy: String,
    pub options_encrypted: Vec<u8>,
    pub webhook_url: Option<String>,
}

/// Payload for updating a tenant's mutable fields.
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateTenantDto {
    pub name: Option<String>,
}

/// Input for creating an OIDC connection.
/// Resolved to `OIDCResolvedConfig` before storage — never stored raw.
#[derive(Debug, Serialize, Deserialize)]
pub struct OIDCCreateInput {
    /// "discover" = fetch {issuer_url}/.well-known/openid-configuration
    /// "custom"   = use the provided discovery_document directly
    pub mode: String,
    pub issuer_url: Option<String>,
    pub discovery_document: Option<serde_json::Value>,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

/// Resolved OIDC configuration stored in options_encrypted.
/// All endpoints are known at creation time — no discovery at authorize/callback.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OIDCResolvedConfig {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct User {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub organization_id: Option<Uuid>,
    pub connection_id: Uuid,
    pub email: String,
    pub password_hash: Option<String>,
    pub external_provider_id: Option<String>,
    pub user_metadata: Json<serde_json::Value>,
    pub app_metadata: Json<serde_json::Value>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct TenantKey {
    pub id: Uuid,
    pub tenant_id: Uuid,
    pub algorithm: String,
    pub private_key_encrypted: Vec<u8>,
    pub public_key_pem: String,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateUserDto {
    pub tenant_id: Uuid,
    pub organization_id: Option<Uuid>,
    pub connection_id: Uuid,
    pub email: String,
    pub password: Option<String>,
    pub external_provider_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateTenantDto {
    pub name: String,
    pub region: String,
}
