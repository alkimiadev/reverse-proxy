# ADR-023: HTTP/2 Client-Facing Support

## Status

Accepted

## Context

The original architecture spec excluded HTTP/2 proxying from scope, stating "HTTP/2
or HTTP/3 proxying (services that need these run their own native Rust servers)."
This was interpreted as excluding HTTP/2 entirely — both for client connections
and upstream connections.

During deployment testing, we discovered that modern browsers and HTTP clients
negotiate HTTP/2 via ALPN during the TLS handshake. The initial implementation
used `hyper_util::server::conn::auto::Builder` which failed to properly detect
HTTP/2 over TLS connections because its `ReadVersion` mechanism doesn't work
reliably with `tokio-rustls` `TlsStream` wrappers.

This caused two problems:
1. HTTP/2 clients received degraded performance (no multiplexing) or connection
   failures
2. In HTTP/2, the host is conveyed via the `:authority` pseudo-header, which
   hyper represents as the URI host rather than a `Host` header — causing 400
   errors for HTTP/2 clients

## Decision

The proxy now supports HTTP/2 on the **client-facing** side (between the client
and the proxy). This is distinct from HTTP/2 proxying to upstream services,
which remains out of scope.

**Implementation:**

1. **ALPN-based protocol detection**: After the TLS handshake, the proxy reads
   the negotiated ALPN protocol from `tls_stream.get_ref().1.alpn_protocol()`.
   If the ALPN is `h2`, the connection uses
   `hyper::server::conn::http2::Builder`; otherwise, it uses
   `hyper_util::server::conn::auto::Builder` with HTTP/1.1 + upgrade support.

2. **Host header fallback**: The proxy handler now falls back to
   `req.uri().host()` when the `Host` header is absent. In HTTP/2, the
   `:authority` pseudo-header is represented as the URI host in hyper, so this
   correctly handles both HTTP/1.1 (where `Host` is always present) and HTTP/2
   (where `:authority` maps to URI host).

3. **ALPN advertisement**: The TLS `ServerConfig` advertises `h2` and
   `http/1.1` as ALPN protocols, plus `acme-tls/1` for ACME challenges.

**Upstream connections remain HTTP/1.1.** The proxy communicates with upstream
services over HTTP/1.1 (or HTTPS/1.1 when `upstream_scheme = "https"`). HTTP/2
to upstreams is out of scope for Phase 1.

## Consequences

**Positive:**
- Modern browsers and HTTP/2 clients work correctly with the proxy
- HTTP/2 multiplexing improves client-facing performance (multiple requests over
  a single connection)
- ALPN-based detection is the standard mechanism for HTTP/2 negotiation over TLS
- Host header fallback correctly handles both HTTP/1.1 and HTTP/2

**Negative:**
- Slightly more complex TLS listener code (ALPN protocol detection, dual
  builder paths)
- The distinction between "HTTP/2 to the proxy" and "HTTP/2 to upstream" must
  be clearly documented to avoid confusion
- `ConnectInfoService` is typed to `Request<Incoming>` rather than the generic
  `Request<B>`, which is a correct but slightly less flexible implementation

## References

- [proxy.md](../proxy.md) — request flow and host-based routing
- [tls.md](../tls.md) — TLS termination and ALPN configuration
- [overview.md](../overview.md) — scope and out-of-scope items