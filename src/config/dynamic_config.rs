use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use serde::Deserialize;
use tokio::sync::Mutex;

use super::static_config::StaticConfig;
use super::validation::validate;

#[derive(Debug, Clone)]
pub struct DynamicConfig {
    pub sites: Vec<SiteConfig>,
    pub routing_table: HashMap<String, SiteConfig>,
    pub rate_limit: RateLimitConfig,
    pub body: BodyConfig,
}

impl DynamicConfig {
    pub fn from_sites(
        sites: Vec<SiteConfig>,
        rate_limit: RateLimitConfig,
        body: BodyConfig,
    ) -> Self {
        let routing_table = build_routing_table(&sites);
        Self {
            sites,
            routing_table,
            rate_limit,
            body,
        }
    }

    pub fn lookup(&self, host: &str) -> Option<&SiteConfig> {
        self.routing_table.get(&normalize_host(host))
    }
}

impl PartialEq for DynamicConfig {
    fn eq(&self, other: &Self) -> bool {
        self.sites == other.sites && self.rate_limit == other.rate_limit && self.body == other.body
    }
}

pub fn build_routing_table(sites: &[SiteConfig]) -> HashMap<String, SiteConfig> {
    sites
        .iter()
        .map(|s| (s.host.to_lowercase(), s.clone()))
        .collect()
}

pub fn normalize_host(host: &str) -> String {
    let stripped = crate::utils::strip_port_from_host(host);
    let lower = stripped.to_lowercase();
    lower
        .strip_prefix('[')
        .unwrap_or(&lower)
        .strip_suffix(']')
        .unwrap_or(&lower)
        .to_string()
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct SerializableDynamicConfig {
    pub sites: Vec<SiteConfig>,
    pub rate_limit: RateLimitConfig,
    pub body: BodyConfig,
}

impl From<SerializableDynamicConfig> for DynamicConfig {
    fn from(value: SerializableDynamicConfig) -> Self {
        DynamicConfig::from_sites(value.sites, value.rate_limit, value.body)
    }
}

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

fn default_upstream_scheme() -> String {
    "http".to_string()
}

fn default_connect_timeout() -> u64 {
    5
}

fn default_request_timeout() -> u64 {
    60
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct RateLimitConfig {
    pub requests_per_second: u32,
    pub burst: u32,
}

#[derive(Debug, Deserialize, Clone, PartialEq)]
pub struct BodyConfig {
    pub limit_bytes: u64,
}

pub struct ConfigReloadHandle {
    config: Arc<ArcSwap<DynamicConfig>>,
    static_config: ArcSwap<StaticConfig>,
    reload_mutex: Mutex<()>,
    cli_allow_wildcard_bind: bool,
}

impl ConfigReloadHandle {
    pub fn new(
        config: Arc<ArcSwap<DynamicConfig>>,
        static_config: StaticConfig,
        cli_allow_wildcard_bind: bool,
    ) -> Self {
        Self {
            config,
            static_config: ArcSwap::from_pointee(static_config),
            reload_mutex: Mutex::new(()),
            cli_allow_wildcard_bind,
        }
    }

    pub fn load(&self) -> Arc<DynamicConfig> {
        self.config.load_full()
    }

    pub fn static_config(&self) -> Arc<StaticConfig> {
        self.static_config.load_full()
    }

    pub fn cli_allow_wildcard_bind(&self) -> bool {
        self.cli_allow_wildcard_bind
    }

    pub async fn reload(
        &self,
        new_static: StaticConfig,
        new_dynamic: DynamicConfig,
    ) -> anyhow::Result<Vec<String>> {
        let _guard = self.reload_mutex.lock().await;

        validate(&new_static, &new_dynamic, self.cli_allow_wildcard_bind).map_err(|errors| {
            anyhow::anyhow!(
                "{}",
                errors
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            )
        })?;

        let changed_fields = diff_static_config(&self.static_config.load(), &new_static);

        self.config.store(Arc::new(new_dynamic));
        self.static_config.store(Arc::new(new_static));

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
    if old.admin_key_path != new.admin_key_path {
        changes.push("admin_key_path".to_string());
    }
    if old.shutdown_timeout_secs != new.shutdown_timeout_secs {
        changes.push("shutdown_timeout_secs".to_string());
    }
    if old.connection_idle_timeout_secs != new.connection_idle_timeout_secs {
        changes.push("connection_idle_timeout_secs".to_string());
    }
    if old.max_connections != new.max_connections {
        changes.push("max_connections".to_string());
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
        let handle = ConfigReloadHandle::new(config_arc.clone(), static_config, false);

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
        let new_dynamic = DynamicConfig::from_sites(
            new_dynamic.sites,
            RateLimitConfig {
                requests_per_second: 50,
                burst: new_dynamic.rate_limit.burst,
            },
            new_dynamic.body,
        );

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
        let handle = ConfigReloadHandle::new(config_arc.clone(), static_config, false);

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
        let handle = Arc::new(ConfigReloadHandle::new(config_arc.clone(), static_config, false));

        let mut handles = Vec::new();
        for i in 1..=5u32 {
            let h = handle.clone();
            let initial = initial.clone();
            handles.push(tokio::spawn(async move {
                let mut dynamic = initial.clone();
                dynamic.rate_limit.requests_per_second = i * 10;
                let dynamic = DynamicConfig::from_sites(
                    dynamic.sites,
                    RateLimitConfig {
                        requests_per_second: i * 10,
                        burst: dynamic.rate_limit.burst,
                    },
                    dynamic.body,
                );
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

    #[test]
    fn reload_second_time_no_further_static_changes() {
        let initial = test_fixtures::test_dynamic_config();
        let config_arc = Arc::new(ArcSwap::from_pointee(initial.clone()));
        let original_static = test_fixtures::test_static_config();
        let handle = ConfigReloadHandle::new(config_arc.clone(), original_static.clone(), false);

        let mut changed_static = original_static.clone();
        changed_static.health_check_port = 8080;

        let rt = tokio::runtime::Runtime::new().unwrap();

        let changes1 = rt
            .block_on(handle.reload(changed_static.clone(), initial.clone()))
            .unwrap();
        assert!(changes1.contains(&"health_check_port".to_string()));

        let changes2 = rt
            .block_on(handle.reload(changed_static.clone(), initial.clone()))
            .unwrap();
        assert!(changes2.is_empty());
    }

    #[test]
    fn reload_second_time_different_static_changes() {
        let initial = test_fixtures::test_dynamic_config();
        let config_arc = Arc::new(ArcSwap::from_pointee(initial.clone()));
        let original_static = test_fixtures::test_static_config();
        let handle = ConfigReloadHandle::new(config_arc.clone(), original_static.clone(), false);

        let mut changed_static = original_static.clone();
        changed_static.health_check_port = 8080;

        let rt = tokio::runtime::Runtime::new().unwrap();

        let changes1 = rt
            .block_on(handle.reload(changed_static.clone(), initial.clone()))
            .unwrap();
        assert!(changes1.contains(&"health_check_port".to_string()));
        assert_eq!(changes1.len(), 1);

        let mut further_changed = changed_static.clone();
        further_changed.shutdown_timeout_secs = 60;

        let changes2 = rt
            .block_on(handle.reload(further_changed.clone(), initial.clone()))
            .unwrap();
        assert!(!changes2.contains(&"health_check_port".to_string()));
        assert!(changes2.contains(&"shutdown_timeout_secs".to_string()));
        assert_eq!(changes2.len(), 1);
    }

    #[test]
    fn normalize_host_converts_to_lowercase() {
        assert_eq!(normalize_host("Git.Alk.DEV"), "git.alk.dev");
    }

    #[test]
    fn normalize_host_strips_port() {
        assert_eq!(normalize_host("git.alk.dev:443"), "git.alk.dev");
        assert_eq!(normalize_host("GIT.ALK.DEV:8443"), "git.alk.dev");
    }

    #[test]
    fn normalize_host_no_port() {
        assert_eq!(normalize_host("git.alk.dev"), "git.alk.dev");
    }

    #[test]
    fn normalize_host_empty_string() {
        assert_eq!(normalize_host(""), "");
    }

    #[test]
    fn normalize_host_ipv6_with_port() {
        assert_eq!(normalize_host("[::1]:443"), "::1");
    }

    #[test]
    fn normalize_host_ipv6_long_with_port() {
        assert_eq!(normalize_host("[2001:db8::1]:8080"), "2001:db8::1");
    }

    #[test]
    fn normalize_host_ipv6_bare() {
        assert_eq!(normalize_host("[::1]"), "::1");
    }

    #[test]
    fn normalize_host_ipv6_uppercase() {
        assert_eq!(normalize_host("[2001:DB8::1]:443"), "2001:db8::1");
    }

    #[test]
    fn routing_table_lookup_finds_site() {
        let config = test_fixtures::test_dynamic_config();
        let site = config.lookup("test.local");
        assert!(site.is_some());
        assert_eq!(site.unwrap().host, "test.local");
    }

    #[test]
    fn routing_table_lookup_case_insensitive() {
        let config = test_fixtures::test_dynamic_config();
        let site = config.lookup("TEST.LOCAL");
        assert!(site.is_some());
        assert_eq!(site.unwrap().host, "test.local");
    }

    #[test]
    fn routing_table_lookup_strips_port() {
        let config = test_fixtures::test_dynamic_config();
        let site = config.lookup("test.local:443");
        assert!(site.is_some());
        assert_eq!(site.unwrap().host, "test.local");
    }

    #[test]
    fn routing_table_lookup_unknown_host() {
        let config = test_fixtures::test_dynamic_config();
        let site = config.lookup("unknown.example");
        assert!(site.is_none());
    }

    #[test]
    fn build_routing_table_multiple_sites() {
        let sites = vec![
            SiteConfig {
                host: "git.example.com".to_string(),
                upstream: "127.0.0.1:3000".to_string(),
                upstream_scheme: "http".to_string(),
                upstream_connect_timeout_secs: 5,
                upstream_request_timeout_secs: 60,
            },
            SiteConfig {
                host: "www.example.com".to_string(),
                upstream: "127.0.0.1:8080".to_string(),
                upstream_scheme: "http".to_string(),
                upstream_connect_timeout_secs: 5,
                upstream_request_timeout_secs: 60,
            },
        ];
        let table = build_routing_table(&sites);
        assert_eq!(table.len(), 2);
        assert!(table.contains_key("git.example.com"));
        assert!(table.contains_key("www.example.com"));
    }

    #[test]
    fn dynamic_config_from_sites_builds_routing_table() {
        let sites = vec![SiteConfig {
            host: "My.Site".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }];
        let config = DynamicConfig::from_sites(
            sites,
            RateLimitConfig {
                requests_per_second: 10,
                burst: 20,
            },
            BodyConfig {
                limit_bytes: 104857600,
            },
        );
        assert!(config.routing_table.contains_key("my.site"));
        assert_eq!(config.routing_table.len(), 1);
    }

    #[test]
    fn reload_accepts_wildcard_bind_when_flag_true() {
        let initial = test_fixtures::test_dynamic_config();
        let config_arc = Arc::new(ArcSwap::from_pointee(initial.clone()));
        let mut static_config = test_fixtures::test_static_config();
        static_config.listeners[0].bind_addr = "0.0.0.0".to_string();
        static_config.allow_wildcard_bind = false;
        let handle = ConfigReloadHandle::new(config_arc.clone(), static_config.clone(), true);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(handle.reload(static_config, initial));
        assert!(result.is_ok());
    }

    #[test]
    fn reload_rejects_wildcard_bind_when_flag_false() {
        let initial = test_fixtures::test_dynamic_config();
        let config_arc = Arc::new(ArcSwap::from_pointee(initial.clone()));
        let mut static_config = test_fixtures::test_static_config();
        static_config.listeners[0].bind_addr = "0.0.0.0".to_string();
        static_config.allow_wildcard_bind = false;
        let handle = ConfigReloadHandle::new(config_arc.clone(), static_config.clone(), false);

        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(handle.reload(static_config, initial));
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("0.0.0.0"));
    }
}
