pub mod dynamic_config;
pub mod static_config;
pub mod test_fixtures;
pub mod validation;

pub use dynamic_config::{
    build_routing_table, normalize_host, BodyConfig, ConfigReloadHandle, DynamicConfig,
    RateLimitConfig, SerializableDynamicConfig, SiteConfig,
};
pub use static_config::{ListenerConfig, LoggingConfig, StaticConfig, TlsConfig};
pub use validation::{validate, ValidationError};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct FullConfig {
    #[serde(default)]
    pub listeners: Vec<ListenerConfig>,
    #[serde(default)]
    pub allow_wildcard_bind: bool,
    #[serde(default = "static_config::default_health_check_port")]
    pub health_check_port: u16,
    #[serde(default = "static_config::default_admin_socket_path")]
    pub admin_socket_path: String,
    #[serde(default = "static_config::default_shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,
    #[serde(default)]
    pub logging: LoggingConfig,
    pub rate_limit: RateLimitConfig,
    pub body: BodyConfig,
}

impl FullConfig {
    pub fn parse(content: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(content)?)
    }

    pub fn into_static_and_dynamic(self) -> (StaticConfig, DynamicConfig) {
        let static_config = StaticConfig {
            listeners: self.listeners,
            allow_wildcard_bind: self.allow_wildcard_bind,
            health_check_port: self.health_check_port,
            admin_socket_path: self.admin_socket_path,
            shutdown_timeout_secs: self.shutdown_timeout_secs,
            logging: self.logging,
        };
        let dynamic_config = DynamicConfig::from_sites(
            static_config
                .listeners
                .iter()
                .flat_map(|l| l.sites.clone())
                .collect(),
            self.rate_limit,
            self.body,
        );
        (static_config, dynamic_config)
    }
}
