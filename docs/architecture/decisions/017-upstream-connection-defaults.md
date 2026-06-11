# ADR-017: Upstream Connection Defaults

## Status

Accepted

## Context

The proxy forwards requests to upstream services using a shared hyper `Client`.
The client's configuration affects all upstream connections: timeouts, redirect
handling, and connection pooling. These are significant design choices that
affect production behavior and should be traceable.

## Decision

Configure the hyper `Client` with the following defaults:

| Setting | Value | Rationale |
|---------|-------|-----------|
| Connect timeout | 5 seconds | Prevents hanging on unreachable upstreams |
| Request timeout | 60 seconds (default) | Per-site override available (ADR-015) |
| Redirect following | Disabled | Proxies should not follow redirects — they pass response status to the client |
| Connection pooling | Enabled (hyper default) | Reuses connections to the same upstream for performance |
| HTTP version | HTTP/1.1 only | Upstream connections use HTTP/1.1; HTTP/2 proxying is out of scope |

The connect timeout of 5 seconds applies to establishing a TCP connection to
the upstream. The request timeout of 60 seconds applies to the full
request/response cycle and can be overridden per-site (ADR-015).

## Rationale

- **5s connect timeout**: Matches common reverse proxy conventions. Long
  enough for remote upstreams with network latency, short enough to fail fast
  on unreachable services.
- **60s request timeout**: Generous default that accommodates Gitea push
  operations with large pack files. Per-site overrides allow tuning for faster
  upstreams.
- **No redirect following**: Standard reverse proxy behavior. If an upstream
  returns a 3xx, the proxy streams it to the client. Following redirects would
  break the client's expectation and could create security issues (redirect
  loops, credential forwarding).
- **Connection pooling**: Hyper's default connection reuse improves performance
  for repeated requests to the same upstream without additional configuration.
- **HTTP/1.1 only**: The proxy communicates with upstreams over plain HTTP
  (loopback). HTTP/2 proxying is explicitly out of scope (see overview.md).
  Upstreams needing HTTP/2+ run their own servers (e.g., api.alk.dev).

## Consequences

**Positive:**
- Explicit, documented defaults that match reverse proxy conventions
- Per-site timeout overrides available for specific needs
- No redirect following prevents common proxy pitfalls
- Connection pooling provides good performance by default

**Negative:**
- 60s default may be too long for fast upstreams (mitigated by per-site
  overrides in ADR-015)
- HTTP/1.1 only for upstream means no HTTP/2 multiplexing to backends (out of
  scope per project design)

## References

- [proxy.md](../proxy.md)
- ADR-015 (per-site upstream timeouts)
- ADR-002 (custom proxy handler)