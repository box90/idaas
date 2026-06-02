pub mod csrf;
pub mod ports;
pub mod router;
pub mod services;

use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub master_key: Arc<[u8; 32]>,
    pub issuer: String,
    pub mgmt_api_key: String,
    pub auth: Arc<dyn ports::AuthPort>,
    pub token: Arc<dyn ports::TokenPort>,
    pub mgmt: Arc<dyn ports::MgmtPort>,
    pub csrf: Arc<dyn ports::CsrfStore>,
}
