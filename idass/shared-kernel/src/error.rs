use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("Tenant is locked for an ongoing migration")]
    TenantMigrating,
    #[error("Invalid credentials or user not found")]
    InvalidCredentials,
    #[error("Tenant not found")]
    TenantNotFound,
    #[error("User not found")]
    UserNotFound,
    #[error("Internal database error")]
    Database(#[from] sqlx::Error),
    #[error("Cryptographic processing error")]
    Crypto,
    #[error("External webhook timeout or failure")]
    WebhookTimeout,
    #[error("Invalid token")]
    InvalidToken,
    #[error("Invalid connection configuration")]
    InvalidConnection,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            AppError::TenantMigrating    => (StatusCode::SERVICE_UNAVAILABLE,   "Tenant is locked for an ongoing migration".to_string()),
            AppError::InvalidCredentials => (StatusCode::UNAUTHORIZED,           "Invalid credentials or user not found".to_string()),
            AppError::TenantNotFound     => (StatusCode::NOT_FOUND,             "Tenant not found".to_string()),
            AppError::UserNotFound       => (StatusCode::NOT_FOUND,             "User not found".to_string()),
            AppError::Database(_)        => (StatusCode::INTERNAL_SERVER_ERROR, "Internal database error".to_string()),
            AppError::Crypto             => (StatusCode::INTERNAL_SERVER_ERROR, "Cryptographic processing error".to_string()),
            AppError::WebhookTimeout     => (StatusCode::GATEWAY_TIMEOUT,       "External webhook timeout or failure".to_string()),
            AppError::InvalidToken       => (StatusCode::UNAUTHORIZED,           "Invalid token".to_string()),
            AppError::InvalidConnection  => (StatusCode::BAD_REQUEST,           "Invalid connection configuration".to_string()),
        };
        (status, Json(serde_json::json!({ "error": msg }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_credentials_maps_to_401() {
        let r = AppError::InvalidCredentials.into_response();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn tenant_migrating_maps_to_503() {
        let r = AppError::TenantMigrating.into_response();
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn webhook_timeout_maps_to_504() {
        let r = AppError::WebhookTimeout.into_response();
        assert_eq!(r.status(), StatusCode::GATEWAY_TIMEOUT);
    }
}
