use std::net::SocketAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::middleware;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use tokio::net::TcpListener;
use tracing::info;

use crate::admin::handler::AdminState;
use crate::admin::auth::admin_auth_middleware;

async fn health_handler() -> impl IntoResponse {
    axum::http::StatusCode::OK
}

pub fn health_router() -> Router {
    Router::new().route("/health", get(health_handler))
}

pub fn admin_router(admin_state: Arc<AdminState>, key_hash: Arc<ArcSwap<[u8; 32]>>) -> Router {
    let admin_routes = Router::new()
        .route("/admin/reload", post(crate::admin::handler::reload_handler))
        .route("/admin/status", get(crate::admin::handler::status_handler))
        .route("/admin/rotate-key", post(crate::admin::handler::rotate_key_handler))
        .layer(middleware::from_fn_with_state(key_hash, admin_auth_middleware))
        .with_state(admin_state);

    health_router().merge(admin_routes)
}

pub async fn start_health_check_listener(
    port: u16,
    admin_state: Option<Arc<AdminState>>,
    key_hash: Option<Arc<ArcSwap<[u8; 32]>>>,
) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    let app = match (admin_state, key_hash) {
        (Some(state), Some(hash)) => {
            info!(addr = %local_addr, "Health check + admin listener bound");
            admin_router(state, hash)
        }
        _ => {
            info!(addr = %local_addr, "Health check listener bound (admin disabled)");
            health_router()
        }
    };

    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .map_err(anyhow::Error::from)
    });

    Ok((local_addr, handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use crate::admin::auth::load_admin_key;
    use crate::config::{ConfigReloadHandle, test_fixtures};

    async fn start_test_listener_with_admin(
        key_content: &str,
    ) -> (SocketAddr, Arc<ArcSwap<[u8; 32]>>, tokio::task::JoinHandle<anyhow::Result<()>>) {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("admin-key");
        std::fs::write(&key_path, key_content).unwrap();

        let key_hash = load_admin_key(key_path.to_str().unwrap())
            .unwrap()
            .unwrap();

        let key_hash_arc = Arc::new(ArcSwap::from_pointee(key_hash));

        let config_arc = Arc::new(ArcSwap::from_pointee(
            test_fixtures::test_dynamic_config(),
        ));
        let static_config = test_fixtures::test_static_config();
        let reload_handle = Arc::new(ConfigReloadHandle::new(config_arc, static_config, false));

        let admin_state = Arc::new(AdminState {
            reload_handle,
            config_path: dir.path().join("config.toml").to_string_lossy().to_string(),
            start_time: Instant::now(),
            key_hash: key_hash_arc.clone(),
        });

        let (addr, handle) = start_health_check_listener(0, Some(admin_state), Some(key_hash_arc.clone()))
            .await
            .unwrap();

        (addr, key_hash_arc, handle)
    }

    #[tokio::test]
    async fn test_health_check_returns_200() {
        let (addr, _, handle) = start_test_listener_with_admin("test-admin-key").await;

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{}/health", addr.port()))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        handle.abort();
    }

    #[tokio::test]
    async fn test_health_check_binds_to_localhost() {
        let (addr, _, handle) = start_test_listener_with_admin("test-admin-key").await;
        assert!(addr.ip().is_loopback());
        handle.abort();
    }

    #[tokio::test]
    async fn test_admin_reload_with_valid_token() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("admin-key");
        std::fs::write(&key_path, "test-admin-key\n").unwrap();

        let config_content = r#"
health_check_port = 9900
admin_key_path = "/tmp/test-admin-key"

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "127.0.0.1"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme-cache"
acme_directory = "staging"
acme_contact = "mailto:admin@test.local"

[[listeners.sites]]
host = "test.local"
upstream = "127.0.0.1:8080"
"#;
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, config_content).unwrap();

        let key_hash = load_admin_key(key_path.to_str().unwrap()).unwrap().unwrap();
        let key_hash_arc = Arc::new(ArcSwap::from_pointee(key_hash));

        let config_arc = Arc::new(ArcSwap::from_pointee(
            test_fixtures::test_dynamic_config(),
        ));
        let static_config = test_fixtures::test_static_config();
        let reload_handle = Arc::new(ConfigReloadHandle::new(config_arc, static_config, false));

        let admin_state = Arc::new(AdminState {
            reload_handle,
            config_path: config_path.to_string_lossy().to_string(),
            start_time: Instant::now(),
            key_hash: key_hash_arc.clone(),
        });

        let (addr, handle) = start_health_check_listener(0, Some(admin_state), Some(key_hash_arc.clone()))
            .await
            .unwrap();

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{}/admin/reload", addr.port()))
            .header("Authorization", "Bearer test-admin-key")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "ok");
        handle.abort();
    }

    #[tokio::test]
    async fn test_admin_reload_with_wrong_token_returns_401() {
        let (addr, _, handle) = start_test_listener_with_admin("correct-key").await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{}/admin/reload", addr.port()))
            .header("Authorization", "Bearer wrong-key")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
        handle.abort();
    }

    #[tokio::test]
    async fn test_admin_reload_with_no_token_returns_401() {
        let (addr, _, handle) = start_test_listener_with_admin("some-key").await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{}/admin/reload", addr.port()))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
        handle.abort();
    }

    #[tokio::test]
    async fn test_admin_status_with_valid_token() {
        let (addr, _, handle) = start_test_listener_with_admin("status-key").await;

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{}/admin/status", addr.port()))
            .header("Authorization", "Bearer status-key")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "ok");
        assert!(body["uptime_secs"].is_number());
        assert!(body["sites"].is_number());
        handle.abort();
    }

    #[tokio::test]
    async fn test_admin_rotate_key() {
        let (addr, _key_hash_arc, handle) = start_test_listener_with_admin("initial-key").await;

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{}/admin/rotate-key", addr.port()))
            .header("Authorization", "Bearer initial-key")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(body["status"], "ok");
        let new_key = body["key"].as_str().unwrap();
        assert!(!new_key.is_empty());

        let resp_old = client
            .get(format!("http://127.0.0.1:{}/admin/status", addr.port()))
            .header("Authorization", "Bearer initial-key")
            .send()
            .await
            .unwrap();
        assert_eq!(resp_old.status(), reqwest::StatusCode::UNAUTHORIZED);

        let resp_new = client
            .get(format!("http://127.0.0.1:{}/admin/status", addr.port()))
            .header("Authorization", format!("Bearer {}", new_key))
            .send()
            .await
            .unwrap();
        assert_eq!(resp_new.status(), reqwest::StatusCode::OK);

        handle.abort();
    }

    #[tokio::test]
    async fn test_health_endpoint_always_200() {
        let (addr, _, handle) = start_test_listener_with_admin("any-key").await;

        let client = reqwest::Client::new();
        let resp_no_auth = client
            .get(format!("http://127.0.0.1:{}/health", addr.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(resp_no_auth.status(), reqwest::StatusCode::OK);

        let resp_with_auth = client
            .get(format!("http://127.0.0.1:{}/health", addr.port()))
            .header("Authorization", "Bearer random")
            .send()
            .await
            .unwrap();
        assert_eq!(resp_with_auth.status(), reqwest::StatusCode::OK);

        handle.abort();
    }

    #[tokio::test]
    async fn test_admin_disabled_returns_404() {
        let (addr, handle) = start_health_check_listener(0, None, None).await.unwrap();

        let client = reqwest::Client::new();
        let resp = client
            .post(format!("http://127.0.0.1:{}/admin/reload", addr.port()))
            .header("Authorization", "Bearer some-key")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

        let resp = client
            .get(format!("http://127.0.0.1:{}/admin/status", addr.port()))
            .header("Authorization", "Bearer some-key")
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

        let health_resp = client
            .get(format!("http://127.0.0.1:{}/health", addr.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(health_resp.status(), reqwest::StatusCode::OK);

        handle.abort();
    }
}