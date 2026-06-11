use std::collections::HashSet;
use std::path::Path;

use thiserror::Error;

use super::dynamic_config::DynamicConfig;
use super::static_config::StaticConfig;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("at least one listener must be defined")]
    NoListeners,
    #[error("listener {bind_addr}: bind_addr is 0.0.0.0 but allow_wildcard_bind is not enabled (config or CLI flag required)")]
    WildcardBindNotAllowed { bind_addr: String },
    #[error("duplicate bind_addr:https_port combination: {bind_addr}:{https_port}")]
    DuplicateHttpsBind { bind_addr: String, https_port: u16 },
    #[error("listener {bind_addr}: ACME mode requires acme_domains to be non-empty")]
    AcmeDomainsEmpty { bind_addr: String },
    #[error("listener {bind_addr}: manual mode requires both cert_path and key_path")]
    ManualCertMissing { bind_addr: String },
    #[error("listener {bind_addr}: cert_path '{path}' is not readable: {reason}")]
    CertPathNotReadable {
        bind_addr: String,
        path: String,
        reason: String,
    },
    #[error("listener {bind_addr}: key_path '{path}' is not readable: {reason}")]
    KeyPathNotReadable {
        bind_addr: String,
        path: String,
        reason: String,
    },
    #[error("site '{host}': host must be set")]
    SiteHostEmpty { host: String },
    #[error("site '{host}': upstream must be set")]
    SiteUpstreamEmpty { host: String },
    #[error("duplicate site host '{host}' across listeners")]
    DuplicateSiteHost { host: String },
    #[error("rate_limit.requests_per_second must be > 0, got {value}")]
    RequestsPerSecondZero { value: u32 },
    #[error("body.limit_bytes must be > 0, got {value}")]
    BodyLimitBytesZero { value: u64 },
    #[error("duplicate bind_addr:http_port combination: {bind_addr}:{http_port}")]
    DuplicateHttpBind { bind_addr: String, http_port: u16 },
    #[error(
        "listener {bind_addr}: http_port ({http_port}) and https_port ({https_port}) must differ"
    )]
    HttpsAndHttpPortSame {
        bind_addr: String,
        http_port: u16,
        https_port: u16,
    },
    #[error("listener {bind_addr}: https_port must be 1-65535, got {https_port}")]
    HttpsPortInvalid { bind_addr: String, https_port: u16 },
    #[error("listener {bind_addr}: http_port must be 0 (disabled) or 1-65535, got {http_port}")]
    HttpPortInvalid { bind_addr: String, http_port: u16 },
    #[error("health_check_port {health_check_port} conflicts with listener {bind_addr}:{port}")]
    HealthCheckPortConflict {
        health_check_port: u16,
        bind_addr: String,
        port: u16,
    },
    #[error("site '{host}': host must not include a port number")]
    SiteHostContainsPort { host: String },
    #[error("site '{host}': invalid hostname (must be a valid DNS name, not an IP address)")]
    SiteHostInvalid { host: String },
    #[error(
        "site '{host}': upstream must be in host:port format with port 1-65535, got '{upstream}'"
    )]
    UpstreamInvalid { host: String, upstream: String },
    #[error("site '{host}': upstream_scheme must be 'http' or 'https', got '{scheme}'")]
    UpstreamSchemeInvalid { host: String, scheme: String },
}

pub fn validate(
    static_config: &StaticConfig,
    dynamic_config: &DynamicConfig,
    cli_allow_wildcard_bind: bool,
) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    let allow_wildcard = static_config.allow_wildcard_bind || cli_allow_wildcard_bind;

    // Rule 1: At least one listener
    if static_config.listeners.is_empty() {
        errors.push(ValidationError::NoListeners);
    }

    let mut https_bind_keys = HashSet::new();
    let mut http_bind_keys = HashSet::new();

    for listener in &static_config.listeners {
        // Rule 2: Wildcard bind address
        if listener.bind_addr == "0.0.0.0" && !allow_wildcard {
            errors.push(ValidationError::WildcardBindNotAllowed {
                bind_addr: listener.bind_addr.clone(),
            });
        }

        // Rule 3: Unique bind_addr:https_port
        let https_key = (listener.bind_addr.as_str(), listener.https_port);
        if !https_bind_keys.insert(https_key) {
            errors.push(ValidationError::DuplicateHttpsBind {
                bind_addr: listener.bind_addr.clone(),
                https_port: listener.https_port,
            });
        }

        // Rule 10: Unique bind_addr:http_port (if http_port > 0)
        if listener.http_port > 0 {
            let http_key = (listener.bind_addr.as_str(), listener.http_port);
            if !http_bind_keys.insert(http_key) {
                errors.push(ValidationError::DuplicateHttpBind {
                    bind_addr: listener.bind_addr.clone(),
                    http_port: listener.http_port,
                });
            }
        }

        // Rule 12: https_port must be 1-65535
        if listener.https_port == 0 {
            errors.push(ValidationError::HttpsPortInvalid {
                bind_addr: listener.bind_addr.clone(),
                https_port: listener.https_port,
            });
        }

        // Rule 11: http_port and https_port must differ
        if listener.http_port > 0 && listener.http_port == listener.https_port {
            errors.push(ValidationError::HttpsAndHttpPortSame {
                bind_addr: listener.bind_addr.clone(),
                http_port: listener.http_port,
                https_port: listener.https_port,
            });
        }

        // Rule 4 & 5: TLS mode validation
        match listener.tls.mode.as_str() {
            "acme" => {
                // Rule 4: ACME domains must be non-empty
                if listener.tls.acme_domains.is_empty() {
                    errors.push(ValidationError::AcmeDomainsEmpty {
                        bind_addr: listener.bind_addr.clone(),
                    });
                }
            }
            "manual" => {
                let cert_empty = listener.tls.cert_path.is_empty();
                let key_empty = listener.tls.key_path.is_empty();
                if cert_empty || key_empty {
                    // Rule 5: Both paths must be set
                    errors.push(ValidationError::ManualCertMissing {
                        bind_addr: listener.bind_addr.clone(),
                    });
                } else {
                    // Rule 5: Files must be readable
                    let cert_path = Path::new(&listener.tls.cert_path);
                    if !cert_path.exists() {
                        errors.push(ValidationError::CertPathNotReadable {
                            bind_addr: listener.bind_addr.clone(),
                            path: listener.tls.cert_path.clone(),
                            reason: "file does not exist".to_string(),
                        });
                    }
                    let key_path = Path::new(&listener.tls.key_path);
                    if !key_path.exists() {
                        errors.push(ValidationError::KeyPathNotReadable {
                            bind_addr: listener.bind_addr.clone(),
                            path: listener.tls.key_path.clone(),
                            reason: "file does not exist".to_string(),
                        });
                    }
                }
            }
            _ => {}
        }
    }

    // Rule 14: health_check_port conflicts
    if static_config.health_check_port > 0 {
        for listener in &static_config.listeners {
            if static_config.health_check_port == listener.https_port {
                errors.push(ValidationError::HealthCheckPortConflict {
                    health_check_port: static_config.health_check_port,
                    bind_addr: listener.bind_addr.clone(),
                    port: listener.https_port,
                });
            }
            if listener.http_port > 0 && static_config.health_check_port == listener.http_port {
                errors.push(ValidationError::HealthCheckPortConflict {
                    health_check_port: static_config.health_check_port,
                    bind_addr: listener.bind_addr.clone(),
                    port: listener.http_port,
                });
            }
        }
    }

    // Site validation
    let mut site_hosts: HashSet<String> = HashSet::new();

    for listener in &static_config.listeners {
        for site in &listener.sites {
            // Rule 6: host must be set
            if site.host.is_empty() {
                errors.push(ValidationError::SiteHostEmpty {
                    host: String::new(),
                });
            }

            // Rule 6: upstream must be set
            if site.upstream.is_empty() {
                errors.push(ValidationError::SiteUpstreamEmpty {
                    host: site.host.clone(),
                });
            }

            // Rule 16: Normalize hostname and check validity
            let normalized_host = site.host.to_lowercase();

            // Rule 7: Unique hosts (case-insensitive)
            if !site_hosts.insert(normalized_host.clone()) {
                errors.push(ValidationError::DuplicateSiteHost {
                    host: normalized_host,
                });
            }

            // Rule 15: Host must not contain port
            if site.host.contains(':') {
                errors.push(ValidationError::SiteHostContainsPort {
                    host: site.host.clone(),
                });
            }

            // Rule 16: Host must be a valid hostname
            if !is_valid_hostname(&site.host) {
                errors.push(ValidationError::SiteHostInvalid {
                    host: site.host.clone(),
                });
            }

            // Rule 17: Upstream must be host:port format
            if !site.upstream.is_empty() && !is_valid_upstream(&site.upstream) {
                errors.push(ValidationError::UpstreamInvalid {
                    host: site.host.clone(),
                    upstream: site.upstream.clone(),
                });
            }

            // Rule 18: upstream_scheme must be "http" or "https"
            if site.upstream_scheme != "http" && site.upstream_scheme != "https" {
                errors.push(ValidationError::UpstreamSchemeInvalid {
                    host: site.host.clone(),
                    scheme: site.upstream_scheme.clone(),
                });
            }
        }
    }

    // Rule 8: requests_per_second > 0
    if dynamic_config.rate_limit.requests_per_second == 0 {
        errors.push(ValidationError::RequestsPerSecondZero { value: 0 });
    }

    // Rule 9: body limit_bytes > 0
    if dynamic_config.body.limit_bytes == 0 {
        errors.push(ValidationError::BodyLimitBytesZero { value: 0 });
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn is_valid_hostname(host: &str) -> bool {
    if host.is_empty() {
        return false;
    }
    if host.contains(':') {
        return false;
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    if host.starts_with('-') || host.ends_with('-') {
        return false;
    }
    if host.contains('.') {
        for label in host.split('.') {
            if label.is_empty() {
                return false;
            }
            if label.starts_with('-') || label.ends_with('-') {
                return false;
            }
            if !label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                return false;
            }
        }
        true
    } else {
        host.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    }
}

fn is_valid_upstream(upstream: &str) -> bool {
    if let Some(idx) = upstream.rfind(':') {
        let host_part = &upstream[..idx];
        let port_str = &upstream[idx + 1..];
        if host_part.is_empty() {
            return false;
        }
        if upstream.starts_with("http://") || upstream.starts_with("https://") {
            return false;
        }
        let port: u16 = match port_str.parse() {
            Ok(p) => p,
            Err(_) => return false,
        };
        port != 0
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dynamic_config::{BodyConfig, DynamicConfig, RateLimitConfig, SiteConfig};
    use crate::config::static_config::{ListenerConfig, LoggingConfig, StaticConfig, TlsConfig};
    use std::fs;

    fn valid_static_config() -> StaticConfig {
        StaticConfig {
            listeners: vec![ListenerConfig {
                bind_addr: "127.0.0.1".to_string(),
                http_port: 80,
                https_port: 443,
                tls: TlsConfig {
                    mode: "manual".to_string(),
                    acme_domains: vec![],
                    acme_cache_dir: String::new(),
                    acme_directory: "production".to_string(),
                    cert_path: String::new(),
                    key_path: String::new(),
                },
                sites: vec![],
            }],
            allow_wildcard_bind: false,
            health_check_port: 9900,
            admin_socket_path: "/run/reverse-proxy/admin.sock".to_string(),
            shutdown_timeout_secs: 30,
            logging: LoggingConfig::default(),
        }
    }

    fn valid_dynamic_config() -> DynamicConfig {
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

    fn make_static_with_sites(sites: Vec<SiteConfig>, tls: TlsConfig) -> StaticConfig {
        StaticConfig {
            listeners: vec![ListenerConfig {
                bind_addr: "127.0.0.1".to_string(),
                http_port: 80,
                https_port: 443,
                tls,
                sites,
            }],
            allow_wildcard_bind: false,
            health_check_port: 9900,
            admin_socket_path: "/run/reverse-proxy/admin.sock".to_string(),
            shutdown_timeout_secs: 30,
            logging: LoggingConfig::default(),
        }
    }

    fn make_acme_tls() -> TlsConfig {
        TlsConfig {
            mode: "acme".to_string(),
            acme_domains: vec!["test.local".to_string()],
            acme_cache_dir: "/tmp/acme-cache".to_string(),
            acme_directory: "production".to_string(),
            cert_path: String::new(),
            key_path: String::new(),
        }
    }

    fn make_manual_tls(cert: &str, key: &str) -> TlsConfig {
        TlsConfig {
            mode: "manual".to_string(),
            acme_domains: vec![],
            acme_cache_dir: String::new(),
            acme_directory: "production".to_string(),
            cert_path: cert.to_string(),
            key_path: key.to_string(),
        }
    }

    #[test]
    fn rule1_no_listeners() {
        let mut config = valid_static_config();
        config.listeners = vec![];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::NoListeners)));
    }

    #[test]
    fn rule2_wildcard_bind_rejected() {
        let mut config = valid_static_config();
        config.listeners[0].bind_addr = "0.0.0.0".to_string();
        config.listeners[0].tls = make_acme_tls();
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::WildcardBindNotAllowed { .. })));
    }

    #[test]
    fn rule2_wildcard_bind_allowed_by_config_flag() {
        let mut config = valid_static_config();
        config.listeners[0].bind_addr = "0.0.0.0".to_string();
        config.allow_wildcard_bind = true;
        config.listeners[0].tls = make_acme_tls();
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_ok());
    }

    #[test]
    fn rule2_wildcard_bind_allowed_by_cli_flag() {
        let mut config = valid_static_config();
        config.listeners[0].bind_addr = "0.0.0.0".to_string();
        config.listeners[0].tls = make_acme_tls();
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, true);
        assert!(result.is_ok());
    }

    #[test]
    fn rule2_wildcard_bind_or_logic() {
        let mut config = valid_static_config();
        config.listeners[0].bind_addr = "0.0.0.0".to_string();
        config.listeners[0].tls = make_acme_tls();
        config.allow_wildcard_bind = false;
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, true);
        assert!(result.is_ok());
    }

    #[test]
    fn rule3_duplicate_https_bind() {
        let mut config = valid_static_config();
        config.listeners = vec![
            ListenerConfig {
                bind_addr: "127.0.0.1".to_string(),
                http_port: 80,
                https_port: 443,
                tls: make_acme_tls(),
                sites: vec![],
            },
            ListenerConfig {
                bind_addr: "127.0.0.1".to_string(),
                http_port: 8080,
                https_port: 443,
                tls: make_acme_tls(),
                sites: vec![],
            },
        ];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::DuplicateHttpsBind { .. })));
    }

    #[test]
    fn rule4_acme_domains_empty() {
        let mut config = valid_static_config();
        config.listeners[0].tls = TlsConfig {
            mode: "acme".to_string(),
            acme_domains: vec![],
            acme_cache_dir: "/tmp/cache".to_string(),
            acme_directory: "production".to_string(),
            cert_path: String::new(),
            key_path: String::new(),
        };
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::AcmeDomainsEmpty { .. })));
    }

    #[test]
    fn rule5_manual_cert_missing() {
        let mut config = valid_static_config();
        config.listeners[0].tls = TlsConfig {
            mode: "manual".to_string(),
            acme_domains: vec![],
            acme_cache_dir: String::new(),
            acme_directory: "production".to_string(),
            cert_path: String::new(),
            key_path: String::new(),
        };
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::ManualCertMissing { .. })));
    }

    #[test]
    fn rule5_cert_path_not_readable() {
        let mut config = valid_static_config();
        config.listeners[0].tls = make_manual_tls("/nonexistent/cert.pem", "/nonexistent/key.pem");
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::CertPathNotReadable { .. })));
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::KeyPathNotReadable { .. })));
    }

    #[test]
    fn rule5_cert_path_readable() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");
        fs::write(&cert_path, "cert").unwrap();
        fs::write(&key_path, "key").unwrap();

        let mut config = valid_static_config();
        config.listeners[0].tls =
            make_manual_tls(cert_path.to_str().unwrap(), key_path.to_str().unwrap());
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_ok());
    }

    #[test]
    fn rule6_site_host_empty() {
        let mut config = valid_static_config();
        config.listeners[0].tls = make_acme_tls();
        config.listeners[0].sites = vec![SiteConfig {
            host: String::new(),
            upstream: "127.0.0.1:8080".to_string(),
            ..valid_dynamic_config().sites[0].clone()
        }];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::SiteHostEmpty { .. })));
    }

    #[test]
    fn rule6_site_upstream_empty() {
        let mut config = valid_static_config();
        config.listeners[0].tls = make_acme_tls();
        config.listeners[0].sites = vec![SiteConfig {
            host: "test.local".to_string(),
            upstream: String::new(),
            ..valid_dynamic_config().sites[0].clone()
        }];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::SiteUpstreamEmpty { .. })));
    }

    #[test]
    fn rule7_duplicate_site_host() {
        let mut config = valid_static_config();
        config.listeners = vec![
            ListenerConfig {
                bind_addr: "127.0.0.1".to_string(),
                http_port: 80,
                https_port: 443,
                tls: make_acme_tls(),
                sites: vec![SiteConfig {
                    host: "test.local".to_string(),
                    upstream: "127.0.0.1:8080".to_string(),
                    upstream_scheme: "http".to_string(),
                    upstream_connect_timeout_secs: 5,
                    upstream_request_timeout_secs: 60,
                }],
            },
            ListenerConfig {
                bind_addr: "127.0.0.2".to_string(),
                http_port: 80,
                https_port: 443,
                tls: make_acme_tls(),
                sites: vec![SiteConfig {
                    host: "test.local".to_string(),
                    upstream: "127.0.0.1:9090".to_string(),
                    upstream_scheme: "http".to_string(),
                    upstream_connect_timeout_secs: 5,
                    upstream_request_timeout_secs: 60,
                }],
            },
        ];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::DuplicateSiteHost { .. })));
    }

    #[test]
    fn rule8_requests_per_second_zero() {
        let config = valid_static_config();
        let mut dynamic = valid_dynamic_config();
        dynamic.rate_limit.requests_per_second = 0;
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::RequestsPerSecondZero { .. })));
    }

    #[test]
    fn rule9_body_limit_bytes_zero() {
        let config = valid_static_config();
        let mut dynamic = valid_dynamic_config();
        dynamic.body.limit_bytes = 0;
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::BodyLimitBytesZero { .. })));
    }

    #[test]
    fn rule10_duplicate_http_bind() {
        let mut config = valid_static_config();
        config.listeners = vec![
            ListenerConfig {
                bind_addr: "127.0.0.1".to_string(),
                http_port: 80,
                https_port: 443,
                tls: make_acme_tls(),
                sites: vec![],
            },
            ListenerConfig {
                bind_addr: "127.0.0.1".to_string(),
                http_port: 80,
                https_port: 8443,
                tls: make_acme_tls(),
                sites: vec![],
            },
        ];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::DuplicateHttpBind { .. })));
    }

    #[test]
    fn rule11_http_and_https_port_same() {
        let mut config = valid_static_config();
        config.listeners[0].http_port = 443;
        config.listeners[0].https_port = 443;
        config.listeners[0].tls = make_acme_tls();
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::HttpsAndHttpPortSame { .. })));
    }

    #[test]
    fn rule12_https_port_zero() {
        let mut config = valid_static_config();
        config.listeners[0].https_port = 0;
        config.listeners[0].tls = make_acme_tls();
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::HttpsPortInvalid { .. })));
    }

    #[test]
    fn rule13_http_port_disabled_valid() {
        let mut config = valid_static_config();
        config.listeners[0].http_port = 0;
        config.listeners[0].tls = make_acme_tls();
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_ok());
    }

    #[test]
    fn rule14_health_check_port_conflict() {
        let mut config = valid_static_config();
        config.health_check_port = 443;
        config.listeners[0].tls = make_acme_tls();
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::HealthCheckPortConflict { .. })));
    }

    #[test]
    fn rule14_health_check_port_conflict_with_http() {
        let mut config = valid_static_config();
        config.health_check_port = 80;
        config.listeners[0].tls = make_acme_tls();
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::HealthCheckPortConflict { .. })));
    }

    #[test]
    fn rule15_site_host_contains_port() {
        let mut config = valid_static_config();
        config.listeners[0].tls = make_acme_tls();
        config.listeners[0].sites = vec![SiteConfig {
            host: "test.local:443".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            ..valid_dynamic_config().sites[0].clone()
        }];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::SiteHostContainsPort { .. })));
    }

    #[test]
    fn rule16_site_host_is_ip() {
        let mut config = valid_static_config();
        config.listeners[0].tls = make_acme_tls();
        config.listeners[0].sites = vec![SiteConfig {
            host: "127.0.0.1".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            ..valid_dynamic_config().sites[0].clone()
        }];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::SiteHostInvalid { .. })));
    }

    #[test]
    fn rule16_hostname_normalized_lowercase() {
        let mut config = valid_static_config();
        config.listeners = vec![
            ListenerConfig {
                bind_addr: "127.0.0.1".to_string(),
                http_port: 80,
                https_port: 443,
                tls: make_acme_tls(),
                sites: vec![SiteConfig {
                    host: "Test.Local".to_string(),
                    upstream: "127.0.0.1:8080".to_string(),
                    ..valid_dynamic_config().sites[0].clone()
                }],
            },
            ListenerConfig {
                bind_addr: "127.0.0.2".to_string(),
                http_port: 80,
                https_port: 443,
                tls: make_acme_tls(),
                sites: vec![SiteConfig {
                    host: "test.local".to_string(),
                    upstream: "127.0.0.1:9090".to_string(),
                    ..valid_dynamic_config().sites[0].clone()
                }],
            },
        ];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::DuplicateSiteHost { .. })));
    }

    #[test]
    fn rule17_upstream_missing_port() {
        let mut config = valid_static_config();
        config.listeners[0].tls = make_acme_tls();
        config.listeners[0].sites = vec![SiteConfig {
            host: "test.local".to_string(),
            upstream: "gitea".to_string(),
            ..valid_dynamic_config().sites[0].clone()
        }];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::UpstreamInvalid { .. })));
    }

    #[test]
    fn rule17_upstream_with_scheme() {
        let mut config = valid_static_config();
        config.listeners[0].tls = make_acme_tls();
        config.listeners[0].sites = vec![SiteConfig {
            host: "test.local".to_string(),
            upstream: "http://gitea:3000".to_string(),
            ..valid_dynamic_config().sites[0].clone()
        }];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::UpstreamInvalid { .. })));
    }

    #[test]
    fn rule17_upstream_port_zero() {
        let mut config = valid_static_config();
        config.listeners[0].tls = make_acme_tls();
        config.listeners[0].sites = vec![SiteConfig {
            host: "test.local".to_string(),
            upstream: "gitea:0".to_string(),
            ..valid_dynamic_config().sites[0].clone()
        }];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::UpstreamInvalid { .. })));
    }

    #[test]
    fn rule18_upstream_scheme_invalid() {
        let mut config = valid_static_config();
        config.listeners[0].tls = make_acme_tls();
        config.listeners[0].sites = vec![SiteConfig {
            host: "test.local".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "ftp".to_string(),
            ..valid_dynamic_config().sites[0].clone()
        }];
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::UpstreamSchemeInvalid { .. })));
    }

    #[test]
    fn valid_config_passes() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");
        fs::write(&cert_path, "cert").unwrap();
        fs::write(&key_path, "key").unwrap();

        let config = make_static_with_sites(
            vec![SiteConfig {
                host: "test.local".to_string(),
                upstream: "127.0.0.1:8080".to_string(),
                upstream_scheme: "http".to_string(),
                upstream_connect_timeout_secs: 5,
                upstream_request_timeout_secs: 60,
            }],
            make_manual_tls(cert_path.to_str().unwrap(), key_path.to_str().unwrap()),
        );
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_ok());
    }

    #[test]
    fn collects_all_errors() {
        let config = StaticConfig {
            listeners: vec![],
            allow_wildcard_bind: false,
            health_check_port: 9900,
            admin_socket_path: "/run/reverse-proxy/admin.sock".to_string(),
            shutdown_timeout_secs: 30,
            logging: LoggingConfig::default(),
        };
        let mut dynamic = valid_dynamic_config();
        dynamic.rate_limit.requests_per_second = 0;
        dynamic.body.limit_bytes = 0;
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors.len() >= 3);
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::NoListeners)));
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::RequestsPerSecondZero { .. })));
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::BodyLimitBytesZero { .. })));
    }

    #[test]
    fn rule5_only_cert_missing() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("key.pem");
        fs::write(&key_path, "key").unwrap();

        let mut config = valid_static_config();
        config.listeners[0].tls =
            make_manual_tls("/nonexistent/cert.pem", key_path.to_str().unwrap());
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::CertPathNotReadable { .. })));
        assert!(!errors
            .iter()
            .any(|e| matches!(e, ValidationError::KeyPathNotReadable { .. })));
    }

    #[test]
    fn rule5_only_key_missing() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("cert.pem");
        fs::write(&cert_path, "cert").unwrap();

        let mut config = valid_static_config();
        config.listeners[0].tls =
            make_manual_tls(cert_path.to_str().unwrap(), "/nonexistent/key.pem");
        let dynamic = valid_dynamic_config();
        let result = validate(&config, &dynamic, false);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(!errors
            .iter()
            .any(|e| matches!(e, ValidationError::CertPathNotReadable { .. })));
        assert!(errors
            .iter()
            .any(|e| matches!(e, ValidationError::KeyPathNotReadable { .. })));
    }
}
