use async_trait::async_trait;
use dashmap::DashMap;
use redis::{aio::ConnectionManager, AsyncCommands};
use shared_kernel::error::AppError;
use uuid::Uuid;

use crate::ports::CsrfStore;

// ── RedisCsrfStore (production) ───────────────────────────────────────────────

pub struct RedisCsrfStore {
    redis: ConnectionManager,
}

impl RedisCsrfStore {
    pub fn new(redis: ConnectionManager) -> Self {
        Self { redis }
    }

    fn key(tenant_id: Uuid, state_val: &str) -> String {
        format!("oauth_state:{}:{}", tenant_id, state_val)
    }
}

#[async_trait]
impl CsrfStore for RedisCsrfStore {
    async fn store(
        &self,
        tenant_id: Uuid,
        state_val: &str,
        ttl_secs: u64,
    ) -> Result<(), AppError> {
        let mut conn = self.redis.clone();
        conn.set_ex::<_, _, ()>(&Self::key(tenant_id, state_val), "1", ttl_secs)
            .await
            .map_err(|_| AppError::WebhookTimeout)
    }

    async fn validate_and_consume(
        &self,
        tenant_id: Uuid,
        state_val: &str,
    ) -> Result<bool, AppError> {
        let key = Self::key(tenant_id, state_val);
        let mut conn = self.redis.clone();
        let found: Option<String> = conn
            .get_del(&key)
            .await
            .map_err(|_| AppError::WebhookTimeout)?;
        Ok(found.is_some())
    }
}

// ── InMemoryCsrfStore (tests) ─────────────────────────────────────────────────

pub struct InMemoryCsrfStore {
    states: DashMap<String, ()>,
}

impl InMemoryCsrfStore {
    pub fn new() -> Self {
        Self {
            states: DashMap::new(),
        }
    }
}

impl Default for InMemoryCsrfStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CsrfStore for InMemoryCsrfStore {
    async fn store(
        &self,
        tenant_id: Uuid,
        state_val: &str,
        _ttl_secs: u64,
    ) -> Result<(), AppError> {
        self.states
            .insert(format!("{}:{}", tenant_id, state_val), ());
        Ok(())
    }

    async fn validate_and_consume(
        &self,
        tenant_id: Uuid,
        state_val: &str,
    ) -> Result<bool, AppError> {
        let key = format!("{}:{}", tenant_id, state_val);
        Ok(self.states.remove(&key).is_some())
    }
}
