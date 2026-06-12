use std::sync::Arc;

use anyhow::{bail, Context, Result};
use rustls::version::{TLS12, TLS13};
use rustls::ServerConfig;
use tracing::info;

use super::acme::{spawn_acme_state, AcmeTlsConfig};
use super::config::crypto_provider;
use crate::config::static_config::TlsConfig;
use crate::shutdown::GracefulShutdown;

const ACME_TLS_ALPN_01: &[u8] = b"acme-tls/1";

fn build_acme_server_config(
    resolver: Arc<rustls_acme::ResolvesServerCertAcme>,
) -> Result<Arc<ServerConfig>> {
    let provider = crypto_provider();
    let config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS12, &TLS13])
        .context("failed to set TLS protocol versions")?
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    let mut config = (*Arc::new(config)).clone();
    config.alpn_protocols = vec![
        b"h2".to_vec(),
        b"http/1.1".to_vec(),
        ACME_TLS_ALPN_01.to_vec(),
    ];
    Ok(Arc::new(config))
}

#[derive(Debug)]
#[non_exhaustive]
pub enum TlsMode {
    Manual(Arc<ServerConfig>),
    Acme {
        default_config: Arc<ServerConfig>,
        resolver: Arc<rustls_acme::ResolvesServerCertAcme>,
    },
}

pub fn setup_tls(tls_config: &TlsConfig, shutdown: Arc<GracefulShutdown>) -> Result<TlsMode> {
    match tls_config.mode.as_str() {
        "manual" => {
            if tls_config.cert_path.is_empty() {
                bail!("manual TLS mode requires cert_path");
            }
            if tls_config.key_path.is_empty() {
                bail!("manual TLS mode requires key_path");
            }
            let config = super::config::build_manual_server_config(
                &tls_config.cert_path,
                &tls_config.key_path,
            )?;
            Ok(TlsMode::Manual(Arc::new(config)))
        }
        "acme" => {
            if tls_config.acme_domains.is_empty() {
                bail!("ACME TLS mode requires at least one domain in acme_domains");
            }
            if tls_config.acme_cache_dir.is_empty() {
                bail!("ACME TLS mode requires acme_cache_dir");
            }

            let acme_tls_config = AcmeTlsConfig {
                domains: tls_config.acme_domains.clone(),
                cache_dir: tls_config.acme_cache_dir.clone().into(),
                directory: tls_config.acme_directory.clone(),
                contact: vec![tls_config.acme_contact.clone()],
            };

            let super::acme::AcmeTlsSetup { resolver, state } = acme_tls_config.setup()?;

            let default_config = build_acme_server_config(resolver.clone())?;

            spawn_acme_state(state, tls_config.acme_domains.clone(), shutdown);

            info!(
                domains = ?tls_config.acme_domains,
                "ACME TLS mode initialized"
            );

            Ok(TlsMode::Acme {
                default_config,
                resolver,
            })
        }
        other => {
            bail!("unknown TLS mode: '{}', expected 'manual' or 'acme'", other);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_acme_tls_alpn_value() {
        assert_eq!(ACME_TLS_ALPN_01, b"acme-tls/1");
    }

    fn make_test_resolver() -> Arc<rustls_acme::ResolvesServerCertAcme> {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let config = rustls_acme::AcmeConfig::new(["test.example.com"])
            .cache(rustls_acme::caches::DirCache::new(
                temp_dir.path().to_path_buf(),
            ))
            .directory("https://acme-staging-v02.api.letsencrypt.org/directory");
        let state = config.state();
        state.resolver()
    }

    #[test]
    fn test_build_acme_server_config() {
        let resolver = make_test_resolver();
        let config = build_acme_server_config(resolver);
        assert!(config.is_ok());

        let config = config.unwrap();
        assert!(config.alpn_protocols.contains(&b"h2".to_vec()));
        assert!(config.alpn_protocols.contains(&b"http/1.1".to_vec()));
        assert!(config.alpn_protocols.contains(&ACME_TLS_ALPN_01.to_vec()));
    }

    #[test]
    fn test_setup_tls_manual_missing_cert_path() {
        let tls_config = TlsConfig {
            mode: "manual".to_string(),
            acme_domains: vec![],
            acme_cache_dir: String::new(),
            acme_directory: "production".to_string(),
            acme_contact: String::new(),
            cert_path: String::new(),
            key_path: "/some/key.pem".to_string(),
        };
        let shutdown = Arc::new(GracefulShutdown::new(30));
        let result = setup_tls(&tls_config, shutdown);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("cert_path"));
    }

    #[test]
    fn test_setup_tls_manual_missing_key_path() {
        let tls_config = TlsConfig {
            mode: "manual".to_string(),
            acme_domains: vec![],
            acme_cache_dir: String::new(),
            acme_directory: "production".to_string(),
            acme_contact: String::new(),
            cert_path: "/some/cert.pem".to_string(),
            key_path: String::new(),
        };
        let shutdown = Arc::new(GracefulShutdown::new(30));
        let result = setup_tls(&tls_config, shutdown);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("key_path"));
    }

    #[test]
    fn test_setup_tls_acme_missing_domains() {
        let tls_config = TlsConfig {
            mode: "acme".to_string(),
            acme_domains: vec![],
            acme_cache_dir: "/tmp/cache".to_string(),
            acme_directory: "staging".to_string(),
            acme_contact: "mailto:admin@example.com".to_string(),
            cert_path: String::new(),
            key_path: String::new(),
        };
        let shutdown = Arc::new(GracefulShutdown::new(30));
        let result = setup_tls(&tls_config, shutdown);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("acme_domains"));
    }

    #[test]
    fn test_setup_tls_acme_missing_cache_dir() {
        let tls_config = TlsConfig {
            mode: "acme".to_string(),
            acme_domains: vec!["example.com".to_string()],
            acme_cache_dir: String::new(),
            acme_directory: "staging".to_string(),
            acme_contact: "mailto:admin@example.com".to_string(),
            cert_path: String::new(),
            key_path: String::new(),
        };
        let shutdown = Arc::new(GracefulShutdown::new(30));
        let result = setup_tls(&tls_config, shutdown);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("acme_cache_dir"));
    }

    #[test]
    fn test_setup_tls_unknown_mode() {
        let tls_config = TlsConfig {
            mode: "invalid".to_string(),
            acme_domains: vec![],
            acme_cache_dir: String::new(),
            acme_directory: "production".to_string(),
            acme_contact: String::new(),
            cert_path: String::new(),
            key_path: String::new(),
        };
        let shutdown = Arc::new(GracefulShutdown::new(30));
        let result = setup_tls(&tls_config, shutdown);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown TLS mode"));
    }
}
