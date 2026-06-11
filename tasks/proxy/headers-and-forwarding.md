---
id: proxy/headers-and-forwarding
name: Implement proxy header injection, hop-by-hop removal, and request forwarding with hyper Client
status: completed
depends_on: [proxy/host-routing]
scope: moderate
risk: medium
impact: component
level: implementation
---

## Description

Implement the core reverse proxy logic: inject proxy headers, remove hop-by-hop headers, and forward requests to the upstream via a shared `hyper::Client`.

### Proxy Header Injection

The proxy is an **edge proxy** — it sits directly in front of the internet with no trusted proxies upstream. This means existing `X-Forwarded-For` headers from the client cannot be trusted.

| Header | Value Source | Behavior |
|--------|-------------|----------|
| `Host` | Original request `Host` header | Preserved as-is |
| `X-Real-IP` | `ConnectInfo<SocketAddr>` remote IP | Set to client's IP address |
| `X-Forwarded-For` | `ConnectInfo<SocketAddr>` remote IP | **Replaced**, not appended |
| `X-Forwarded-Proto` | Determined by listener port | `https` for `https_port`, `http` for `http_port` |

### Hop-by-Hop Header Removal

Remove these headers before forwarding to upstream (RFC 2616 §13.5.1):
- `Connection`, `Keep-Alive`, `Proxy-Authorization`, `Proxy-Authenticate`
- `TE`, `Trailers`, `Transfer-Encoding`, `Upgrade`

Also remove these from upstream responses before sending to client.

### Request Forwarding

1. Build the upstream URI: `{upstream_scheme}://{upstream}{path}?{query}`
2. Copy request method, headers (with proxy headers injected, hop-by-hop removed), and body
3. Send via shared `hyper::Client` with per-site timeout overrides
4. Stream response back to client (chunk-by-chunk, not buffered)
5. Handle client disconnect (log at debug, close upstream connection)
6. Handle upstream disconnect (send whatever was already sent, close connection)

### hyper Client Configuration

- Created once at startup, shared via axum State
- HTTP/1.1 only for upstream connections
- No redirect following (proxies should not follow redirects)
- Connection pooling (hyper default behavior)
- Per-site timeout overrides: `upstream_connect_timeout_secs` (default 5s), `upstream_request_timeout_secs` (default 60s)

### Upstream Scheme

Default is `http://`. When `upstream_scheme` is `"https"`, validate the upstream's TLS certificate using the system's native TLS root certificates. Certificate validation failures result in `502 Bad Gateway`.

## Acceptance Criteria

- [ ] `X-Real-IP` set from `ConnectInfo<SocketAddr>` remote IP
- [ ] `X-Forwarded-For` **replaced** (not appended) with client IP
- [ ] `X-Forwarded-Proto` set to `https` or `http` based on listener port
- [ ] `Host` header preserved as-is
- [ ] Hop-by-hop headers removed before forwarding to upstream
- [ ] Hop-by-hop headers removed from upstream response before sending to client
- [ ] No `Server` header added to responses
- [ ] No `Via` header added in Phase 1
- [ ] Request body streamed (not buffered) to upstream
- [ ] Response body streamed (not buffered) to client
- [ ] Client disconnect logged at debug level, upstream connection closed
- [ ] Upstream disconnect: client receives whatever was already sent
- [ ] Per-site timeout overrides applied to hyper Client requests
- [ ] `upstream_scheme: "https"` validates upstream TLS certificate with system roots
- [ ] Shared `hyper::Client` instance via axum State
- [ ] Unit tests for header injection and removal
- [ ] Integration test: proxy request to upstream, verify headers and response

## References

- docs/architecture/proxy.md — header injection, request forwarding, error handling
- docs/architecture/decisions/002-custom-proxy-handler.md — custom handler rationale
- docs/architecture/decisions/017-upstream-connection-defaults.md — HTTP/1.1, no redirects
- docs/architecture/decisions/021-x-forwarded-for-edge-proxy.md — edge proxy model

## Notes

> The `X-Forwarded-For: replace, don't append` behavior is critical. The proxy is the edge — there are no trusted proxies upstream. Existing `X-Forwarded-For` values from the client could be spoofed and must not be trusted.

## Summary

> To be filled on completion