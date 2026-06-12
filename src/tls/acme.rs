use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use rustls_acme::caches::DirCache;
use rustls_acme::{AcmeConfig, AcmeState, EventError, EventOk, ResolvesServerCertAcme};
use tracing::{error, info, warn};

use crate::shutdown::GracefulShutdown;

const LETS_ENCRYPT_PRODUCTION_DIRECTORY: &str = "https://acme-v02.api.letsencrypt.org/directory";
const LETS_ENCRYPT_STAGING_DIRECTORY: &str =
    "https://acme-staging-v02.api.letsencrypt.org/directory";

pub struct AcmeTlsConfig {
    pub domains: Vec<String>,
    pub cache_dir: PathBuf,
    pub directory: String,
    pub contact: Vec<String>,
}

pub struct AcmeTlsSetup {
    pub resolver: Arc<ResolvesServerCertAcme>,
    pub state: AcmeState<std::io::Error, std::io::Error>,
}

impl AcmeTlsConfig {
    pub fn setup(self) -> Result<AcmeTlsSetup> {
        let directory_url = match self.directory.as_str() {
            "production" => LETS_ENCRYPT_PRODUCTION_DIRECTORY.to_string(),
            "staging" => LETS_ENCRYPT_STAGING_DIRECTORY.to_string(),
            other => other.to_string(),
        };

        let acme_config = AcmeConfig::new(self.domains.clone())
            .cache(DirCache::new(self.cache_dir.clone()))
            .directory(&directory_url)
            .contact(self.contact.iter().map(|c| c.as_str()));

        let state = acme_config.state();
        let resolver = state.resolver();

        info!(
            domains = ?self.domains,
            cache_dir = %self.cache_dir.display(),
            directory = %directory_url,
            "ACME state machine created"
        );

        Ok(AcmeTlsSetup { resolver, state })
    }

    pub fn directory_url(&self) -> &str {
        match self.directory.as_str() {
            "production" => LETS_ENCRYPT_PRODUCTION_DIRECTORY,
            "staging" => LETS_ENCRYPT_STAGING_DIRECTORY,
            other => other,
        }
    }
}

pub fn spawn_acme_state(
    state: AcmeState<std::io::Error, std::io::Error>,
    domains: Vec<String>,
    shutdown: Arc<GracefulShutdown>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        use futures::StreamExt;
        let mut state = state;
        let mut shutdown_rx = shutdown.subscribe();
        loop {
            tokio::select! {
                event = state.next() => {
                    match event {
                        Some(Ok(event)) => match event {
                            EventOk::DeployedCachedCert => {
                                info!(
                                    domains = ?domains,
                                    "ACME: deployed cached certificate"
                                );
                            }
                            EventOk::DeployedNewCert => {
                                info!(
                                    domains = ?domains,
                                    "ACME: deployed new certificate"
                                );
                            }
                            EventOk::CertCacheStore => {
                                info!(
                                    domains = ?domains,
                                    "ACME: certificate stored to cache"
                                );
                            }
                            EventOk::AccountCacheStore => {
                                info!(
                                    domains = ?domains,
                                    "ACME: account stored to cache"
                                );
                            }
                        },
                        Some(Err(err)) => match &err {
                            EventError::CertCacheLoad(e) => {
                                error!(
                                    domains = ?domains,
                                    error = ?e,
                                    "ACME: certificate cache load failed"
                                );
                            }
                            EventError::AccountCacheLoad(e) => {
                                error!(
                                    domains = ?domains,
                                    error = ?e,
                                    "ACME: account cache load failed"
                                );
                            }
                            EventError::CertCacheStore(e) => {
                                warn!(
                                    domains = ?domains,
                                    error = ?e,
                                    "ACME: certificate cache store failed"
                                );
                            }
                            EventError::AccountCacheStore(e) => {
                                warn!(
                                    domains = ?domains,
                                    error = ?e,
                                    "ACME: account cache store failed"
                                );
                            }
                            EventError::CachedCertParse(e) => {
                                error!(
                                    domains = ?domains,
                                    error = ?e,
                                    "ACME: cached certificate parse failed"
                                );
                            }
                            EventError::Order(e) => {
                                warn!(
                                    domains = ?domains,
                                    error = ?e,
                                    "ACME: certificate order failed, will retry"
                                );
                            }
                            EventError::NewCertParse(e) => {
                                error!(
                                    domains = ?domains,
                                    error = ?e,
                                    "ACME: new certificate parse failed"
                                );
                            }
                        },
                        None => {
                            info!(
                                domains = ?domains,
                                "ACME: state machine ended"
                            );
                            break;
                        }
                    }
                }
                _ = shutdown_rx.changed() => {
                    info!(
                        domains = ?domains,
                        "ACME: state machine shutting down"
                    );
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_acme_config_production_directory() {
        let config = AcmeTlsConfig {
            domains: vec!["example.com".to_string()],
            cache_dir: PathBuf::from("/tmp/test-cache"),
            directory: "production".to_string(),
            contact: vec![],
        };
        assert_eq!(config.directory_url(), LETS_ENCRYPT_PRODUCTION_DIRECTORY);
    }

    #[test]
    fn test_acme_config_staging_directory() {
        let config = AcmeTlsConfig {
            domains: vec!["example.com".to_string()],
            cache_dir: PathBuf::from("/tmp/test-cache"),
            directory: "staging".to_string(),
            contact: vec![],
        };
        assert_eq!(config.directory_url(), LETS_ENCRYPT_STAGING_DIRECTORY);
    }

    #[test]
    fn test_acme_config_custom_directory() {
        let custom_url = "https://custom-acme.example.com/directory";
        let config = AcmeTlsConfig {
            domains: vec!["example.com".to_string()],
            cache_dir: PathBuf::from("/tmp/test-cache"),
            directory: custom_url.to_string(),
            contact: vec![],
        };
        assert_eq!(config.directory_url(), custom_url);
    }

    #[test]
    fn test_acme_config_multiple_domains() {
        let config = AcmeTlsConfig {
            domains: vec!["git.alk.dev".to_string(), "alk.dev".to_string()],
            cache_dir: PathBuf::from("/var/lib/reverse-proxy/acme-cache"),
            directory: "production".to_string(),
            contact: vec!["mailto:admin@alk.dev".to_string()],
        };
        assert_eq!(config.domains.len(), 2);
        assert_eq!(config.directory_url(), LETS_ENCRYPT_PRODUCTION_DIRECTORY);
    }

    #[test]
    fn test_acme_setup_creates_resolver() {
        let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let config = AcmeTlsConfig {
            domains: vec!["test.example.com".to_string()],
            cache_dir: temp_dir.path().to_path_buf(),
            directory: "staging".to_string(),
            contact: vec!["mailto:admin@example.com".to_string()],
        };
        let setup = config.setup().expect("setup should succeed");
        assert!(Arc::strong_count(&setup.resolver) >= 1);
    }
}
