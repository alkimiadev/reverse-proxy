use crate::config::dynamic_config::{BodyConfig, DynamicConfig, RateLimitConfig, SiteConfig};
use crate::config::static_config::{ListenerConfig, LoggingConfig, StaticConfig, TlsConfig};

pub fn test_static_config() -> StaticConfig {
    StaticConfig {
        listeners: vec![ListenerConfig {
            bind_addr: "127.0.0.1".to_string(),
            http_port: 80,
            https_port: 443,
            tls: TlsConfig {
                mode: "acme".to_string(),
                acme_domains: vec!["test.local".to_string()],
                acme_cache_dir: "/tmp/acme-cache".to_string(),
                acme_directory: "staging".to_string(),
                cert_path: String::new(),
                key_path: String::new(),
            },
            sites: vec![],
        }],
        allow_wildcard_bind: false,
        health_check_port: 9900,
        admin_socket_path: "/tmp/reverse-proxy-test/admin.sock".to_string(),
        shutdown_timeout_secs: 30,
        logging: LoggingConfig::default(),
    }
}

pub fn test_dynamic_config() -> DynamicConfig {
    DynamicConfig {
        sites: vec![SiteConfig {
            host: "test.local".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }],
        rate_limit: RateLimitConfig {
            requests_per_second: 10,
            burst: 20,
        },
        body: BodyConfig {
            limit_bytes: 104857600,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_config_fixture_is_valid() {
        let config = test_static_config();
        assert!(!config.listeners.is_empty());
        assert_eq!(config.health_check_port, 9900);
    }

    #[test]
    fn test_dynamic_config_fixture_is_valid() {
        let config = test_dynamic_config();
        assert!(!config.sites.is_empty());
        assert_eq!(config.rate_limit.requests_per_second, 10);
        assert_eq!(config.body.limit_bytes, 104857600);
    }
}
