use std::sync::Arc;

use axum::{body::Body, http::Request, response::Response, Router};
use chrono::Utc;
use shared_kernel::models::{Tenant, User};
use sqlx::types::Json;
use tower::ServiceExt;
use uuid::Uuid;

// Re-export mock types for use in test files
pub use api_gateway::ports::{
    MockAuthPort, MockCsrfStore, MockMgmtPort, MockTokenPort,
};

// ── TestAppBuilder ────────────────────────────────────────────────────────────

pub struct TestAppBuilder {
    auth: MockAuthPort,
    token: MockTokenPort,
    mgmt: MockMgmtPort,
    csrf: MockCsrfStore,
    issuer: String,
    mgmt_api_key: String,
}

impl TestAppBuilder {
    pub fn new(
        auth: MockAuthPort,
        token: MockTokenPort,
        mgmt: MockMgmtPort,
        csrf: MockCsrfStore,
    ) -> Self {
        Self {
            auth,
            token,
            mgmt,
            csrf,
            issuer: "https://test.idass.example.com".into(),
            mgmt_api_key: "test-mgmt-key".into(),
        }
    }

    pub fn mgmt_api_key(mut self, key: impl Into<String>) -> Self {
        self.mgmt_api_key = key.into();
        self
    }

    pub fn build(self) -> Router {
        let state = api_gateway::AppState {
            master_key: Arc::new([0x42u8; 32]),
            issuer: self.issuer,
            mgmt_api_key: self.mgmt_api_key,
            auth: Arc::new(self.auth),
            token: Arc::new(self.token),
            mgmt: Arc::new(self.mgmt),
            csrf: Arc::new(self.csrf),
        };
        api_gateway::router::build_router(state)
    }
}

// ── Request helpers ───────────────────────────────────────────────────────────

pub fn post_json(uri: &str, body: &serde_json::Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

pub fn post_json_with_auth(
    uri: &str,
    body: &serde_json::Value,
    token: &str,
) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("authorization", format!("Bearer {}", token))
        .body(Body::from(serde_json::to_vec(body).unwrap()))
        .unwrap()
}

pub fn get_with_auth(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap()
}

pub fn delete_with_auth(uri: &str, token: &str) -> Request<Body> {
    Request::builder()
        .method("DELETE")
        .uri(uri)
        .header("authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap()
}

// ── Response helpers ──────────────────────────────────────────────────────────

pub async fn call(app: Router, req: Request<Body>) -> Response {
    app.oneshot(req).await.unwrap()
}

pub async fn body_json(resp: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
}

// ── Fixture builders ──────────────────────────────────────────────────────────

pub fn make_tenant(id: Uuid, name: &str, status: &str) -> Tenant {
    Tenant {
        id,
        name: name.to_owned(),
        current_region: "eu-west-1".into(),
        status: status.to_owned(),
        updated_at: Utc::now(),
    }
}

pub fn make_user(tenant_id: Uuid) -> User {
    User {
        id: Uuid::new_v4(),
        tenant_id,
        organization_id: None,
        connection_id: Uuid::new_v4(),
        email: "test@example.com".into(),
        password_hash: None,
        external_provider_id: None,
        user_metadata: Json(serde_json::json!({})),
        app_metadata: Json(serde_json::json!({})),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}
