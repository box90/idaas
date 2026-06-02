use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use api_gateway::{csrf, router, services, AppState};
use dotenvy::dotenv;
use shared_kernel::crypto::master_key_from_env;
use sqlx::postgres::PgPoolOptions;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer())
        .init();

    let master_key   = Arc::new(master_key_from_env());
    let issuer       = std::env::var("ISSUER")
        .unwrap_or_else(|_| "https://idass.example.com".into());
    let mgmt_api_key = std::env::var("MANAGEMENT_API_KEY")
        .expect("MANAGEMENT_API_KEY env var required");

    let region_list = std::env::var("REGIONS")
        .unwrap_or_else(|_| "eu-west-1".into());
    let mut pools: HashMap<String, sqlx::PgPool> = HashMap::new();
    for region in region_list.split(',').map(str::trim) {
        let env_key = format!(
            "DATABASE_URL_{}",
            region.replace('-', "_").to_uppercase()
        );
        let url = std::env::var(&env_key)
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| panic!("No database URL for region {region}"));
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .connect(&url)
            .await
            .unwrap_or_else(|e| panic!("Pool error for region {region}: {e}"));
        sqlx::migrate!("../migrations")
            .run(&pool)
            .await
            .unwrap_or_else(|e| panic!("Migration failed for region {region}: {e}"));
        tracing::info!(region, "migrations applied");
        pools.insert(region.to_owned(), pool);
    }
    let pools = Arc::new(pools);

    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1/".into());
    let redis_client = redis::Client::open(redis_url).expect("invalid Redis URL");
    let redis = redis::aio::ConnectionManager::new(redis_client)
        .await
        .expect("failed to connect to Redis");

    let token_inner = Arc::new(mod_token::TokenService::new(*master_key));

    let state = AppState {
        master_key:   master_key.clone(),
        issuer:       issuer.clone(),
        mgmt_api_key,
        auth:  Arc::new(services::RealAuthService::new(pools.clone(), master_key.clone())),
        token: Arc::new(services::RealTokenService::new(pools.clone(), token_inner)),
        mgmt:  Arc::new(services::RealMgmtService::new(pools)),
        csrf:  Arc::new(csrf::RedisCsrfStore::new(redis)),
    };

    let app = router::build_router(state);

    let port: u16 = std::env::var("APP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    tracing::info!(%addr, "IDaaS server starting");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind {addr}: {e}"));
    axum::serve(listener, app.into_make_service())
        .await
        .expect("server error");
}
