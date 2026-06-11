# ADR-005: tokio-rustls Directly, Not axum-server

## Status

Accepted

## Context

We need to serve HTTPS (TLS) traffic through axum. Two approaches exist for
integrating TLS with axum:

1. **`axum-server`**: A wrapper that provides TLS support for axum via
   `tls_rustls` feature. Handles TCP binding, TLS accept, and passing TLS
   streams to axum. Simple API but limited control over the TLS configuration.

2. **`tokio-rustls` directly**: Bind TCP manually, perform TLS handshake with
   `TlsAcceptor`, then serve the TLS stream to axum/hyper. More code but full
   control over `ServerConfig`, cipher suites, ALPN protocols, and cert
   resolvers.

The alknet project uses tokio-rustls directly and has proven this pattern for
both manual and ACME certificate management.

## Decision

Use `tokio-rustls` directly for TLS termination, with `hyper` serving the
resulting TLS streams to axum. Do not use `axum-server`.

## Rationale

- **ACME integration**: The `rustls-acme` `ResolvesServerCertAcme` resolver
  needs to be set as the certificate resolver on `ServerConfig` via
  `with_cert_resolver()`. `axum-server` does not expose this level of control
  over the `ServerConfig`.
- **Cipher suite control**: We may need to configure cipher suites beyond the
  defaults (see OQ-01). `axum-server` wraps the `ServerConfig` construction
  and may not expose `CryptoProvider` configuration. Direct `tokio-rustls`
  usage gives us full control.
- **ALPN configuration**: ACME TLS-ALPN-01 challenge requires adding
  `acme-tls/1` to the ALPN protocol list. This is only possible with direct
  `ServerConfig` access.
- **Proven pattern**: alknet uses exactly this approach (`TlsAcceptor` wrapping
  `tokio-rustls`, with manual or ACME `ServerConfig` construction).
- **No abstraction cost**: The code to bind TCP, accept TLS, and serve to
  axum/hyper is ~50 lines. `axum-server` saves little for our simple case.

## Consequences

**Positive:**
- Full control over TLS configuration
- Direct `rustls-acme` integration
- Ability to add ALPN protocols for ACME challenges
- Proven pattern from alknet

**Negative:**
- Slightly more code than `axum-server` (~50 lines for the TLS acceptor loop)
- Need to manage the TCP listener and TLS accept explicitly
- Must handle the `TlsStream<TcpStream>` → `hyper::service_fn` → axum
  integration manually (well-documented pattern from Felix Knorr's blog and
  alknet)

## References

- [tls.md](../tls.md)
- alknet transport layer (`alknet-core/src/transport/tls.rs`, `alknet-core/src/transport/acme.rs`)