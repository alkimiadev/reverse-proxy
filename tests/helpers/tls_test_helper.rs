use rcgen::{CertificateParams, DistinguishedName, KeyPair};

pub struct SelfSignedCert {
    pub cert_pem: String,
    pub key_pem: String,
}

pub fn generate_self_signed_cert(domains: &[&str]) -> SelfSignedCert {
    let mut params = CertificateParams::new(
        domains
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<String>>(),
    )
    .unwrap();
    params.distinguished_name = DistinguishedName::new();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "test.local");

    let key_pair = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key_pair).unwrap();

    SelfSignedCert {
        cert_pem: cert.pem(),
        key_pem: key_pair.serialize_pem(),
    }
}
