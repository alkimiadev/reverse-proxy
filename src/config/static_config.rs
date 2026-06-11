use serde::Deserialize;

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct StaticConfig {
    pub listeners: Vec<ListenerConfig>,
    #[serde(default)]
    pub allow_wildcard_bind: bool,
    #[serde(default = "default_health_check_port")]
    pub health_check_port: u16,
    #[serde(default = "default_admin_socket_path")]
    pub admin_socket_path: String,
    #[serde(default = "default_shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,
    #[serde(default)]
    pub logging: LoggingConfig,
}

#[allow(dead_code)]
fn default_health_check_port() -> u16 {
    9900
}

#[allow(dead_code)]
fn default_admin_socket_path() -> String {
    "/run/reverse-proxy/admin.sock".to_string()
}

#[allow(dead_code)]
fn default_shutdown_timeout_secs() -> u64 {
    30
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct ListenerConfig {
    pub bind_addr: String,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    #[serde(default = "default_https_port")]
    pub https_port: u16,
    pub tls: TlsConfig,
    #[serde(default)]
    pub sites: Vec<crate::config::dynamic_config::SiteConfig>,
}

#[allow(dead_code)]
fn default_http_port() -> u16 {
    80
}

#[allow(dead_code)]
fn default_https_port() -> u16 {
    443
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct TlsConfig {
    pub mode: String,
    #[serde(default)]
    pub acme_domains: Vec<String>,
    #[serde(default)]
    pub acme_cache_dir: String,
    #[serde(default = "default_acme_directory")]
    pub acme_directory: String,
    #[serde(default)]
    pub cert_path: String,
    #[serde(default)]
    pub key_path: String,
}

#[allow(dead_code)]
fn default_acme_directory() -> String {
    "production".to_string()
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String,
    #[serde(default)]
    pub log_file_path: Option<String>,
}

#[allow(dead_code)]
fn default_log_level() -> String {
    "info".to_string()
}

#[allow(dead_code)]
fn default_log_format() -> String {
    "text".to_string()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            format: default_log_format(),
            log_file_path: None,
        }
    }
}
