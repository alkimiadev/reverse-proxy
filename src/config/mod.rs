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
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ReloadError {
    #[error("IO error reading config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("Failed to parse config file: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("Config validation failed: {0}")]
    Validation(String),
    #[error("config file changed during read, please retry")]
    FileChangedDuringRead,
}

impl From<Vec<ValidationError>> for ReloadError {
    fn from(errors: Vec<ValidationError>) -> Self {
        ReloadError::Validation(
            errors
                .iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("; "),
        )
    }
}

#[derive(Debug, Deserialize)]
pub struct FullConfig {
    #[serde(default)]
    pub listeners: Vec<ListenerConfig>,
    #[serde(default)]
    pub allow_wildcard_bind: bool,
    #[serde(default = "static_config::default_health_check_port")]
    pub health_check_port: u16,
    #[serde(default = "static_config::default_admin_key_path")]
    pub admin_key_path: String,
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
            admin_key_path: self.admin_key_path,
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

pub async fn read_and_validate_config(
    config_path: &str,
    cli_allow_wildcard_bind: bool,
) -> Result<(StaticConfig, DynamicConfig), ReloadError> {
    let metadata_before = tokio::fs::metadata(config_path).await?;
    let config_content = tokio::fs::read_to_string(config_path).await?;
    let metadata_after = tokio::fs::metadata(config_path).await?;

    if metadata_before.modified().ok() != metadata_after.modified().ok() {
        tracing::warn!(
            event = "CONFIG_RELOAD",
            status = "rejected",
            reason = "file_mtime_changed",
            path = config_path
        );
        return Err(ReloadError::FileChangedDuringRead);
    }

    let full_config: FullConfig = toml::from_str(&config_content)?;
    let (new_static, new_dynamic) = full_config.into_static_and_dynamic();

    validate(&new_static, &new_dynamic, cli_allow_wildcard_bind)?;

    Ok((new_static, new_dynamic))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn valid_config_toml() -> &'static str {
        r#"
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
"#
    }

    #[tokio::test]
    async fn read_and_validate_config_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, valid_config_toml()).unwrap();

        let result = read_and_validate_config(config_path.to_str().unwrap(), false).await;
        assert!(result.is_ok());
        let (static_config, dynamic_config) = result.unwrap();
        assert_eq!(dynamic_config.rate_limit.requests_per_second, 20);
        assert_eq!(static_config.health_check_port, 9900);
    }

    #[tokio::test]
    async fn read_and_validate_config_missing_file() {
        let result = read_and_validate_config("/nonexistent/config.toml", false).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ReloadError::Io(_) => {}
            e => panic!("expected Io error, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn read_and_validate_config_invalid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, "invalid toml {{{").unwrap();

        let result = read_and_validate_config(config_path.to_str().unwrap(), false).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ReloadError::Parse(_) => {}
            e => panic!("expected Parse error, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn read_and_validate_config_validation_fails() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let config_content = r#"
health_check_port = 9900

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 0
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
acme_contact = "mailto:admin@test.local"
"#;
        std::fs::write(&config_path, config_content).unwrap();

        let result = read_and_validate_config(config_path.to_str().unwrap(), false).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ReloadError::Validation(msg) => {
                assert!(msg.contains("requests_per_second"));
            }
            e => panic!("expected Validation error, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn read_and_validate_config_wildcard_bind_allowed_with_flag() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let config_content = r#"
health_check_port = 9900

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "0.0.0.0"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme-cache"
acme_contact = "mailto:admin@test.local"

[[listeners.sites]]
host = "test.local"
upstream = "127.0.0.1:8080"
"#;
        std::fs::write(&config_path, config_content).unwrap();

        let result = read_and_validate_config(config_path.to_str().unwrap(), true).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn read_and_validate_config_wildcard_bind_rejected_without_flag() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        let config_content = r#"
health_check_port = 9900

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "0.0.0.0"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme-cache"
acme_contact = "mailto:admin@test.local"

[[listeners.sites]]
host = "test.local"
upstream = "127.0.0.1:8080"
"#;
        std::fs::write(&config_path, config_content).unwrap();

        let result = read_and_validate_config(config_path.to_str().unwrap(), false).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ReloadError::Validation(msg) => {
                assert!(msg.contains("0.0.0.0"));
            }
            e => panic!("expected Validation error, got: {:?}", e),
        }
    }

    #[tokio::test]
    async fn read_and_validate_config_mtime_change_detected() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, valid_config_toml()).unwrap();

        let path = config_path.clone();
        let handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .truncate(true)
                .open(&path)
                .unwrap();
            file.write_all(valid_config_toml().as_bytes()).unwrap();
            file.sync_all().unwrap();
        });

        let result = read_and_validate_config(config_path.to_str().unwrap(), false).await;

        handle.await.unwrap();

        match result {
            Err(ReloadError::FileChangedDuringRead) => {}
            Ok(_) => {}
            Err(e) => panic!("expected FileChangedDuringRead or Ok, got: {:?}", e),
        }
    }
}
