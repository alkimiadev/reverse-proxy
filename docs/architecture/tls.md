---
status: reviewed
last_updated: 2026-06-12
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

The proxy supports multiple independent TLS listeners, each with its own bind
address, TLS configuration, and site routing. See ADR-019 for the rationale.

```
                    ┌──────────────────────────────────────────┐
                    │          TLS Termination                   │
                    │                                            │
                    │  ┌─ Listener 1 ─────────────────────────┐ │
                    │  │  bind_addr_1:443                       │ │
                    │  │  TcpListener::bind(bind_addr_1)       │ │
                    │  │       │                               │ │
                    │  │       ▼                               │ │
                    │  │  tokio-rustls::TlsAcceptor            │ │
                    │  │       │                               │ │
                    │  │  ACME or Manual TLS config            │ │
                    │  │  (per-listener TLS mode)              │ │
                    │  │       │                               │ │
                    │  │       ▼                               │ │
                    │  │  TlsStream<TcpStream>                 │ │
                    │  │       │                               │ │
                    │  │       ▼                               │ │
                    │  │  axum router (per-listener sites)     │ │
                    │  └───────────────────────────────────────┘ │
                    │                                            │
                    │  ┌─ Listener N ─────────────────────────┐ │
                    │  │  bind_addr_N:443                       │ │
                    │  │  ... (same structure)                  │ │
                    │  └───────────────────────────────────────┘ │
                    └──────────────────────────────────────────┘

  bind_addr:80  ──►  HTTP listener (redirect to HTTPS, no TLS)
```

Each listener is independently configured. This supports two deployment models:

1. **Shared-IP multi-domain**: One listener with multiple domains in
   `acme_domains`, using a single SAN certificate and SNI routing.
2. **Dedicated-IP single-domain**: Multiple listeners, each with its own IP,
   its own TLS certificate, and its own site. No SNI needed.

## Certificate Provisioning

Each listener has its own TLS mode (ACME or manual), configured independently.

### ACME Mode (Primary)

Uses `rustls-acme` for automatic certificate provisioning and renewal through
Let's Encrypt. This is the primary mode — no certbot dependency, no cron jobs,
no deploy hooks.

**How it works:**

1. Each listener in ACME mode creates its own `AcmeCertProvider` with the
   listener's domain list, cache directory, and Let's Encrypt directory.
2. `AcmeConfig::new(domains)` creates an ACME configuration for the domains
   listed in that listener's `acme_domains`. Let's Encrypt will issue a
   certificate covering those domains (a single SAN certificate or a
   single-domain certificate, depending on how many domains are listed).
3. The `acme_contact` field provides a contact email address (as a `mailto:`
   URI) required by Let's Encrypt for production certificate requests. Without
   a contact email, Let's Encrypt production API returns a 400-level error.
4. The ACME state machine runs as a background tokio task per listener,
   handling:
   - Account registration with Let's Encrypt
   - Certificate ordering
   - TLS-ALPN-01 challenge (or HTTP-01 challenge)
   - Certificate issuance
   - Certificate renewal (automatic, ~30 days before expiry)
5. `ResolvesServerCertAcme` is a rustls `ResolvesServerCert` implementation
   that automatically serves the ACME-provisioned certificate.
6. When a new certificate is issued, the resolver updates atomically — no
   restart or signal handling needed.

**Configuration (within a `[[listeners]]` entry):**

```toml
[[listeners]]
bind_addr = "203.0.113.10"

[listeners.tls]
mode = "acme"
acme_domains = ["git.alk.dev", "alk.dev"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"  # or "staging" for testing
acme_contact = "mailto:admin@alk.dev"  # Required for Let's Encrypt production
```

**Cache directory:** The `DirCache` from rustls-acme persists ACME account data,
private keys, and certificates between restarts. This avoids re-provisioning on
every restart. Each listener should use its own cache directory to avoid conflicts
between separate ACME state machines.

### Manual Mode (Fallback)

For environments where ACME is not desired (testing, self-signed certs,
corporate CAs, or BYO certificates), the proxy loads certificates from file
paths at startup.

```toml
[[listeners]]
bind_addr = "203.0.113.11"

[listeners.tls]
mode = "manual"
cert_path = "/etc/ssl/alk.dev/fullchain.pem"
key_path = "/etc/ssl/alk.dev/privkey.pem"
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

Cipher suites are explicitly restricted to match the scope of our current nginx
configuration. See ADR-012 for the full rationale.

**TLS 1.2 (explicitly selected):**

- `TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256`
- `TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256`
- `TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384`
- `TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384`

**TLS 1.3 (all default suites):**

- `TLS_AES_128_GCM_SHA256`
- `TLS_AES_256_GCM_SHA384`
- `TLS_CHACHA20_POLY1305_SHA256`

This is configured by building a `CryptoProvider` with a custom `cipher_suite`
list and passing it to `ServerConfig::builder_with_provider()`. The cipher
list matches our current nginx configuration's scope, providing behavioral
parity during migration.

### ServerConfig Construction

Each listener constructs its own `ServerConfig` based on its TLS mode.

For manual mode, the `ServerConfig` is built with `with_no_client_auth()` and
the loaded certificate chain and private key. If the listener serves multiple
domains from a single listener, a custom `ResolvesServerCert` implementation
maps SNI hostnames to certificate/key pairs loaded from disk.

For ACME mode, the `ServerConfig` is built with `with_cert_resolver()`, passing
the `ResolvesServerCertAcme` resolver. The ACME configuration includes the
domains listed in that listener's `acme_domains`, and the resolver manages the
certificate.

The TLS `ServerConfig` advertises ALPN protocols to enable HTTP/2 negotiation.
The ALPN configuration differs by TLS mode:

- **ACME mode**: `h2`, `http/1.1`, and `acme-tls/1`. The `acme-tls/1` entry is
  required for TLS-ALPN-01 challenge verification during certificate provisioning.
- **Manual mode** (single-cert and multi-domain/SNI): `h2` and `http/1.1` only.
  The `acme-tls/1` entry is not included because manual mode does not use ACME
  challenges.

After the TLS handshake, the proxy inspects the negotiated ALPN protocol to
select the appropriate HTTP server: `h2` triggers
`hyper::server::conn::http2::Builder`, while `http/1.1` (or no ALPN) triggers
`hyper_util::server::conn::auto::Builder`. See ADR-023 for details.

Both modes use the `aws_lc_rs` crypto provider with safe default protocol
versions (TLS 1.2 and TLS 1.3).

## SNI-Based Certificate Selection

After the TLS handshake, the proxy inspects the negotiated ALPN protocol to
determine whether to serve the connection as HTTP/2 or HTTP/1.1. If the client
negotiated `h2` via ALPN, the proxy uses `hyper::server::conn::http2::Builder`;
otherwise, it uses `hyper_util::server::conn::auto::Builder` with HTTP/1.1
and upgrade support. See ADR-023 for details.

### Dedicated-IP Single-Domain (Multi-Config)

In the dedicated-IP model, each listener binds to its own IP address and serves
exactly one domain with one certificate. SNI is not required for certificate
selection — the listener's TLS config already has the correct certificate.

This is the simplest case: one IP, one listener, one certificate, one domain.
No SNI resolution logic is needed.

### Shared-IP Multi-Domain (SAN Certificate)

In the shared-IP model, a single listener serves multiple domains using a SAN
certificate. SNI-based certificate selection is required.

In ACME mode, `rustls-acme` manages a single SAN certificate covering all
configured domains for that listener. The `ResolvesServerCertAcme` resolver
automatically serves the correct certificate during the TLS handshake.

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

### Manual Mode with Multiple Domains

In manual mode on a shared-IP listener, a custom `ResolvesServerCert`
implementation maps SNI hostnames to the correct `CertifiedKey`. This
implementation:

1. Loads certificate files at startup (or on SIGHUP for reload)
2. Maps each domain name to its certificate chain and private key
3. During the TLS handshake, looks up the SNI hostname and returns the
   matching `CertifiedKey`

The custom resolver must handle the case where no matching certificate exists
for the SNI hostname — in this case, the handshake fails, which is the correct
behavior (we don't serve a default certificate for unknown domains).

## HTTP Listener (Port 80)

Each listener has its own HTTP listener on port 80 (or the configured
`http_port`). It is a plain TCP listener with no TLS. It has one job: redirect
all requests to the HTTPS equivalent.

Each HTTP listener binds to the same IP address as its corresponding TLS
listener, but on port 80.

### ACME Challenge Type

The default ACME challenge type is **TLS-ALPN-01**, since the proxy already
listens on port 443. This avoids requiring a separate HTTP-01 challenge server.
HTTP-01 is available as a fallback for environments where TLS-ALPN-01 is not
suitable (e.g., behind a CDN that terminates TLS). When using HTTP-01, the
port 80 listener serves `/.well-known/acme-challenge/{token}` paths for
challenge verification.

## Certificate Failure Behavior

ACME certificate provisioning and renewal can fail for various reasons (network
outages, Let's Encrypt unavailability, DNS issues, rate limiting). The proxy's
behavior depends on the scenario:

| Scenario | Behavior |
|----------|----------|
| First start, no cached cert, ACME unreachable | **Fail to start** with clear error message. The proxy cannot serve TLS without a certificate. |
| First start, no cached cert, ACME succeeds | Normal startup. Certificate is provisioned and cached. |
| Start with cached cert, ACME unreachable for renewal | **Start normally** with cached cert. Log error at `warn` level. `rustls-acme` retries per its built-in schedule. |
| Renewal failure after startup | **Continue serving existing cert**. Log error at `warn` level. `rustls-acme` retries per its built-in schedule. |
| Cached cert expired, renewal fails at startup | **Fail to start** if cert is expired at startup. An expired certificate cannot serve valid TLS. |
| Cached cert expires during runtime | **Continue serving expired cert**. Clients will receive certificate errors. Log at `error` level. This is the correct behavior — silently dropping TLS would be worse. |

The key principle: **never start without a valid TLS certificate**, but **always
continue serving if a valid cert exists**, even if renewal fails.

## TLS Error Handling

TLS handshake failures are logged and the connection is closed. The proxy does
not serve a default certificate for unknown hostnames — connections that don't
match any configured certificate fail.

| Scenario | Behavior |
|----------|----------|
| SNI hostname doesn't match any certificate (manual mode) | TLS handshake fails with `unrecognized_name` alert. Log at `warn` level with client IP and SNI hostname. |
| No SNI extension sent by client | TLS handshake fails with `handshake_failure` alert. Log at `warn` level with client IP. |
| Unsupported TLS version (1.0/1.1) | TLS handshake fails with `protocol_version` alert. Log at `info` level. |
| Cipher suite negotiation fails | TLS handshake fails with `handshake_failure` alert. Log at `info` level with client IP. |
| Certificate expired (manual mode) | Connection fails during TLS handshake. Log at `error` level. Other listeners/connections continue serving. |

In ACME mode, the `ResolvesServerCertAcme` resolver handles certificate
selection automatically — there is no SNI mismatch scenario because the
resolver serves the ACME-provisioned certificate for all valid domains.

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
| [012](decisions/012-cipher-suite-restriction.md) | Restrict cipher suites | Match nginx scope: four ECDHE-AES-GCM suites for TLS 1.2, all TLS 1.3 suites |
| [019](decisions/019-multi-config-listeners.md) | Multi-config listeners | `[[listeners]]` supporting both dedicated-IP and shared-IP deployment models |
| [023](decisions/023-http2-client-facing.md) | HTTP/2 client-facing support | ALPN-based protocol detection; `h2` and `http/1.1` advertised |

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). Key
questions affecting this document:

- ~~**OQ-01**: Should cipher suites be restricted beyond rustls defaults?~~ (resolved — ADR-012: restrict to nginx scope)
- ~~**OQ-07**: Should per-site TLS overrides be supported for mixed ACME/manual domains?~~ (resolved — ADR-019: `[[listeners]]` with per-listener TLS config)