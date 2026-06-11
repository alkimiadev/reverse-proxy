---
status: draft
last_updated: 2026-06-11
---

# TLS Termination

## What It Is

The TLS termination component handles all aspects of encrypted connections:
certificate provisioning (ACME and manual), TLS handshake, SNI-based certificate
selection, and connection wrapping for the axum router.

## Why It Exists

TLS termination is the security boundary between the public internet and our
upstream services. It replaces nginx's `ssl_certificate`, `ssl_protocols`, and
`ssl_ciphers` configuration with a memory-safe Rust implementation using rustls.

## Architecture

```
                    ┌──────────────────────────────────────────┐
                    │          TLS Termination                   │
                    │                                            │
  bind_addr:443 ──► │  TcpListener::bind(bind_addr)             │
                    │       │                                    │
                    │       ▼                                    │
                    │  tokio-rustls::TlsAcceptor                 │
                    │       │                                    │
                    │       ├─ ACME mode:                        │
                    │       │  rustls-acme::ResolvesServerCertAcme │
                    │       │  (auto-provisions & renews certs)   │
                    │       │                                    │
                    │       └─ Manual mode:                        │
                    │          rustls::ServerConfig               │
                    │          .with_single_cert(cert_chain, key) │
                    │                                            │
                    │       │                                    │
                    │       ▼                                    │
                    │  TlsStream<TcpStream>                      │
                    │       │                                    │
                    │       ▼                                    │
                    │  hyper::service_fn → axum router            │
                    └──────────────────────────────────────────┘

  bind_addr:80  ──►  HTTP listener (redirect to HTTPS, no TLS)
```

## Certificate Provisioning

### ACME Mode (Primary)

Uses `rustls-acme` for automatic certificate provisioning and renewal through
Let's Encrypt. This is the primary mode — no certbot dependency, no cron jobs,
no deploy hooks.

**How it works:**

1. `AcmeCertProvider` configures the ACME client with the domain list, cache
   directory, and Let's Encrypt directory (staging or production).
2. `AcmeConfig::new(domains)` creates an ACME configuration for all listed
   domains. Let's Encrypt will issue a single SAN certificate covering all
   domains.
3. The ACME state machine runs as a background tokio task, handling:
   - Account registration with Let's Encrypt
   - Certificate ordering
   - TLS-ALPN-01 challenge (or HTTP-01 challenge)
   - Certificate issuance
   - Certificate renewal (automatic, ~30 days before expiry)
4. `ResolvesServerCertAcme` is a rustls `ResolvesServerCert` implementation
   that automatically serves the ACME-provisioned certificate.
5. When a new certificate is issued, the resolver updates atomically — no
   restart or signal handling needed.

**Configuration:**

```toml
[server.tls]
mode = "acme"
acme_domains = ["git.alk.dev", "alk.dev"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"  # or "staging" for testing
```

**Cache directory:** The `DirCache` from rustls-acme persists ACME account data,
private keys, and certificates between restarts. This avoids re-provisioning on
every restart.

### Manual Mode (Fallback)

For environments where ACME is not desired (testing, self-signed certs,
corporate CAs, or BYO certificates), the proxy loads certificates from file
paths at startup.

```toml
[tls]
mode = "manual"
cert_path = "/etc/letsencrypt/live/git.alk.dev/fullchain.pem"
key_path = "/etc/letsencrypt/live/git.alk.dev/privkey.pem"
```

Certificate files are loaded once at startup using `rustls_pemfile`. Manual
mode requires a restart to pick up new certificates. See ADR-004 for the
rationale behind making ACME the primary mode and manual mode restart-dependent.

## TLS Configuration

### Protocol Versions

The proxy supports TLS 1.2 and TLS 1.3 only, matching the minimum security
level of the current nginx configuration. The `aws_lc_rs` crypto provider
defaults to these protocol versions; explicit configuration ensures no
regression if defaults change in future rustls releases.

### Cipher Suites

rustls 0.23 with the `aws_lc_rs` crypto provider defaults to a conservative
cipher suite selection that excludes all weak ciphers (no SHA-1, no 3DES, no
RC4, no CBC-mode suites, no RSA key exchange).

The current nginx config explicitly restricts to:

```
ECDHE-ECDSA-AES128-GCM-SHA256
ECDHE-RSA-AES128-GCM-SHA256
ECDHE-ECDSA-AES256-GCM-SHA384
ECDHE-RSA-AES256-GCM-SHA384
```

rustls's defaults include these plus TLS 1.3 suites (which nginx's config
also allows via `TLSv1.3`). The default rustls cipher list is a strict subset
of what browsers accept.

See [open-questions.md](open-questions.md) OQ-01 for whether to further
restrict cipher suites beyond rustls defaults.

### ServerConfig Construction

For manual mode, the `ServerConfig` is built with `with_no_client_auth()` and
a custom `ResolvesServerCert` implementation that maps SNI hostnames to
certificate/key pairs loaded from disk.

For ACME mode, the `ServerConfig` is built with `with_cert_resolver()`, passing
the `ResolvesServerCertAcme` resolver. The ACME configuration includes all
domains listed in `acme_domains`, and the resolver manages a single SAN
certificate covering all of them. The ACME TLS-ALPN-01 protocol identifier
(`acme-tls/1`) must be registered in the `alpn_protocols` list so the server
can respond to TLS-ALPN-01 challenges.

Both modes use the `aws_lc_rs` crypto provider with safe default protocol
versions (TLS 1.2 and TLS 1.3).

## SNI-Based Certificate Selection

### ACME Mode (Multi-Domain)

In ACME mode, `rustls-acme` manages a single SAN certificate covering all
configured domains. The `ResolvesServerCertAcme` resolver automatically serves
the correct certificate during the TLS handshake.

1. **TLS handshake**: The client sends the SNI extension indicating which
   hostname it's connecting to.
2. **Certificate resolution**: `ResolvesServerCertAcme` matches the SNI
   hostname against the provisioned certificate's Subject Alternative Names
   and serves the certificate.
3. **HTTP routing**: After the TLS handshake, axum's `Host` extractor routes
   the request to the correct site handler based on the `Host` header.

This is the same pattern nginx uses — SNI selects the cert during TLS, then
`Host` header selects the server block. ACME mode handles this automatically
through the cert resolver.

### Manual Mode (Multi-Domain)

In manual mode, a custom `ResolvesServerCert` implementation is required to
map SNI hostnames to the correct `CertifiedKey`. This implementation:

1. Loads certificate files at startup (or on SIGHUP for reload)
2. Maps each domain name to its certificate chain and private key
3. During the TLS handshake, looks up the SNI hostname and returns the
   matching `CertifiedKey`

The custom resolver must handle the case where no matching certificate exists
for the SNI hostname — in this case, the handshake fails, which is the
correct behavior (we don't serve a default certificate for unknown domains).

See [open-questions.md](open-questions.md) OQ-07 for per-site TLS overrides.

## HTTP Listener (Port 80)

The HTTP listener on port 80 is a plain TCP listener with no TLS. It has one
job: redirect all requests to the HTTPS equivalent.

The listener binds to the same IP address as the TLS listener, but on port 80.

### ACME Challenge Type

The default ACME challenge type is **TLS-ALPN-01**, since the proxy already
listens on port 443. This avoids requiring a separate HTTP-01 challenge server.
HTTP-01 is available as a fallback for environments where TLS-ALPN-01 is not
suitable (e.g., behind a CDN that terminates TLS). When using HTTP-01, the
port 80 listener serves `/.well-known/acme-challenge/{token}` paths for
challenge verification.

## Key Files and Crates

| Component | Crate | Purpose |
|-----------|-------|---------|
| TLS acceptor | `tokio-rustls` 0.26 | Async TLS handshake over TCP streams |
| TLS config | `rustls` 0.23 | ServerConfig, CryptoProvider, cipher suites |
| ACME client | `rustls-acme` 0.12 | Automatic cert provisioning and renewal |
| PEM parsing | `rustls-pemfile` 2 | Load cert/key from PEM files (manual mode) |
| PKI types | `rustls-pki-types` 1 | CertificateDer, PrivateKeyDer |

## Design Decisions

All design decisions are documented as ADRs in [decisions/](decisions/).

| ADR | Decision | Summary |
|-----|----------|---------|
| [004](decisions/004-rustls-acme.md) | ACME-primary cert management | Eliminates certbot; automatic provisioning and renewal |
| [005](decisions/005-tokio-rustls-direct.md) | tokio-rustls directly | Full control over TLS config and ACME resolver integration |
| [010](decisions/010-multi-site-phase1.md) | Multi-site in Phase 1 | Multiple domains from initial release |
| [011](decisions/011-multi-domain-tls.md) | Multi-domain TLS config | Single SAN certificate covering all domains via rustls-acme |

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). Key
questions affecting this document:

- **OQ-01**: Should cipher suites be restricted beyond rustls defaults? (open)
- **OQ-07**: Should per-site TLS overrides be supported for mixed ACME/manual
  domains? (open)