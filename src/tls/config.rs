use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use rustls::crypto::aws_lc_rs::cipher_suite;
use rustls::crypto::aws_lc_rs::{default_provider, kx_group};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use rustls::version::{TLS12, TLS13};
use rustls::ServerConfig;
use rustls::SupportedCipherSuite;
use rustls_pemfile;

#[allow(dead_code)]
static RESTRICTED_CIPHER_SUITES: &[SupportedCipherSuite] = &[
    cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256,
    cipher_suite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256,
    cipher_suite::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384,
    cipher_suite::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384,
    cipher_suite::TLS13_AES_128_GCM_SHA256,
    cipher_suite::TLS13_AES_256_GCM_SHA384,
    cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
];

pub(crate) fn crypto_provider() -> Arc<rustls::crypto::CryptoProvider> {
    let provider = default_provider();
    Arc::new(rustls::crypto::CryptoProvider {
        cipher_suites: RESTRICTED_CIPHER_SUITES.to_vec(),
        kx_groups: vec![kx_group::X25519, kx_group::SECP256R1, kx_group::SECP384R1],
        ..provider
    })
}

pub fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let file =
        File::open(path).with_context(|| format!("failed to open certificate file: {path}"))?;
    let mut reader = BufReader::new(file);
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| format!("failed to parse certificate file: {path}"))?;
    if certs.is_empty() {
        bail!("no certificates found in {path}");
    }
    Ok(certs)
}

pub fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let file =
        File::open(path).with_context(|| format!("failed to open private key file: {path}"))?;
    let mut reader = BufReader::new(file);
    let key = rustls_pemfile::private_key(&mut reader)
        .with_context(|| format!("failed to parse private key file: {path}"))?;
    key.context(format!("no private key found in {path}"))
}

pub fn build_manual_server_config(cert_path: &str, key_path: &str) -> Result<ServerConfig> {
    let certs = load_certs(cert_path)?;
    let key = load_private_key(key_path)?;

    let provider = crypto_provider();
    let config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS12, &TLS13])
        .with_context(|| "failed to set protocol versions")?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .with_context(|| "failed to configure certificate/key pair")?;

    Ok(config)
}

pub fn build_multi_domain_server_config(
    domain_certs: &HashMap<String, (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)>,
) -> Result<ServerConfig> {
    let provider = crypto_provider();

    let mut resolver = SniCertResolver::new();
    for (domain, (certs, key)) in domain_certs {
        let certified_key = CertifiedKey::from_der(certs.clone(), key.clone_key(), &provider)
            .with_context(|| format!("failed to load cert/key for domain {domain}"))?;
        resolver.add(domain, Arc::new(certified_key));
    }

    let config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&TLS12, &TLS13])
        .with_context(|| "failed to set protocol versions")?
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(resolver));

    Ok(config)
}

#[derive(Debug)]
struct SniCertResolver {
    entries: HashMap<String, Arc<CertifiedKey>>,
}

impl SniCertResolver {
    fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    fn add(&mut self, domain: &str, certified_key: Arc<CertifiedKey>) {
        self.entries.insert(domain.to_lowercase(), certified_key);
    }
}

impl ResolvesServerCert for SniCertResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        let server_name = client_hello.server_name()?;
        self.entries.get(&server_name.to_lowercase()).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{CertificateParams, IsCa, KeyPair};
    use rustls::pki_types::PrivatePkcs8KeyDer;

    fn generate_test_cert(domain: &str) -> (Vec<CertificateDer<'static>>, PrivateKeyDer<'static>) {
        let mut params =
            CertificateParams::new(vec![domain.to_string()]).expect("failed to create cert params");
        params.is_ca = IsCa::NoCa;
        let key_pair = KeyPair::generate().expect("failed to generate key pair");
        let cert = params
            .self_signed(&key_pair)
            .expect("failed to self-sign cert");
        let cert_der = cert.der().clone();
        let key_der = key_pair.serialize_der();
        let private_key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));
        (vec![cert_der], private_key)
    }

    fn generate_test_cert_pem(domain: &str) -> (String, String) {
        let mut params =
            CertificateParams::new(vec![domain.to_string()]).expect("failed to create cert params");
        params.is_ca = IsCa::NoCa;
        let key_pair = KeyPair::generate().expect("failed to generate key pair");
        let cert = params
            .self_signed(&key_pair)
            .expect("failed to self-sign cert");
        let cert_pem = cert.pem();
        let key_pem = key_pair.serialize_pem();
        (cert_pem, key_pem)
    }

    #[test]
    fn test_build_manual_server_config() {
        let (certs, key) = generate_test_cert("test.example.com");
        let provider = crypto_provider();
        let config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&TLS12, &TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap();
        assert!(!config.ignore_client_order);
    }

    #[test]
    fn test_load_certs_from_pem() {
        let dir = tempfile::tempdir().unwrap();
        let (cert_pem, _) = generate_test_cert_pem("test.example.com");
        let cert_path = dir.path().join("cert.pem");
        std::fs::write(&cert_path, cert_pem).unwrap();
        let certs = load_certs(cert_path.to_str().unwrap()).unwrap();
        assert_eq!(certs.len(), 1);
    }

    #[test]
    fn test_load_private_key_from_pem() {
        let dir = tempfile::tempdir().unwrap();
        let (_, key_pem) = generate_test_cert_pem("test.example.com");
        let key_path = dir.path().join("key.pem");
        std::fs::write(&key_path, key_pem).unwrap();
        let key = load_private_key(key_path.to_str().unwrap()).unwrap();
        assert!(matches!(key, PrivateKeyDer::Pkcs8(_)));
    }

    #[test]
    fn test_build_manual_server_config_from_files() {
        let dir = tempfile::tempdir().unwrap();
        let (cert_pem, key_pem) = generate_test_cert_pem("test.example.com");
        let cert_path = dir.path().join("cert.pem");
        let key_path = dir.path().join("key.pem");
        std::fs::write(&cert_path, &cert_pem).unwrap();
        std::fs::write(&key_path, &key_pem).unwrap();
        let config =
            build_manual_server_config(cert_path.to_str().unwrap(), key_path.to_str().unwrap())
                .unwrap();
        assert!(!config.ignore_client_order);
    }

    #[test]
    fn test_cipher_suite_restriction() {
        let provider = crypto_provider();
        assert_eq!(provider.cipher_suites.len(), 7);

        let cipher_suites: Vec<String> = provider
            .cipher_suites
            .iter()
            .map(|cs| format!("{cs:?}"))
            .collect();

        assert!(cipher_suites
            .iter()
            .any(|cs| cs.contains("AES_256_GCM_SHA384")));
        assert!(cipher_suites
            .iter()
            .any(|cs| cs.contains("AES_128_GCM_SHA256")));
        assert!(cipher_suites
            .iter()
            .any(|cs| cs.contains("CHACHA20_POLY1305_SHA256")));
        assert!(cipher_suites
            .iter()
            .any(|cs| cs.contains("ECDHE_ECDSA_WITH_AES_256_GCM_SHA384")));
        assert!(cipher_suites
            .iter()
            .any(|cs| cs.contains("ECDHE_ECDSA_WITH_AES_128_GCM_SHA256")));
        assert!(cipher_suites
            .iter()
            .any(|cs| cs.contains("ECDHE_RSA_WITH_AES_256_GCM_SHA384")));
        assert!(cipher_suites
            .iter()
            .any(|cs| cs.contains("ECDHE_RSA_WITH_AES_128_GCM_SHA256")));
    }

    #[test]
    fn test_no_chacha20_for_tls12() {
        let provider = crypto_provider();
        let tls12_chacha = provider.cipher_suites.iter().any(|cs| {
            let dbg = format!("{cs:?}");
            dbg.contains("ECDHE") && dbg.contains("CHACHA20")
        });
        assert!(
            !tls12_chacha,
            "TLS 1.2 ChaCha20 suites should not be present"
        );
    }

    #[test]
    fn test_protocol_versions_configured() {
        let (certs, key) = generate_test_cert("test.example.com");
        let provider = crypto_provider();
        let _config = ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&TLS12, &TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .unwrap();
    }

    #[test]
    fn test_sni_resolver_known_domain() {
        let (certs, key) = generate_test_cert("example.com");
        let provider = crypto_provider();
        let certified_key = CertifiedKey::from_der(certs, key, &provider).unwrap();
        let mut resolver = SniCertResolver::new();
        resolver.add("example.com", Arc::new(certified_key));

        let resolved = resolver.entries.get("example.com");
        assert!(resolved.is_some());
    }

    #[test]
    fn test_sni_resolver_unknown_domain_returns_none() {
        let (certs, key) = generate_test_cert("example.com");
        let provider = crypto_provider();
        let certified_key = CertifiedKey::from_der(certs, key, &provider).unwrap();
        let mut resolver = SniCertResolver::new();
        resolver.add("example.com", Arc::new(certified_key));

        let resolved = resolver.entries.get("unknown.com");
        assert!(resolved.is_none());
    }

    #[test]
    fn test_sni_resolver_case_insensitive() {
        let (certs, key) = generate_test_cert("Example.COM");
        let provider = crypto_provider();
        let certified_key = CertifiedKey::from_der(certs, key, &provider).unwrap();
        let mut resolver = SniCertResolver::new();
        resolver.add("Example.COM", Arc::new(certified_key));

        assert!(resolver.entries.contains_key("example.com"));
        assert!(!resolver.entries.contains_key("Example.COM"));
    }

    #[test]
    fn test_build_multi_domain_server_config() {
        let (certs1, key1) = generate_test_cert("site1.example.com");
        let (certs2, key2) = generate_test_cert("site2.example.com");

        let mut domain_certs = HashMap::new();
        domain_certs.insert("site1.example.com".to_string(), (certs1, key1));
        domain_certs.insert("site2.example.com".to_string(), (certs2, key2));

        let config = build_multi_domain_server_config(&domain_certs).unwrap();
        assert!(!config.ignore_client_order);
    }

    #[test]
    fn test_load_certs_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let cert_path = dir.path().join("empty.pem");
        std::fs::write(&cert_path, "").unwrap();
        let result = load_certs(cert_path.to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_load_certs_nonexistent_file() {
        let result = load_certs("/nonexistent/path/cert.pem");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_private_key_nonexistent_file() {
        let result = load_private_key("/nonexistent/path/key.pem");
        assert!(result.is_err());
    }
}
