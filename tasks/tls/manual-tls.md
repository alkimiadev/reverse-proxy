---
id: tls/manual-tls
name: Implement manual TLS certificate loading and ServerConfig construction
status: completed
depends_on: [setup/project-init]
scope: narrow
risk: low
impact: component
level: implementation
---

## Description

Implement the manual TLS mode where certificates are loaded from PEM files on disk at startup. This covers building a `rustls::ServerConfig` with manually loaded certificate chains and private keys.

### Manual Mode

For each listener in manual mode:
1. Load `cert_path` PEM file using `rustls_pemfile` → `Vec<CertificateDer>`
2. Load `key_path` PEM file using `rustls_pemfile` → `PrivateKeyDer`
3. Build `ServerConfig` with `with_no_client_auth()` and the loaded cert/key
4. Configure cipher suites (restricted set per ADR-012)
5. Configure protocol versions (TLS 1.2 and 1.3 only)

### Cipher Suite Configuration

Per ADR-012, restrict to nginx-equivalent cipher suites:

**TLS 1.2 (explicitly selected):**
- `TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256`
- `TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256`
- `TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384`
- `TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384`

**TLS 1.3 (all default suites):**
- `TLS_AES_128_GCM_SHA256`
- `TLS_AES_256_GCM_SHA384`
- `TLS_CHACHA20_POLY1305_SHA256`

This is configured via a custom `CryptoProvider` with a `cipher_suite` list passed to `ServerConfig::builder_with_provider()`.

### Single-Domain Manual Mode

For a listener with one domain, build a simple `ServerConfig` with the single certificate chain and private key. No SNI resolver needed.

### Multi-Domain Manual Mode (on shared-IP listener)

For a listener with multiple sites on a shared IP, implement a custom `ResolvesServerCert` that maps SNI hostnames to `CertifiedKey` entries loaded from disk. If no certificate matches the SNI hostname, the handshake fails — we don't serve a default certificate for unknown domains.

Note: multi-domain manual mode with different certs per domain is a rare edge case. The initial implementation should handle the common case (single cert per manual listener). The SNI resolver can be a follow-up if needed.

## Acceptance Criteria

- [ ] `rustls::ServerConfig` construction for manual TLS mode
- [ ] PEM file loading via `rustls_pemfile` for certificates and private keys
- [ ] Cipher suite restriction per ADR-012 (4 TLS 1.2 suites + all TLS 1.3)
- [ ] Protocol version restriction to TLS 1.2 and 1.3
- [ `aws_lc_rs` crypto provider used
- [ ] `with_no_client_auth()` for no client certificate requirement
- [ ] Custom `ResolvesServerCert` for SNI-based cert selection in multi-domain manual mode
- [ ] Unknown SNI hostname → handshake fails (no default cert)
- [ ] Unit tests for ServerConfig construction with test certs (using `rcgen`)
- [ ] Unit tests for cipher suite and protocol version configuration

## References

- docs/architecture/tls.md — manual mode, cipher suites, SNI
- docs/architecture/decisions/004-rustls-acme.md — manual mode is fallback
- docs/architecture/decisions/005-tokio-rustls-direct.md — direct tokio-rustls usage
- docs/architecture/decisions/012-cipher-suite-restriction.md — cipher suite selection

## Notes

> This task focuses on ServerConfig construction. The actual TCP listener + TLS acceptor wiring is in tls/tls-listener-setup.

## Summary

> To be filled on completion