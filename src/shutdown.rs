use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use tokio::sync::watch;

pub struct GracefulShutdown {
    shutdown_timeout: Duration,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
    shutdown_requested: Arc<AtomicBool>,
}

impl GracefulShutdown {
    pub fn new(shutdown_timeout_secs: u64) -> Self {
        let shutdown_requested = Arc::new(AtomicBool::new(false));
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Self {
            shutdown_timeout: Duration::from_secs(shutdown_timeout_secs),
            shutdown_tx,
            shutdown_rx,
            shutdown_requested,
        }
    }

    pub fn shutdown_timeout(&self) -> Duration {
        self.shutdown_timeout
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested.load(Ordering::SeqCst)
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.shutdown_rx.clone()
    }

    pub fn trigger_shutdown(&self) {
        self.shutdown_requested.store(true, Ordering::SeqCst);
        let _ = self.shutdown_tx.send(true);
    }
}

pub fn register_signal_handlers(
    shutdown: Arc<GracefulShutdown>,
    reload_handle: Arc<crate::config::ConfigReloadHandle>,
    config_path: String,
) -> anyhow::Result<()> {
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGHUP])?;
    let (tx, mut rx) = tokio::sync::mpsc::channel::<i32>(16);

    std::thread::spawn(move || {
        for sig in signals.forever() {
            if tx.blocking_send(sig).is_err() {
                break;
            }
        }
    });

    tokio::spawn(async move {
        while let Some(sig) = rx.recv().await {
            match sig {
                SIGTERM | SIGINT => {
                    tracing::info!(event = "SIGNAL", signal = %sig);
                    shutdown.trigger_shutdown();
                    break;
                }
                SIGHUP => {
                    tracing::info!(event = "SIGNAL", signal = "SIGHUP");
                    handle_sighup_reload(&reload_handle, &config_path).await;
                }
                _ => {
                    tracing::debug!(event = "SIGNAL", signal = %sig);
                }
            }
        }
    });

    Ok(())
}

pub async fn handle_sighup_reload(
    reload_handle: &Arc<crate::config::ConfigReloadHandle>,
    config_path: &str,
) {
    let result = crate::config::read_and_validate_config(
        config_path,
        reload_handle.cli_allow_wildcard_bind(),
    )
    .await;

    let (new_static, new_dynamic) = match result {
        Ok(configs) => configs,
        Err(e) => {
            tracing::error!(event = "CONFIG_RELOAD", status = "error", error = %e);
            return;
        }
    };

    match reload_handle.reload(new_static, new_dynamic).await {
        Ok(changed_fields) => {
            if !changed_fields.is_empty() {
                tracing::warn!(
                    event = "CONFIG_RELOAD",
                    status = "warning",
                    "static config fields changed (restart required): {}",
                    changed_fields.join(", ")
                );
            }
            tracing::info!(event = "CONFIG_RELOAD", status = "success");
        }
        Err(e) => {
            tracing::error!(event = "CONFIG_RELOAD", status = "error", error = %e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graceful_shutdown_new_default_timeout() {
        let shutdown = GracefulShutdown::new(30);
        assert_eq!(shutdown.shutdown_timeout(), Duration::from_secs(30));
        assert!(!shutdown.is_shutdown_requested());
    }

    #[test]
    fn graceful_shutdown_trigger() {
        let shutdown = GracefulShutdown::new(10);
        assert!(!shutdown.is_shutdown_requested());

        shutdown.trigger_shutdown();
        assert!(shutdown.is_shutdown_requested());
    }

    #[test]
    fn graceful_shutdown_subscribe_receives_signal() {
        let shutdown = GracefulShutdown::new(5);
        let mut rx = shutdown.subscribe();

        assert!(!*rx.borrow_and_update());

        shutdown.trigger_shutdown();
        assert!(rx.has_changed().unwrap());
        assert!(*rx.borrow_and_update());
    }

    #[test]
    fn graceful_shutdown_custom_timeout() {
        let shutdown = GracefulShutdown::new(60);
        assert_eq!(shutdown.shutdown_timeout(), Duration::from_secs(60));
    }

    #[tokio::test]
    async fn sighup_reload_valid_config() {
        use crate::config::test_fixtures;

        let config_arc = Arc::new(arc_swap::ArcSwap::from_pointee(
            test_fixtures::test_dynamic_config(),
        ));
        let static_config = test_fixtures::test_static_config();
        let reload_handle = Arc::new(crate::config::ConfigReloadHandle::new(
            config_arc.clone(),
            static_config,
            false,
        ));

        let dir = tempfile::tempdir().unwrap();
        let config_content = r#"
health_check_port = 9900
admin_key_path = "/tmp/test-admin-key"

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 20
burst = 40

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
        tokio::fs::write(&config_path, config_content)
            .await
            .unwrap();

        handle_sighup_reload(&reload_handle, config_path.to_str().unwrap()).await;

        let loaded = reload_handle.load();
        assert_eq!(loaded.rate_limit.requests_per_second, 20);
        assert_eq!(loaded.rate_limit.burst, 40);
    }

    #[tokio::test]
    async fn sighup_reload_invalid_config_keeps_old() {
        use crate::config::test_fixtures;

        let config_arc = Arc::new(arc_swap::ArcSwap::from_pointee(
            test_fixtures::test_dynamic_config(),
        ));
        let static_config = test_fixtures::test_static_config();
        let reload_handle = Arc::new(crate::config::ConfigReloadHandle::new(
            config_arc.clone(),
            static_config,
            false,
        ));

        let dir = tempfile::tempdir().unwrap();
        let config_content = "invalid toml {{{";
        let config_path = dir.path().join("config.toml");
        tokio::fs::write(&config_path, config_content)
            .await
            .unwrap();

        handle_sighup_reload(&reload_handle, config_path.to_str().unwrap()).await;

        let loaded = reload_handle.load();
        assert_eq!(loaded.rate_limit.requests_per_second, 10);
    }

    #[tokio::test]
    async fn sighup_reload_missing_file_logs_error() {
        use crate::config::test_fixtures;

        let config_arc = Arc::new(arc_swap::ArcSwap::from_pointee(
            test_fixtures::test_dynamic_config(),
        ));
        let static_config = test_fixtures::test_static_config();
        let reload_handle = Arc::new(crate::config::ConfigReloadHandle::new(
            config_arc.clone(),
            static_config,
            false,
        ));

        handle_sighup_reload(&reload_handle, "/nonexistent/config.toml").await;

        let loaded = reload_handle.load();
        assert_eq!(loaded.rate_limit.requests_per_second, 10);
    }
}
