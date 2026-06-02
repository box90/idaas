//! Contract tests: validate actual HTTP responses against openapi.yaml.
mod helpers;

use axum::http::StatusCode;
use helpers::{
    body_json, call, delete_with_auth, get_with_auth, make_tenant, make_user, post_json,
    post_json_with_auth, MockAuthPort, MockCsrfStore, MockMgmtPort, MockTokenPort, TestAppBuilder,
};
use jsonschema::JSONSchema;
use openapiv3::{OpenAPI, ReferenceOr, StatusCode as OAStatusCode};
use shared_kernel::error::AppError;
use std::sync::OnceLock;
use uuid::Uuid;

const MGMT_KEY: &str = "test-mgmt-key";

// ── Spec loading ──────────────────────────────────────────────────────────────

static SPEC: OnceLock<OpenAPI> = OnceLock::new();

fn spec() -> &'static OpenAPI {
    SPEC.get_or_init(|| {
        let yaml = std::fs::read_to_string(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../openapi.yaml"),
        )
        .expect("openapi.yaml not found at workspace root");
        serde_yaml::from_str(&yaml).expect("invalid OpenAPI YAML")
    })
}

/// Resolve a local `$ref` of the form `#/components/schemas/Foo` to a raw JSON value.
fn resolve_component_ref(reference: &str) -> Option<serde_json::Value> {
    let name = reference.strip_prefix("#/components/schemas/")?;
    let components = spec().components.as_ref()?;
    let schema_ref = components.schemas.get(name)?;
    match schema_ref {
        ReferenceOr::Item(schema) => Some(normalize_schema(serde_json::to_value(schema).unwrap())),
        ReferenceOr::Reference { .. } => None,
    }
}

/// Post-process a serialized OpenAPI schema so it is valid JSON Schema Draft7:
///  1. Inline all `$ref: "#/components/schemas/X"` occurrences.
///  2. Convert OAS3 `nullable: true` + `type: "T"` → `type: ["T", "null"]`.
fn normalize_schema(val: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;

    match &val {
        Value::Object(map) => {
            // 1. Resolve $ref first (other keys are ignored when $ref is present in Draft7).
            if let Some(Value::String(reference)) = map.get("$ref") {
                if let Some(resolved) = resolve_component_ref(reference) {
                    return resolved;
                }
            }

            // 2. Clone and recurse into all children.
            let mut new_map = serde_json::Map::new();
            for (k, v) in map {
                new_map.insert(k.clone(), normalize_schema(v.clone()));
            }

            // 3. Convert nullable: true + type: "T" → type: ["T", "null"].
            if new_map.get("nullable") == Some(&Value::Bool(true)) {
                new_map.remove("nullable");
                if let Some(Value::String(t)) = new_map.get("type").cloned() {
                    new_map.insert(
                        "type".to_string(),
                        Value::Array(vec![Value::String(t), Value::String("null".into())]),
                    );
                }
            }

            Value::Object(new_map)
        }
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| normalize_schema(v.clone())).collect())
        }
        _ => val,
    }
}

/// Extract the JSON Schema for a response body from the spec, if any.
/// `status` is a string like "200", "401", etc.
fn response_schema(path: &str, method: &str, status: &str) -> Option<serde_json::Value> {
    let status_code: u16 = status.parse().ok()?;
    let oa_status = OAStatusCode::Code(status_code);

    let path_item_ref = spec().paths.paths.get(path)?;
    let path_item = path_item_ref.as_item()?;

    let operation = match method {
        "get" => path_item.get.as_ref(),
        "post" => path_item.post.as_ref(),
        "delete" => path_item.delete.as_ref(),
        "put" => path_item.put.as_ref(),
        "patch" => path_item.patch.as_ref(),
        _ => None,
    }?;

    let response_ref = operation.responses.responses.get(&oa_status)?;
    let response = response_ref.as_item()?;

    let media = response.content.get("application/json")?;
    let schema_ref = media.schema.as_ref()?;

    let raw = match schema_ref {
        ReferenceOr::Item(schema) => serde_json::to_value(schema).unwrap(),
        ReferenceOr::Reference { reference } => {
            // Top-level $ref: inline immediately.
            return resolve_component_ref(reference);
        }
    };
    Some(normalize_schema(raw))
}

fn assert_schema(body: &serde_json::Value, path: &str, method: &str, status: &str) {
    let schema_json = match response_schema(path, method, status) {
        Some(s) => s,
        None => panic!(
            "No response schema found in spec for {method} {path} {status} — add it to openapi.yaml"
        ),
    };
    let compiled = JSONSchema::options()
        .with_draft(jsonschema::Draft::Draft7)
        .compile(&schema_json)
        .unwrap_or_else(|e| {
            panic!("Failed to compile schema for {method} {path} {status}: {e}")
        });
    // Collect errors eagerly so the iterator (which borrows `compiled`) is
    // dropped before `compiled` itself goes out of scope.
    let msgs: Vec<String> = compiled
        .validate(body)
        .err()
        .into_iter()
        .flatten()
        .map(|e| e.to_string())
        .collect();
    if !msgs.is_empty() {
        panic!(
            "Response for {method} {path} {status} violates spec:\n{}",
            msgs.join("\n")
        );
    }
}

// ── Contract: POST /{tenant}/auth/database ────────────────────────────────────

#[tokio::test]
async fn contract_db_login_200() {
    let tenant_id = Uuid::new_v4();
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(make_tenant(tenant_id, "acme", "active")));
    let mut auth = MockAuthPort::new();
    auth.expect_authenticate_db()
        .return_once(move |_, _, _| Ok(make_user(tenant_id)));
    let mut token = MockTokenPort::new();
    token
        .expect_mint()
        .return_once(|_, _, _, _, _| Ok("signed.jwt.here".into()));

    let app = TestAppBuilder::new(auth, token, mgmt, MockCsrfStore::new()).build();
    let resp = call(
        app,
        post_json(
            "/api/v1/acme/auth/database",
            &serde_json::json!({ "email": "u@test.com", "password": "pw" }),
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_schema(&body, "/{tenant}/auth/database", "post", "200");
}

#[tokio::test]
async fn contract_db_login_401() {
    let tenant_id = Uuid::new_v4();
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(make_tenant(tenant_id, "acme", "active")));
    let mut auth = MockAuthPort::new();
    auth.expect_authenticate_db()
        .return_once(|_, _, _| Err(AppError::InvalidCredentials));

    let app =
        TestAppBuilder::new(auth, MockTokenPort::new(), mgmt, MockCsrfStore::new()).build();
    let resp = call(
        app,
        post_json(
            "/api/v1/acme/auth/database",
            &serde_json::json!({ "email": "u@test.com", "password": "bad" }),
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_json(resp).await;
    assert_schema(&body, "/{tenant}/auth/database", "post", "401");
}

#[tokio::test]
async fn contract_db_login_404() {
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(|_| Err(AppError::TenantNotFound));

    let app = TestAppBuilder::new(
        MockAuthPort::new(),
        MockTokenPort::new(),
        mgmt,
        MockCsrfStore::new(),
    )
    .build();
    let resp = call(
        app,
        post_json(
            "/api/v1/ghost/auth/database",
            &serde_json::json!({ "email": "u@test.com", "password": "pw" }),
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_schema(&body, "/{tenant}/auth/database", "post", "404");
}

// ── Contract: POST /mgmt/tenants ─────────────────────────────────────────────

#[tokio::test]
async fn contract_create_tenant_201() {
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_create_tenant()
        .return_once(move |_| Ok(make_tenant(Uuid::new_v4(), "newco", "active")));

    let app = TestAppBuilder::new(
        MockAuthPort::new(),
        MockTokenPort::new(),
        mgmt,
        MockCsrfStore::new(),
    )
    .build();
    let resp = call(
        app,
        post_json_with_auth(
            "/api/v1/mgmt/tenants",
            &serde_json::json!({ "name": "newco", "region": "eu-west-1" }),
            MGMT_KEY,
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp).await;
    assert_schema(&body, "/mgmt/tenants", "post", "201");
}

#[tokio::test]
async fn contract_create_tenant_401() {
    let app = TestAppBuilder::new(
        MockAuthPort::new(),
        MockTokenPort::new(),
        MockMgmtPort::new(),
        MockCsrfStore::new(),
    )
    .build();
    let resp = call(
        app,
        post_json(
            "/api/v1/mgmt/tenants",
            &serde_json::json!({ "name": "x", "region": "eu-west-1" }),
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_json(resp).await;
    assert_schema(&body, "/mgmt/tenants", "post", "401");
}

// ── Contract: GET /mgmt/tenants ───────────────────────────────────────────────

#[tokio::test]
async fn contract_list_tenants_200() {
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_list_tenants()
        .return_once(|| Ok(vec![make_tenant(Uuid::new_v4(), "acme", "active")]));

    let app = TestAppBuilder::new(
        MockAuthPort::new(),
        MockTokenPort::new(),
        mgmt,
        MockCsrfStore::new(),
    )
    .build();
    let resp = call(app, get_with_auth("/api/v1/mgmt/tenants", MGMT_KEY)).await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_schema(&body, "/mgmt/tenants", "get", "200");
}

// ── Contract: GET /mgmt/tenants/:id ──────────────────────────────────────────

#[tokio::test]
async fn contract_get_tenant_200() {
    let tenant_id = Uuid::new_v4();
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_get_tenant()
        .return_once(move |_| Ok(make_tenant(tenant_id, "acme", "active")));

    let app = TestAppBuilder::new(
        MockAuthPort::new(),
        MockTokenPort::new(),
        mgmt,
        MockCsrfStore::new(),
    )
    .build();
    let resp = call(
        app,
        get_with_auth(&format!("/api/v1/mgmt/tenants/{}", tenant_id), MGMT_KEY),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_schema(&body, "/mgmt/tenants/{id}", "get", "200");
}

#[tokio::test]
async fn contract_get_tenant_404() {
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_get_tenant()
        .return_once(|_| Err(AppError::TenantNotFound));

    let app = TestAppBuilder::new(
        MockAuthPort::new(),
        MockTokenPort::new(),
        mgmt,
        MockCsrfStore::new(),
    )
    .build();
    let resp = call(
        app,
        get_with_auth(
            &format!("/api/v1/mgmt/tenants/{}", Uuid::new_v4()),
            MGMT_KEY,
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_schema(&body, "/mgmt/tenants/{id}", "get", "404");
}

// ── Contract: GET /mgmt/{tenant}/users/:user_id ───────────────────────────────

#[tokio::test]
async fn contract_get_user_200() {
    let tenant_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let mut user = make_user(tenant_id);
    user.id = user_id;

    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(make_tenant(tenant_id, "acme", "active")));
    mgmt.expect_get_user()
        .return_once(move |_, _| Ok(user));

    let app = TestAppBuilder::new(
        MockAuthPort::new(),
        MockTokenPort::new(),
        mgmt,
        MockCsrfStore::new(),
    )
    .build();
    let resp = call(
        app,
        get_with_auth(
            &format!("/api/v1/mgmt/acme/users/{}", user_id),
            MGMT_KEY,
        ),
    )
    .await;

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_schema(&body, "/mgmt/{tenant}/users/{user_id}", "get", "200");
}

// ── Contract: DELETE /mgmt/{tenant}/users/:user_id ────────────────────────────

#[tokio::test]
async fn contract_delete_user_204() {
    let tenant_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();

    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(make_tenant(tenant_id, "acme", "active")));
    mgmt.expect_delete_user().return_once(|_, _| Ok(()));

    let app = TestAppBuilder::new(
        MockAuthPort::new(),
        MockTokenPort::new(),
        mgmt,
        MockCsrfStore::new(),
    )
    .build();
    let resp = call(
        app,
        delete_with_auth(
            &format!("/api/v1/mgmt/acme/users/{}", user_id),
            MGMT_KEY,
        ),
    )
    .await;

    // 204 No Content — no body to validate against schema
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

// ── Contract: POST /mgmt/{tenant}/users ──────────────────────────────────────

#[tokio::test]
async fn contract_create_user_201() {
    let tenant_id = Uuid::new_v4();
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(make_tenant(tenant_id, "acme", "active")));
    mgmt.expect_create_user()
        .return_once(move |_| Ok(make_user(tenant_id)));

    let app = TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new()).build();
    let resp = call(app, post_json_with_auth(
        "/api/v1/mgmt/acme/users",
        &serde_json::json!({ "connection_id": Uuid::new_v4(), "email": "new@test.com" }),
        MGMT_KEY,
    )).await;

    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = body_json(resp).await;
    assert_schema(&body, "/mgmt/{tenant}/users", "post", "201");
}

// ── Contract: POST /mgmt/tenants/{id}/migrate ─────────────────────────────────

#[tokio::test]
async fn contract_migrate_tenant_204() {
    let tenant_id = Uuid::new_v4();
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_migrate_tenant()
        .return_once(|_, _| Ok(()));

    let app = TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new()).build();
    let resp = call(app, post_json_with_auth(
        &format!("/api/v1/mgmt/tenants/{}/migrate", tenant_id),
        &serde_json::json!({ "target_region": "us-east-1" }),
        MGMT_KEY,
    )).await;

    // 204 No Content — no body to validate
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn contract_migrate_tenant_401() {
    let app = TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), MockMgmtPort::new(), MockCsrfStore::new()).build();
    let resp = call(app, post_json(
        &format!("/api/v1/mgmt/tenants/{}/migrate", Uuid::new_v4()),
        &serde_json::json!({ "target_region": "us-east-1" }),
    )).await;

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let body = body_json(resp).await;
    assert_schema(&body, "/mgmt/tenants/{id}/migrate", "post", "401");
}

#[tokio::test]
async fn contract_migrate_tenant_404() {
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_migrate_tenant()
        .return_once(|_, _| Err(AppError::TenantNotFound));

    let app = TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new()).build();
    let resp = call(app, post_json_with_auth(
        &format!("/api/v1/mgmt/tenants/{}/migrate", Uuid::new_v4()),
        &serde_json::json!({ "target_region": "us-east-1" }),
        MGMT_KEY,
    )).await;

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = body_json(resp).await;
    assert_schema(&body, "/mgmt/tenants/{id}/migrate", "post", "404");
}

// ── Drift detection ───────────────────────────────────────────────────────────

/// Fails if a route expected in the spec is missing from openapi.yaml.
/// Add new routes to openapi.yaml or this test will fail CI.
#[test]
fn all_routes_are_documented_in_spec() {
    let spec = spec();
    let documented: Vec<&str> = spec
        .paths
        .paths
        .keys()
        .map(String::as_str)
        .collect();

    let required = [
        "/{tenant}/auth/database",
        "/{tenant}/auth/{connection}/authorize",
        "/{tenant}/auth/{connection}/callback",
        "/{tenant}/auth/saml/{connection}/acs",
        "/mgmt/tenants",
        "/mgmt/tenants/{id}",
        "/mgmt/tenants/{id}/migrate",
        "/mgmt/{tenant}/users",
        "/mgmt/{tenant}/users/{user_id}",
    ];

    for route in required {
        assert!(
            documented.contains(&route),
            "Route `{route}` is missing from openapi.yaml — update the spec before merging"
        );
    }
}
