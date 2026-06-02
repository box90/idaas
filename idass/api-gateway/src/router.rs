use axum::{
    body::Body,
    extract::{Extension, Path, Query, State},
    http::{header, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use tower_http::cors::CorsLayer;
use axum::http::Method;
use serde::Deserialize;
use shared_kernel::{
    error::AppError,
    models::{CreateTenantDto, CreateUserDto, Tenant},
    crypto,
};
use subtle::ConstantTimeEq;
use uuid::Uuid;

use crate::AppState;

// ── Request types ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct DbLoginRequest {
    email: String,
    password: String,
}

#[derive(Deserialize)]
struct OAuthCallbackQuery {
    code: String,
    state: Option<String>,
}

#[derive(Deserialize)]
struct SamlAcsRequest {
    #[serde(rename = "SAMLResponse")]
    saml_response: String,
}

#[derive(Deserialize)]
struct CreateTenantRequest {
    name: String,
    region: String,
}

#[derive(Deserialize)]
struct CreateUserRequest {
    connection_id: Uuid,
    email: String,
    password: Option<String>,
    external_provider_id: Option<String>,
    organization_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct MigrateRequest {
    target_region: String,
}

#[derive(Deserialize)]
struct CreateConnectionRequest {
    name: String,
    strategy: String,
    options: serde_json::Value,
    webhook_url: Option<String>,
}

#[derive(Deserialize)]
struct UpdateConnectionRequest {
    name: Option<String>,
    options: Option<serde_json::Value>,
    webhook_url: Option<Option<String>>,
}

#[derive(Deserialize)]
struct CreateOrganizationRequest {
    name: String,
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct UpdateTenantRequest {
    name: Option<String>,
}

// ── Middleware ─────────────────────────────────────────────────────────────────

async fn resolve_tenant(
    State(state): State<AppState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let tenant_name = {
        let path = req.uri().path();
        // Middleware runs inside nested routers where Axum strips the nesting prefix.
        // Path here is "/{tenant}/..." — we strip the leading '/' and take the first segment.
        path.trim_start_matches('/')
            .split('/')
            .next()
            .map(str::to_owned)
            .unwrap_or_default()
    };

    let tenant = state.mgmt.resolve_tenant_by_name(&tenant_name).await?;

    if tenant.status == "migrating" || tenant.status == "read_only" {
        return Err(AppError::TenantMigrating);
    }

    req.extensions_mut().insert(tenant);
    Ok(next.run(req).await)
}

async fn require_mgmt_key(
    State(state): State<AppState>,
    req: Request<Body>,
    next: Next,
) -> Result<Response, AppError> {
    let expected = &state.mgmt_api_key;
    let provided = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match provided {
        Some(token) if token.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() == 1 => {
            Ok(next.run(req).await)
        }
        _ => Err(AppError::InvalidCredentials),
    }
}

// ── Auth handlers ─────────────────────────────────────────────────────────────

async fn db_login(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Json(body): Json<DbLoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    let user = state
        .auth
        .authenticate_db(tenant.id, &body.email, &body.password)
        .await?;
    let token = state
        .token
        .mint(tenant.id, &user, "database", &state.issuer, serde_json::json!({}))
        .await?;
    Ok(Json(
        serde_json::json!({ "access_token": token, "token_type": "Bearer" }),
    ))
}

async fn oauth_redirect(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Path((_tenant_name, connection_name)): Path<(String, String)>,
) -> Result<impl IntoResponse, AppError> {
    let (auth_url, csrf_state) = state
        .auth
        .generate_oauth_redirect(tenant.id, &connection_name)
        .await?;
    state.csrf.store(tenant.id, &csrf_state, 300).await?;

    let resp = axum::http::Response::builder()
        .status(StatusCode::FOUND)
        .header(header::LOCATION, auth_url)
        .header(
            header::SET_COOKIE,
            format!(
                "oauth_state={csrf_state}; HttpOnly; Secure; SameSite=Lax; Max-Age=300; Path=/"
            ),
        )
        .body(Body::empty())
        .map_err(|_| AppError::Crypto)?;
    Ok(resp)
}

async fn oauth_callback(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Path((_tenant_name, connection_name)): Path<(String, String)>,
    Query(q): Query<OAuthCallbackQuery>,
    headers: axum::http::HeaderMap,
) -> Result<impl IntoResponse, AppError> {
    let csrf_from_query = q.state.as_deref().ok_or(AppError::InvalidToken)?;
    let csrf_from_cookie = headers
        .get(axum::http::header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|cookies| {
            cookies
                .split("; ")
                .find_map(|p| p.strip_prefix("oauth_state="))
        })
        .map(str::to_owned)
        .ok_or(AppError::InvalidToken)?;

    if csrf_from_query
        .as_bytes()
        .ct_eq(csrf_from_cookie.as_bytes())
        .unwrap_u8()
        != 1
    {
        return Err(AppError::InvalidToken);
    }

    let valid = state
        .csrf
        .validate_and_consume(tenant.id, csrf_from_query)
        .await?;
    if !valid {
        return Err(AppError::InvalidToken);
    }

    let user = state
        .auth
        .exchange_code_oauth(tenant.id, &connection_name, &q.code)
        .await?;
    let token = state
        .token
        .mint(tenant.id, &user, &connection_name, &state.issuer, serde_json::json!({}))
        .await?;
    Ok(Json(
        serde_json::json!({ "access_token": token, "token_type": "Bearer" }),
    ))
}

async fn saml_acs(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Path((_tenant_name, connection_name)): Path<(String, String)>,
    Json(body): Json<SamlAcsRequest>,
) -> Result<impl IntoResponse, AppError> {
    let user = state
        .auth
        .handle_saml_acs(tenant.id, &connection_name, &body.saml_response)
        .await?;
    let token = state
        .token
        .mint(tenant.id, &user, &connection_name, &state.issuer, serde_json::json!({}))
        .await?;
    Ok(Json(
        serde_json::json!({ "access_token": token, "token_type": "Bearer" }),
    ))
}

// ── Management handlers ───────────────────────────────────────────────────────

async fn create_tenant_handler(
    State(state): State<AppState>,
    Json(body): Json<CreateTenantRequest>,
) -> Result<impl IntoResponse, AppError> {
    let lower = body.name.to_lowercase();
    if lower == "mgmt" || lower.starts_with("mgmt-") {
        return Err(AppError::InvalidConnection);
    }
    let tenant = state
        .mgmt
        .create_tenant(&CreateTenantDto {
            name: body.name,
            region: body.region,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(tenant)))
}

async fn get_tenant_handler(
    State(state): State<AppState>,
    Path(tenant_id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let tenant = state.mgmt.get_tenant(tenant_id).await?;
    Ok(Json(tenant))
}

async fn list_tenants_handler(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let tenants = state.mgmt.list_tenants().await?;
    Ok(Json(tenants))
}

async fn create_user_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Json(body): Json<CreateUserRequest>,
) -> Result<impl IntoResponse, AppError> {
    let user = state
        .mgmt
        .create_user(&CreateUserDto {
            tenant_id: tenant.id,
            organization_id: body.organization_id,
            connection_id: body.connection_id,
            email: body.email,
            password: body.password,
            external_provider_id: body.external_provider_id,
        })
        .await?;
    Ok((StatusCode::CREATED, Json(user)))
}

async fn get_user_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Path((_tenant_name, user_id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    let user = state.mgmt.get_user(tenant.id, user_id).await?;
    Ok(Json(user))
}

async fn delete_user_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Path((_tenant_name, user_id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    state.mgmt.delete_user(tenant.id, user_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn migrate_tenant_handler(
    State(state): State<AppState>,
    Path(tenant_id): Path<Uuid>,
    Json(body): Json<MigrateRequest>,
) -> Result<impl IntoResponse, AppError> {
    state.mgmt.migrate_tenant(tenant_id, &body.target_region).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_users_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let users = state.mgmt.list_users(tenant.id).await?;
    Ok(Json(users))
}

async fn list_connections_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let conns = state.mgmt.list_connections(tenant.id).await?;
    Ok(Json(conns))
}

async fn create_connection_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Json(body): Json<CreateConnectionRequest>,
) -> Result<impl IntoResponse, AppError> {
    let options_encrypted = if body.strategy == "oidc" {
        let oidc_input: shared_kernel::models::OIDCCreateInput =
            serde_json::from_value(body.options)
                .map_err(|_| AppError::InvalidConnection)?;
        let resolved = mod_auth::oidc::resolve_oidc_config(&oidc_input).await?;
        let bytes = serde_json::to_vec(&resolved)
            .map_err(|_| AppError::InvalidConnection)?;
        shared_kernel::crypto::encrypt(&state.master_key, &bytes)?
    } else {
        let bytes = serde_json::to_vec(&body.options)
            .map_err(|_| AppError::InvalidConnection)?;
        shared_kernel::crypto::encrypt(&state.master_key, &bytes)?
    };

    let dto = shared_kernel::models::CreateConnectionDto {
        tenant_id: tenant.id,
        name: body.name,
        strategy: body.strategy,
        options_encrypted,
        webhook_url: body.webhook_url,
    };
    let conn = state.mgmt.create_connection(&dto).await?;
    Ok((StatusCode::CREATED, Json(conn)))
}

async fn update_connection_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Path((_tenant_name, conn_id)): Path<(String, Uuid)>,
    Json(body): Json<UpdateConnectionRequest>,
) -> Result<impl IntoResponse, AppError> {
    let options_encrypted = if let Some(opts) = body.options.as_ref() {
        // If options contains "mode", treat as OIDC config update
        let bytes = if opts.get("mode").is_some() {
            let oidc_input: shared_kernel::models::OIDCCreateInput =
                serde_json::from_value(opts.clone())
                    .map_err(|_| AppError::InvalidConnection)?;
            let resolved = mod_auth::oidc::resolve_oidc_config(&oidc_input).await?;
            serde_json::to_vec(&resolved).map_err(|_| AppError::InvalidConnection)?
        } else {
            serde_json::to_vec(opts).map_err(|_| AppError::InvalidConnection)?
        };
        Some(shared_kernel::crypto::encrypt(&state.master_key, &bytes)?)
    } else {
        None
    };

    let conn = state.mgmt
        .update_connection(tenant.id, conn_id, body.name, options_encrypted, body.webhook_url)
        .await?;
    Ok(Json(conn))
}

async fn delete_connection_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Path((_tenant_name, conn_id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    state.mgmt.delete_connection(tenant.id, conn_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_organizations_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
) -> Result<impl IntoResponse, AppError> {
    let orgs = state.mgmt.list_organizations(tenant.id).await?;
    Ok(Json(orgs))
}

async fn create_organization_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Json(body): Json<CreateOrganizationRequest>,
) -> Result<impl IntoResponse, AppError> {
    let org = state.mgmt.create_organization(tenant.id, &body.name, body.display_name).await?;
    Ok((StatusCode::CREATED, Json(org)))
}

async fn delete_organization_handler(
    Extension(tenant): Extension<Tenant>,
    State(state): State<AppState>,
    Path((_tenant_name, org_id)): Path<(String, Uuid)>,
) -> Result<impl IntoResponse, AppError> {
    state.mgmt.delete_organization(tenant.id, org_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_tenant_handler(
    State(state): State<AppState>,
    Path(tenant_id): Path<Uuid>,
    Json(body): Json<UpdateTenantRequest>,
) -> Result<impl IntoResponse, AppError> {
    let tenant = state.mgmt.update_tenant(tenant_id, body.name).await?;
    Ok(Json(tenant))
}

// ── Router assembly ───────────────────────────────────────────────────────────

pub fn build_router(state: AppState) -> Router {
    let tenant_routes = Router::new()
        .route("/:tenant/auth/database", post(db_login))
        .route("/:tenant/auth/:connection/authorize", get(oauth_redirect))
        .route("/:tenant/auth/:connection/callback", get(oauth_callback))
        .route("/:tenant/auth/saml/:connection/acs", post(saml_acs))
        .layer(middleware::from_fn_with_state(state.clone(), resolve_tenant));

    let mgmt_tenant_routes = Router::new()
        .route("/:tenant/users",                 post(create_user_handler).get(list_users_handler))
        .route("/:tenant/users/:user_id",        get(get_user_handler).delete(delete_user_handler))
        .route("/:tenant/connections",           get(list_connections_handler).post(create_connection_handler))
        .route("/:tenant/connections/:conn_id",  axum::routing::put(update_connection_handler).delete(delete_connection_handler))
        .route("/:tenant/organizations",         get(list_organizations_handler).post(create_organization_handler))
        .route("/:tenant/organizations/:org_id", axum::routing::delete(delete_organization_handler))
        .layer(middleware::from_fn_with_state(state.clone(), resolve_tenant));

    let mgmt_routes = Router::new()
        .route("/tenants",             post(create_tenant_handler).get(list_tenants_handler))
        .route("/tenants/:id",         get(get_tenant_handler).put(update_tenant_handler))
        .route("/tenants/:id/migrate", post(migrate_tenant_handler))
        .merge(mgmt_tenant_routes)
        .layer(middleware::from_fn_with_state(state.clone(), require_mgmt_key));

    // Allow only the known admin SPA origins. In production, set ADMIN_ORIGIN to the
    // deployed URL (e.g. https://admin.example.com). Falls back to localhost for dev.
    let admin_origin = std::env::var("ADMIN_ORIGIN")
        .unwrap_or_else(|_| "http://localhost:5173".into());
    let cors = CorsLayer::new()
        .allow_origin(
            admin_origin
                .parse::<axum::http::HeaderValue>()
                .expect("ADMIN_ORIGIN must be a valid HTTP origin"),
        )
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);

    Router::new()
        .nest("/api/v1", tenant_routes)
        .nest("/api/v1/mgmt", mgmt_routes)
        .layer(cors)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state)
}
