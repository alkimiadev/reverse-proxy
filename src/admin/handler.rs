use std::sync::Arc;
use std::time::Instant;

use arc_swap::ArcSwap;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rand::RngCore;
use serde::Serialize;

use crate::config::ConfigReloadHandle;

#[derive(Clone)]
pub struct AdminState {
    pub reload_handle: Arc<ConfigReloadHandle>,
    pub config_path: String,
    pub start_time: Instant,
    pub key_hash: Arc<ArcSwap<[u8; 32]>>,
}

#[derive(Serialize)]
pub struct ReloadResponse {
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Serialize)]
pub struct StatusResponse {
    pub status: &'static str,
    pub uptime_secs: u64,
    pub sites: usize,
}

#[derive(Serialize)]
pub struct RotateKeyResponse {
    pub status: &'static str,
    pub key: String,
}

pub async fn reload_handler(State(state): State<Arc<AdminState>>) -> impl IntoResponse {
    let config_content = match tokio::fs::read_to_string(&state.config_path).await {
        Ok(content) => content,
        Err(e) => {
            tracing::error!("admin reload: failed to read config file: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ReloadResponse {
                    status: "error",
                    message: Some("reload failed".to_string()),
                }),
            );
        }
    };

    let full_config = match crate::config::FullConfig::parse(&config_content) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("admin reload: failed to parse config file: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ReloadResponse {
                    status: "error",
                    message: Some("reload failed".to_string()),
                }),
            );
        }
    };

    let (new_static, new_dynamic) = full_config.into_static_and_dynamic();

    match state.reload_handle.reload(new_static, new_dynamic).await {
        Ok(changed_fields) => {
            if !changed_fields.is_empty() {
                tracing::warn!(
                    "static config fields changed (restart required): {}",
                    changed_fields.join(", ")
                );
            }
            tracing::info!(event = "CONFIG_RELOAD", status = "success", source = "admin");
            (
                StatusCode::OK,
                Json(ReloadResponse {
                    status: "ok",
                    message: None,
                }),
            )
        }
        Err(e) => {
            tracing::error!("admin reload: config reload failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ReloadResponse {
                    status: "error",
                    message: Some("reload failed".to_string()),
                }),
            )
        }
    }
}

pub async fn status_handler(State(state): State<Arc<AdminState>>) -> impl IntoResponse {
    let config = state.reload_handle.load();
    let uptime_secs = state.start_time.elapsed().as_secs();

    (
        StatusCode::OK,
        Json(StatusResponse {
            status: "ok",
            uptime_secs,
            sites: config.sites.len(),
        }),
    )
}

pub async fn rotate_key_handler(State(state): State<Arc<AdminState>>) -> impl IntoResponse {
    let mut new_key = [0u8; 32];
    rand::rng().fill_bytes(&mut new_key);
    let new_key_hex = hex::encode(new_key);

    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(new_key_hex.as_bytes());
    let new_hash: [u8; 32] = hasher.finalize().into();

    state.key_hash.store(Arc::new(new_hash));

    tracing::info!(event = "ADMIN_KEY_ROTATION", status = "success");

    (
        StatusCode::OK,
        Json(RotateKeyResponse {
            status: "ok",
            key: new_key_hex,
        }),
    )
}