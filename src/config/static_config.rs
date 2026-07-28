use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct StaticConfig {
    pub listeners: Vec<ListenerConfig>,
    #[serde(default)]
    pub allow_wildcard_bind: bool,
    #[serde(default = "default_health_check_port")]
    pub health_check_port: u16,
    #[serde(default = "default_admin_key_path")]
    pub admin_key_path: String,
    #[serde(default = "default_shutdown_timeout_secs")]
    pub shutdown_timeout_secs: u64,
    #[serde(default = "default_connection_idle_timeout_secs")]
    pub connection_idle_timeout_secs: u64,
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
    #[serde(default)]
    pub logging: LoggingConfig,
}

pub fn default_health_check_port() -> u16 {
    9900
}

pub fn default_admin_key_path() -> String {
    "/etc/reverse-proxy/admin-key".to_string()
}

pub fn default_shutdown_timeout_secs() -> u64 {
    30
}

pub fn default_connection_idle_timeout_secs() -> u64 {
    60
}

pub fn default_max_connections() -> usize {
    1024
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
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

fn default_http_port() -> u16 {
    80
}

fn default_https_port() -> u16 {
    443
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct TlsConfig {
    pub mode: String,
    #[serde(default)]
    pub acme_domains: Vec<String>,
    #[serde(default)]
    pub acme_cache_dir: String,
    #[serde(default = "default_acme_directory")]
    pub acme_directory: String,
    #[serde(default)]
    pub acme_contact: String,
    #[serde(default)]
    pub cert_path: String,
    #[serde(default)]
    pub key_path: String,
}

fn default_acme_directory() -> String {
    "production".to_string()
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct LoggingConfig {
    #[serde(default = "default_log_level")]
    pub level: String,
    #[serde(default = "default_log_format")]
    pub format: String,
    #[serde(default)]
    pub log_file_path: Option<String>,
}

fn default_log_level() -> String {
    "info".to_string()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    fn multi_config_toml() -> &'static str {
        r#"
health_check_port = 9900
admin_key_path = "/etc/reverse-proxy/admin-key"

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "203.0.113.10"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["git.alk.dev"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache-git"
acme_directory = "production"

[[listeners.sites]]
host = "git.alk.dev"
upstream = "127.0.0.1:3000"
upstream_scheme = "http"

[[listeners]]
bind_addr = "203.0.113.11"
http_port = 80
https_port = 443

[listeners.tls]
mode = "manual"
cert_path = "/etc/ssl/alk.dev/fullchain.pem"
key_path = "/etc/ssl/alk.dev/privkey.pem"

[[listeners.sites]]
host = "alk.dev"
upstream = "127.0.0.1:8080"
upstream_scheme = "http"
"#
    }

    fn shared_ip_san_toml() -> &'static str {
        r#"
health_check_port = 9900
admin_key_path = "/etc/reverse-proxy/admin-key"

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "203.0.113.10"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["git.alk.dev", "alk.dev"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"

[[listeners.sites]]
host = "git.alk.dev"
upstream = "127.0.0.1:3000"

[[listeners.sites]]
host = "alk.dev"
upstream = "127.0.0.1:8080"
"#
    }

    #[allow(dead_code)]
    #[derive(Debug, serde::Deserialize)]
    struct FullConfig {
        #[serde(default)]
        listeners: Vec<ListenerConfig>,
        #[serde(default)]
        allow_wildcard_bind: bool,
        #[serde(default = "default_health_check_port")]
        health_check_port: u16,
        #[serde(default = "default_admin_key_path")]
        admin_key_path: String,
        #[serde(default = "default_shutdown_timeout_secs")]
        shutdown_timeout_secs: u64,
        #[serde(default = "default_connection_idle_timeout_secs")]
        connection_idle_timeout_secs: u64,
        #[serde(default = "default_max_connections")]
        max_connections: usize,
        #[serde(default)]
        logging: LoggingConfig,
        rate_limit: crate::config::dynamic_config::RateLimitConfig,
        body: crate::config::dynamic_config::BodyConfig,
    }

    #[test]
    fn deserialize_multi_config() {
        let config: FullConfig =
            toml::from_str(multi_config_toml()).expect("multi-config TOML should parse");

        assert_eq!(config.listeners.len(), 2);
        assert!(!config.allow_wildcard_bind);
        assert_eq!(config.health_check_port, 9900);
        assert_eq!(config.admin_key_path, "/etc/reverse-proxy/admin-key");
        assert_eq!(config.shutdown_timeout_secs, 30);
        assert_eq!(config.connection_idle_timeout_secs, 60);
        assert_eq!(config.max_connections, 1024);
        assert_eq!(config.logging.level, "info");
        assert_eq!(config.logging.format, "text");
        assert!(config.logging.log_file_path.is_none());

        let listener1 = &config.listeners[0];
        assert_eq!(listener1.bind_addr, "203.0.113.10");
        assert_eq!(listener1.http_port, 80);
        assert_eq!(listener1.https_port, 443);
        assert_eq!(listener1.tls.mode, "acme");
        assert_eq!(listener1.tls.acme_domains, vec!["git.alk.dev"]);
        assert_eq!(
            listener1.tls.acme_cache_dir,
            "/var/lib/reverse-proxy/acme-cache-git"
        );
        assert_eq!(listener1.tls.acme_directory, "production");
        assert_eq!(listener1.sites.len(), 1);
        assert_eq!(listener1.sites[0].host, "git.alk.dev");
        assert_eq!(listener1.sites[0].upstream, "127.0.0.1:3000");
        assert_eq!(listener1.sites[0].upstream_scheme, "http");

        let listener2 = &config.listeners[1];
        assert_eq!(listener2.bind_addr, "203.0.113.11");
        assert_eq!(listener2.tls.mode, "manual");
        assert_eq!(listener2.tls.cert_path, "/etc/ssl/alk.dev/fullchain.pem");
        assert_eq!(listener2.tls.key_path, "/etc/ssl/alk.dev/privkey.pem");
        assert_eq!(listener2.sites.len(), 1);
        assert_eq!(listener2.sites[0].host, "alk.dev");
    }

    #[test]
    fn deserialize_shared_ip_san() {
        let config: FullConfig =
            toml::from_str(shared_ip_san_toml()).expect("shared-IP SAN TOML should parse");

        assert_eq!(config.listeners.len(), 1);
        let listener = &config.listeners[0];
        assert_eq!(listener.bind_addr, "203.0.113.10");
        assert_eq!(listener.tls.mode, "acme");
        assert_eq!(listener.tls.acme_domains, vec!["git.alk.dev", "alk.dev"]);
        assert_eq!(
            listener.tls.acme_cache_dir,
            "/var/lib/reverse-proxy/acme-cache"
        );
        assert_eq!(listener.sites.len(), 2);
        assert_eq!(listener.sites[0].host, "git.alk.dev");
        assert_eq!(listener.sites[1].host, "alk.dev");
    }

    #[test]
    fn defaults_applied_when_omitted() {
        let minimal = r#"
[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "192.168.1.1"

[listeners.tls]
mode = "acme"
acme_domains = ["example.com"]
acme_cache_dir = "/tmp/cache"
"#;

        #[allow(dead_code)]
        #[derive(Debug, serde::Deserialize)]
        struct MinimalConfig {
            #[serde(default)]
            listeners: Vec<ListenerConfig>,
            #[serde(default)]
            allow_wildcard_bind: bool,
            #[serde(default = "default_health_check_port")]
            health_check_port: u16,
            #[serde(default = "default_admin_key_path")]
            admin_key_path: String,
            #[serde(default = "default_shutdown_timeout_secs")]
            shutdown_timeout_secs: u64,
            #[serde(default = "default_connection_idle_timeout_secs")]
            connection_idle_timeout_secs: u64,
            #[serde(default = "default_max_connections")]
            max_connections: usize,
            #[serde(default)]
            logging: LoggingConfig,
            rate_limit: crate::config::dynamic_config::RateLimitConfig,
            body: crate::config::dynamic_config::BodyConfig,
        }

        let config: MinimalConfig = toml::from_str(minimal).expect("minimal config should parse");

        assert!(!config.allow_wildcard_bind);
        assert_eq!(config.health_check_port, 9900);
        assert_eq!(config.admin_key_path, "/etc/reverse-proxy/admin-key");
        assert_eq!(config.shutdown_timeout_secs, 30);
        assert_eq!(config.connection_idle_timeout_secs, 60);
        assert_eq!(config.max_connections, 1024);
        assert_eq!(config.logging.level, "info");
        assert_eq!(config.logging.format, "text");
        assert!(config.logging.log_file_path.is_none());

        let listener = &config.listeners[0];
        assert_eq!(listener.http_port, 80);
        assert_eq!(listener.https_port, 443);
        assert_eq!(listener.tls.acme_directory, "production");
        assert!(listener.sites.is_empty());
    }

    #[test]
    fn logging_with_file_path() {
        let toml = r#"
[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[logging]
level = "debug"
format = "json"
log_file_path = "/var/log/reverse-proxy/access.log"

[[listeners]]
bind_addr = "203.0.113.10"

[listeners.tls]
mode = "manual"
cert_path = "/etc/ssl/cert.pem"
key_path = "/etc/ssl/key.pem"
"#;

        #[allow(dead_code)]
        #[derive(Debug, serde::Deserialize)]
        struct TestConfig {
            #[serde(default)]
            listeners: Vec<ListenerConfig>,
            #[serde(default)]
            logging: LoggingConfig,
            rate_limit: crate::config::dynamic_config::RateLimitConfig,
            body: crate::config::dynamic_config::BodyConfig,
        }

        let config: TestConfig =
            toml::from_str(toml).expect("config with log_file_path should parse");

        assert_eq!(config.logging.level, "debug");
        assert_eq!(config.logging.format, "json");
        assert_eq!(
            config.logging.log_file_path,
            Some("/var/log/reverse-proxy/access.log".to_string())
        );
    }

    #[test]
    fn site_defaults() {
        let toml = r#"
[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "203.0.113.10"

[listeners.tls]
mode = "acme"
acme_domains = ["test.example"]
acme_cache_dir = "/tmp/acme"

[[listeners.sites]]
host = "test.example"
upstream = "127.0.0.1:8080"
"#;

        #[allow(dead_code)]
        #[derive(Debug, serde::Deserialize)]
        struct TestConfig {
            #[serde(default)]
            listeners: Vec<ListenerConfig>,
            rate_limit: crate::config::dynamic_config::RateLimitConfig,
            body: crate::config::dynamic_config::BodyConfig,
        }

        let config: TestConfig = toml::from_str(toml).expect("config should parse");

        let site = &config.listeners[0].sites[0];
        assert_eq!(site.host, "test.example");
        assert_eq!(site.upstream, "127.0.0.1:8080");
        assert_eq!(site.upstream_scheme, "http");
        assert_eq!(site.upstream_connect_timeout_secs, 5);
        assert_eq!(site.upstream_request_timeout_secs, 60);
    }
}
