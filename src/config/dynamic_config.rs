use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::Deserialize;
use tokio::sync::Mutex;

use super::static_config::StaticConfig;
use super::validation::validate;

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct DynamicConfig {
    pub sites: Vec<SiteConfig>,
    pub rate_limit: RateLimitConfig,
    pub body: BodyConfig,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct SiteConfig {
    pub host: String,
    pub upstream: String,
    #[serde(default = "default_upstream_scheme")]
    pub upstream_scheme: String,
    #[serde(default = "default_connect_timeout")]
    pub upstream_connect_timeout_secs: u64,
    #[serde(default = "default_request_timeout")]
    pub upstream_request_timeout_secs: u64,
}

#[allow(dead_code)]
fn default_upstream_scheme() -> String {
    "http".to_string()
}

#[allow(dead_code)]
fn default_connect_timeout() -> u64 {
    5
}

#[allow(dead_code)]
fn default_request_timeout() -> u64 {
    60
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub burst: u32,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct BodyConfig {
    pub limit_bytes: u64,
}

#[allow(dead_code)]
pub struct ConfigReloadHandle {
    config: Arc<ArcSwap<DynamicConfig>>,
    static_config: StaticConfig,
    reload_mutex: Mutex<()>,
}

#[allow(dead_code)]
impl ConfigReloadHandle {
    pub fn new(config: Arc<ArcSwap<DynamicConfig>>, static_config: StaticConfig) -> Self {
        Self {
            config,
            static_config,
            reload_mutex: Mutex::new(()),
        }
    }

    pub fn load(&self) -> Arc<DynamicConfig> {
        self.config.load_full()
    }

    pub async fn reload(
        &self,
        new_static: StaticConfig,
        new_dynamic: DynamicConfig,
    ) -> anyhow::Result<Vec<String>> {
        let _guard = self.reload_mutex.lock().await;

        validate(&new_static, &new_dynamic, false).map_err(|errors| {
            anyhow::anyhow!(
                "{}",
                errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        })?;

        let changed_fields = diff_static_config(&self.static_config, &new_static);

        self.config.store(Arc::new(new_dynamic));

        Ok(changed_fields)
    }
}

fn diff_static_config(old: &StaticConfig, new: &StaticConfig) -> Vec<String> {
    let mut changes = Vec::new();

    if old.listeners != new.listeners {
        changes.push("listeners".to_string());
    }
    if old.allow_wildcard_bind != new.allow_wildcard_bind {
        changes.push("allow_wildcard_bind".to_string());
    }
    if old.health_check_port != new.health_check_port {
        changes.push("health_check_port".to_string());
    }
    if old.admin_socket_path != new.admin_socket_path {
        changes.push("admin_socket_path".to_string());
    }
    if old.shutdown_timeout_secs != new.shutdown_timeout_secs {
        changes.push("shutdown_timeout_secs".to_string());
    }
    if old.logging != new.logging {
        changes.push("logging".to_string());
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_fixtures;

    #[test]
    fn arcswap_swap_visible_after_reload() {
        let initial = test_fixtures::test_dynamic_config();
        let config_arc = Arc::new(ArcSwap::from_pointee(initial.clone()));
        let static_config = test_fixtures::test_static_config();
        let handle = ConfigReloadHandle::new(config_arc.clone(), static_config);

        let loaded = handle.load();
        assert_eq!(loaded.sites.len(), 1);
        assert_eq!(loaded.rate_limit.requests_per_second, 10);

        let mut new_dynamic = initial.clone();
        new_dynamic.rate_limit.requests_per_second = 50;
        new_dynamic.sites.push(SiteConfig {
            host: "new.test".to_string(),
            upstream: "127.0.0.1:9090".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        });

        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(handle.reload(test_fixtures::test_static_config(), new_dynamic))
            .unwrap();

        let loaded = handle.load();
        assert_eq!(loaded.sites.len(), 2);
        assert_eq!(loaded.rate_limit.requests_per_second, 50);
    }

    #[test]
    fn reload_rejects_invalid_config() {
        let initial = test_fixtures::test_dynamic_config();
        let config_arc = Arc::new(ArcSwap::from_pointee(initial.clone()));
        let static_config = test_fixtures::test_static_config();
        let handle = ConfigReloadHandle::new(config_arc.clone(), static_config);

        let mut invalid_dynamic = initial.clone();
        invalid_dynamic.rate_limit.requests_per_second = 0;

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result =
            rt.block_on(handle.reload(test_fixtures::test_static_config(), invalid_dynamic));
        assert!(result.is_err());

        let loaded = config_arc.load();
        assert_eq!(loaded.rate_limit.requests_per_second, 10);
    }

    #[tokio::test]
    async fn concurrent_reload_serialization() {
        let initial = test_fixtures::test_dynamic_config();
        let config_arc = Arc::new(ArcSwap::from_pointee(initial.clone()));
        let static_config = test_fixtures::test_static_config();
        let handle = Arc::new(ConfigReloadHandle::new(config_arc.clone(), static_config));

        let mut handles = Vec::new();
        for i in 1..=5u32 {
            let h = handle.clone();
            let initial = initial.clone();
            handles.push(tokio::spawn(async move {
                let mut dynamic = initial.clone();
                dynamic.rate_limit.requests_per_second = i * 10;
                h.reload(test_fixtures::test_static_config(), dynamic).await
            }));
        }

        for h in handles {
            h.await.unwrap().unwrap();
        }

        let loaded = config_arc.load();
        let rps = loaded.rate_limit.requests_per_second;
        assert!((rps == 10) || (rps == 20) || (rps == 30) || (rps == 40) || (rps == 50));
    }

    #[test]
    fn static_config_diff_detects_changes() {
        let old = test_fixtures::test_static_config();
        let mut new = old.clone();
        assert!(diff_static_config(&old, &new).is_empty());

        new.health_check_port = 8080;
        new.logging.level = "debug".to_string();
        let changes = diff_static_config(&old, &new);
        assert!(changes.contains(&"health_check_port".to_string()));
        assert!(changes.contains(&"logging".to_string()));
        assert_eq!(changes.len(), 2);
    }
}
