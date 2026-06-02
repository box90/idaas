mod helpers;

use axum::http::StatusCode;
use mockall::predicate::eq;
use shared_kernel::error::AppError;
use uuid::Uuid;

use helpers::{
    body_json, call, delete_with_auth, get_with_auth, make_tenant, make_user, post_json,
    post_json_with_auth, MockAuthPort, MockCsrfStore, MockMgmtPort, MockTokenPort, TestAppBuilder,
};

const MGMT_KEY: &str = "test-mgmt-key";

// ── POST /api/v1/:tenant/auth/database ────────────────────────────────────────

#[tokio::test]
async fn db_login_200_valid_credentials() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let user = make_user(tenant_id);
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    let mut auth = MockAuthPort::new();
    auth.expect_authenticate_db()
        .return_once(move |_, _, _| Ok(user));
    let mut token = MockTokenPort::new();
    token
        .expect_mint()
        .return_once(|_, _, _, _, _| Ok("signed.jwt.token".into()));
    let app =
        TestAppBuilder::new(auth, token, mgmt, MockCsrfStore::new()).build();
    let resp = call(
        app,
        post_json(
            "/api/v1/acme/auth/database",
            &serde_json::json!({ "email": "u@example.com", "password": "pw" }),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["token_type"], "Bearer");
    assert!(body["access_token"].as_str().is_some());
}

#[tokio::test]
async fn db_login_401_wrong_password() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    let mut auth = MockAuthPort::new();
    auth.expect_authenticate_db()
        .return_once(|_, _, _| Err(AppError::InvalidCredentials));
    let app = TestAppBuilder::new(auth, MockTokenPort::new(), mgmt, MockCsrfStore::new()).build();
    let resp = call(
        app,
        post_json(
            "/api/v1/acme/auth/database",
            &serde_json::json!({ "email": "u@example.com", "password": "bad" }),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn db_login_404_unknown_tenant() {
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(|_| Err(AppError::TenantNotFound));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        post_json(
            "/api/v1/unknown/auth/database",
            &serde_json::json!({ "email": "u@example.com", "password": "pw" }),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn db_login_503_migrating_tenant() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "migrating");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        post_json(
            "/api/v1/acme/auth/database",
            &serde_json::json!({ "email": "u@example.com", "password": "pw" }),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

// ── GET /api/v1/:tenant/auth/:conn/authorize ──────────────────────────────────

#[tokio::test]
async fn oauth_redirect_302_sets_location_and_cookie() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    let mut auth = MockAuthPort::new();
    auth.expect_generate_oauth_redirect().return_once(|_, _| {
        Ok((
            "https://accounts.google.com/o/oauth2/v2/auth?client_id=cid".into(),
            "csrf-state-value".into(),
        ))
    });
    let mut csrf = MockCsrfStore::new();
    csrf.expect_store().return_once(|_, _, _| Ok(()));
    let app = TestAppBuilder::new(auth, MockTokenPort::new(), mgmt, csrf).build();
    let resp = call(
        app,
        axum::http::Request::builder()
            .method("GET")
            .uri("/api/v1/acme/auth/google/authorize")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FOUND);
    assert!(resp.headers().get("location").is_some());
    let cookie = resp
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cookie.contains("oauth_state="));
    assert!(cookie.contains("HttpOnly"));
    assert!(cookie.contains("SameSite=Lax"));
}

// ── GET /api/v1/:tenant/auth/:conn/callback ───────────────────────────────────

#[tokio::test]
async fn oauth_callback_200_valid_state() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let user = make_user(tenant_id);
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    let mut csrf = MockCsrfStore::new();
    csrf.expect_validate_and_consume()
        .return_once(|_, _| Ok(true));
    let mut auth = MockAuthPort::new();
    auth.expect_exchange_code_oauth()
        .return_once(move |_, _, _| Ok(user));
    let mut token = MockTokenPort::new();
    token
        .expect_mint()
        .return_once(|_, _, _, _, _| Ok("jwt.token".into()));
    let app = TestAppBuilder::new(auth, token, mgmt, csrf).build();
    let resp = call(
        app,
        axum::http::Request::builder()
            .method("GET")
            .uri("/api/v1/acme/auth/google/callback?code=auth-code&state=my-state")
            .header("cookie", "oauth_state=my-state")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["token_type"], "Bearer");
}

#[tokio::test]
async fn oauth_callback_401_missing_state_cookie() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        axum::http::Request::builder()
            .method("GET")
            .uri("/api/v1/acme/auth/google/callback?code=code&state=my-state")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn oauth_callback_401_state_mismatch() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        axum::http::Request::builder()
            .method("GET")
            .uri("/api/v1/acme/auth/google/callback?code=code&state=query-state")
            .header("cookie", "oauth_state=different-state")
            .body(axum::body::Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── POST /api/v1/:tenant/auth/saml/:conn/acs ─────────────────────────────────

#[tokio::test]
async fn saml_acs_400_validation_not_implemented() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    let mut auth = MockAuthPort::new();
    auth.expect_handle_saml_acs()
        .return_once(|_, _, _| Err(AppError::InvalidConnection));
    let app =
        TestAppBuilder::new(auth, MockTokenPort::new(), mgmt, MockCsrfStore::new()).build();
    let resp = call(
        app,
        post_json(
            "/api/v1/acme/auth/saml/corp-idp/acs",
            &serde_json::json!({ "SAMLResponse": "base64data" }),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── POST /api/v1/mgmt/tenants ─────────────────────────────────────────────────

#[tokio::test]
async fn create_tenant_201_success() {
    let new_tenant = make_tenant(Uuid::new_v4(), "newco", "active");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_create_tenant()
        .return_once(move |_| Ok(new_tenant));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
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
    assert_eq!(body_json(resp).await["name"], "newco");
}

#[tokio::test]
async fn create_tenant_401_missing_key() {
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
}

#[tokio::test]
async fn create_tenant_401_wrong_key() {
    let app = TestAppBuilder::new(
        MockAuthPort::new(),
        MockTokenPort::new(),
        MockMgmtPort::new(),
        MockCsrfStore::new(),
    )
    .build();
    let resp = call(
        app,
        post_json_with_auth(
            "/api/v1/mgmt/tenants",
            &serde_json::json!({ "name": "x", "region": "eu-west-1" }),
            "wrong-key",
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

// ── GET /api/v1/mgmt/tenants ──────────────────────────────────────────────────

#[tokio::test]
async fn list_tenants_200() {
    let tenants = vec![
        make_tenant(Uuid::new_v4(), "acme", "active"),
        make_tenant(Uuid::new_v4(), "globex", "active"),
    ];
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_list_tenants().return_once(move || Ok(tenants));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(app, get_with_auth("/api/v1/mgmt/tenants", MGMT_KEY)).await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 2);
}

// ── GET /api/v1/mgmt/tenants/:id ─────────────────────────────────────────────

#[tokio::test]
async fn get_tenant_200() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_get_tenant()
        .with(eq(tenant_id))
        .return_once(move |_| Ok(tenant));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        get_with_auth(&format!("/api/v1/mgmt/tenants/{}", tenant_id), MGMT_KEY),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(body_json(resp).await["name"], "acme");
}

#[tokio::test]
async fn get_tenant_404() {
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_get_tenant()
        .return_once(|_| Err(AppError::TenantNotFound));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
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
}

// ── POST /api/v1/mgmt/tenants/:id/migrate ────────────────────────────────────

#[tokio::test]
async fn migrate_tenant_204_success() {
    let tenant_id = Uuid::new_v4();
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_migrate_tenant()
        .with(eq(tenant_id), eq("us-east-1"))
        .return_once(|_, _| Ok(()));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        post_json_with_auth(
            &format!("/api/v1/mgmt/tenants/{}/migrate", tenant_id),
            &serde_json::json!({ "target_region": "us-east-1" }),
            MGMT_KEY,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn migrate_tenant_404_unknown_tenant() {
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_migrate_tenant()
        .return_once(|_, _| Err(AppError::TenantNotFound));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        post_json_with_auth(
            &format!("/api/v1/mgmt/tenants/{}/migrate", Uuid::new_v4()),
            &serde_json::json!({ "target_region": "us-east-1" }),
            MGMT_KEY,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn migrate_tenant_400_unknown_region() {
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_migrate_tenant()
        .return_once(|_, _| Err(AppError::InvalidConnection));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        post_json_with_auth(
            &format!("/api/v1/mgmt/tenants/{}/migrate", Uuid::new_v4()),
            &serde_json::json!({ "target_region": "nowhere" }),
            MGMT_KEY,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

// ── POST /api/v1/mgmt/:tenant/users ──────────────────────────────────────────

#[tokio::test]
async fn create_user_201_success() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let user = make_user(tenant_id);
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    mgmt.expect_create_user().return_once(move |_| Ok(user));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        post_json_with_auth(
            "/api/v1/mgmt/acme/users",
            &serde_json::json!({ "connection_id": Uuid::new_v4(), "email": "new@example.com" }),
            MGMT_KEY,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn create_user_401_no_key() {
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
            "/api/v1/mgmt/acme/users",
            &serde_json::json!({ "connection_id": Uuid::new_v4(), "email": "new@example.com" }),
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn create_user_404_unknown_tenant() {
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(|_| Err(AppError::TenantNotFound));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        post_json_with_auth(
            "/api/v1/mgmt/ghost/users",
            &serde_json::json!({ "connection_id": Uuid::new_v4(), "email": "new@example.com" }),
            MGMT_KEY,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── GET /api/v1/mgmt/:tenant/users/:user_id ───────────────────────────────────

#[tokio::test]
async fn get_user_200() {
    let tenant_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let mut user = make_user(tenant_id);
    user.id = user_id;
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    mgmt.expect_get_user()
        .with(eq(tenant_id), eq(user_id))
        .return_once(move |_, _| Ok(user));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
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
}

#[tokio::test]
async fn get_user_404() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    mgmt.expect_get_user()
        .return_once(|_, _| Err(AppError::UserNotFound));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        get_with_auth(
            &format!("/api/v1/mgmt/acme/users/{}", Uuid::new_v4()),
            MGMT_KEY,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

// ── DELETE /api/v1/mgmt/:tenant/users/:user_id ────────────────────────────────

#[tokio::test]
async fn delete_user_204_success() {
    let tenant_id = Uuid::new_v4();
    let user_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    mgmt.expect_delete_user()
        .with(eq(tenant_id), eq(user_id))
        .return_once(|_, _| Ok(()));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        delete_with_auth(
            &format!("/api/v1/mgmt/acme/users/{}", user_id),
            MGMT_KEY,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn delete_user_404() {
    let tenant_id = Uuid::new_v4();
    let tenant = make_tenant(tenant_id, "acme", "active");
    let mut mgmt = MockMgmtPort::new();
    mgmt.expect_resolve_tenant_by_name()
        .return_once(move |_| Ok(tenant));
    mgmt.expect_delete_user()
        .return_once(|_, _| Err(AppError::UserNotFound));
    let app =
        TestAppBuilder::new(MockAuthPort::new(), MockTokenPort::new(), mgmt, MockCsrfStore::new())
            .build();
    let resp = call(
        app,
        delete_with_auth(
            &format!("/api/v1/mgmt/acme/users/{}", Uuid::new_v4()),
            MGMT_KEY,
        ),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
