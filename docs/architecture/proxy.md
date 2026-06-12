---
status: draft
last_updated: 2026-06-12
---

# Proxy Handler

## What It Is

The proxy handler is the core component that receives an incoming HTTP request
on the TLS-terminated connection, applies middleware (rate limiting, header
injection, body size limits), and forwards it to the upstream service.

## Why It Exists

This component replaces nginx's `proxy_pass` directive. For our use case —
one upstream per domain across multiple domains, no load balancing, no HTTP/2
proxying — a custom handler is simpler and more maintainable than a
general-purpose proxy library (ADR-002, ADR-010).

## Architecture

```
Incoming HTTPS request
        │
        ▼
┌─────────────────┐
│  axum Router     │
│  (Host-based)    │
│                  │
│  match Host      │
│  header on       │
│  incoming req    │
└───────┬─────────┘
        │
        ▼
┌─────────────────┐
│ Rate Limiting    │  ← tower middleware layer
│ Middleware        │
└───────┬─────────┘
        │
        ▼
┌─────────────────┐
│ Proxy Header     │  ← custom middleware / handler
│ Injection        │
│                  │
│ X-Real-IP        │  ← connect_info remote_addr
│ X-Forwarded-For  │  ← append to existing or set
│ X-Forwarded-Proto │  ← "https" (or "http" on port 80)
│ Host             │  ← original host header (already set)
└───────┬─────────┘
        │
        ▼
┌─────────────────┐
│ Body Size Limit  │  ← DefaultBodyLimit(100 MB)
│ Middleware        │
└───────┬─────────┘
        │
        ▼
┌─────────────────┐
│ Reverse Proxy    │  ← hyper Client request forwarding
│ Handler          │
│                  │
│ 1. Build upstream│
│    URI from      │
│    original req   │
│ 2. Forward req   │
│    to upstream    │
│ 3. Stream        │
│    response back  │
└─────────────────┘
```

## Request Flow

### 1. Host-Based Routing

The axum router uses a `Host` extractor to match incoming requests to site
definitions from `DynamicConfig`. Sites are defined per-listener in the TOML
configuration for organizational purposes, but at runtime they are collected
into a single global routing table. The proxy looks up the `Host` header in
this global table and either proxies to the upstream or returns 404.

Host matching is **case-insensitive** per RFC 7230 §2.7.3. The `Host` header
is normalized to lowercase before matching. Site `host` values in
configuration are normalized to lowercase during validation.

The `Host` header port component (e.g., `git.alk.dev:443`) is stripped before
matching. Site `host` values must not include ports.

The proxy does not filter or restrict paths. All paths and query strings on a
known host are forwarded to the upstream without modification.

The proxy does **not** serve a `/health` route on the main listener. Health
checking is an operational concern handled by the dedicated local health check
port (default: 9900, bound to `127.0.0.1` only) and the admin socket's `status`
command — not by intercepting traffic on the public-facing proxy. See ADR-013
and ADR-022.

### 2. Proxy Header Injection

Headers are injected before forwarding. The proxy is an **edge proxy** — it
sits directly in front of the internet with no trusted proxies upstream. This
means the client IP from `ConnectInfo<SocketAddr>` is the real client IP, and
existing `X-Forwarded-For` headers from the client cannot be trusted.

| Header | Value Source | Notes |
|--------|-------------|-------|
| `Host` | Original request `Host` header | Preserved as-is |
| `X-Real-IP` | `ConnectInfo<SocketAddr>` remote IP | Set to client's IP address |
| `X-Forwarded-For` | `ConnectInfo<SocketAddr>` remote IP | **Replaced**, not appended. The proxy is the edge proxy — there are no trusted proxies upstream, so existing `X-Forwarded-For` values from the client cannot be trusted. |
| `X-Forwarded-Proto` | Determined by which listener port received the request | `https` for requests on the listener's `https_port`, `http` for requests on the listener's `http_port`. Note: since the TLS-terminating listener only receives HTTPS connections, this is always `"https"` in practice. The HTTP redirect listener sends a 301 redirect rather than proxying, so `X-Forwarded-Proto` is not set there. See OQ-11. |

**ConnectInfo propagation**: `ConnectInfo<SocketAddr>` is populated by
extracting `TcpStream::peer_addr()` before wrapping the connection in
`TlsStream`. Each listener provides this information to its axum Router via
`axum::ServiceExt::into_make_service_with_connect_info::<SocketAddr>()`.

### 3. Request Forwarding

The proxy handler constructs a new request to the upstream:

1. Build the upstream URI using the site's `upstream_scheme` and `upstream`
   address, preserving the original path and query string
2. Copy the request method, headers, and body from the original
3. Inject proxy headers (X-Real-IP, X-Forwarded-For, X-Forwarded-Proto)
4. Send the request via a shared hyper Client instance
5. Stream the response back to the client (chunk-by-chunk, not buffered)

   If the client disconnects while the upstream is still sending, the upstream
   connection is closed and the event is logged at `debug` level. If the
   upstream disconnects mid-stream, the client receives whatever data was
   already sent and the connection is closed.

The hyper Client is created once at startup and shared via axum's `State`. It
must be configured with (see ADR-017 for rationale):
- Connection pooling (hyper default behavior)
- HTTP/1.1 only for upstream connections (HTTP/2 proxying is out of scope)
- No redirect following (proxies should not follow redirects)

Per-site timeout overrides are available via `upstream_connect_timeout_secs`
and `upstream_request_timeout_secs` in `SiteConfig` (see ADR-015). When not
specified, defaults of 5s connect and 60s request are used.

### 4. Header Handling

The proxy must handle request and response headers correctly to avoid security
issues and protocol violations.

**Headers removed before forwarding (hop-by-hop headers per RFC 2616 §13.5.1):**

- `Connection`
- `Keep-Alive`
- `Proxy-Authorization`
- `Proxy-Authenticate`
- `TE`
- `Trailers`
- `Transfer-Encoding`
- `Upgrade`

These headers are connection-specific and must not be forwarded to the
upstream. Removing `Proxy-Authorization` and `Proxy-Authenticate` prevents
credential leakage.

**Headers added or modified:**

See the Proxy Header Injection section above for the full list of proxy headers
(X-Real-IP, X-Forwarded-For, X-Forwarded-Proto, Host).

**Headers NOT added in Phase 1:**

- `Via`: Not added. The proxy is an edge proxy and `Via` is primarily for
  tracking proxy chains. Can be added in Phase 2 if needed.

**Response headers:**

Upstream response headers are forwarded as-is to the client, with the following
exceptions:
- Hop-by-hop headers listed above are removed
- The proxy does not add a `Server` header to responses

### 5. Error Handling

All error responses use plain text bodies with no proxy version or identity
information. No upstream error details are included. Response format:

- Content-Type: `text/plain; charset=utf-8`
- Body: Brief status text matching the HTTP status (e.g., `Bad Gateway` for 502)

| Upstream Condition | Response | Body | Notes |
|-------------------|----------|------|-------|
| Upstream reachable | Stream response as-is | (upstream body) | Headers, status, body all forwarded |
| Upstream unreachable | 502 Bad Gateway | `Bad Gateway` | Logged at `warn` level |
| Upstream timeout | 504 Gateway Timeout | `Gateway Timeout` | Logged at `warn` level |
| Request body too large | 413 Payload Too Large | `Payload Too Large` | From `DefaultBodyLimit` middleware |
| Rate limit exceeded | 429 Too Many Requests | `Too Many Requests` | Logged at `info` level |
| Unknown Host header | 404 Not Found | `Not Found` | No matching site definition |
| Missing Host header | 400 Bad Request | `Bad Request` | Required for routing |

### 6. HTTP → HTTPS Redirect

A separate HTTP listener on port 80 (per listener) handles redirect. It reads
the `Host` header from the incoming request and returns a 301 Permanent Redirect
to the HTTPS equivalent URL.

The redirect URL is constructed as:
`https://{host}:{https_port}/{path}?{query}`

Where:
- `{host}` is the hostname portion of the `Host` header (port stripped)
- `{https_port}` is the listener's `https_port`, omitted if it's 443
- `{path}` and `{query}` are preserved from the original request

If the incoming request has no `Host` header, the proxy returns `400 Bad
Request`.

Each listener has its own HTTP redirect on its own bind address.

## Upstream Connection

The upstream connection scheme defaults to `http://` since the proxy and backend
services typically run on the same host (e.g., `127.0.0.1:3000`). The
`upstream_scheme` field in each site's configuration allows specifying `https://`
for upstreams that require TLS (e.g., separate hosts or secure internal services).

For the initial deployment, upstream connections use plain HTTP (e.g.,
`git.alk.dev` → `127.0.0.1:3000`, `alk.dev` → `127.0.0.1:8080`) since TLS
between the proxy and backend services on loopback is unnecessary.

When `upstream_scheme` is `"https"`, the proxy validates the upstream's TLS
certificate using the system's native TLS root certificates (via `rustls` root
cert store). Certificate validation failures result in a 502 Bad Gateway
response. No certificate pinning or custom CA support is provided in Phase 1.

## Body Size Limit

axum's `DefaultBodyLimit` layer sets the maximum request body size. The default
of 100 MB (104,857,600 bytes) matches our current nginx configuration and
accommodates Gitea's push operations with large pack files (see ADR-018). In
Phase 1, the body limit is a global setting; Phase 2 may add per-site body
limits.

## Design Decisions

All design decisions are documented as ADRs in [decisions/](decisions/).

| ADR | Decision | Summary |
|-----|----------|---------|
| [002](decisions/002-custom-proxy-handler.md) | Custom proxy handler | One upstream per domain — simpler than a general proxy library |
| [007](decisions/007-custom-log-format.md) | Custom structured log format | key=value pairs with RATE_LIMIT prefix for fail2ban |
| [010](decisions/010-multi-site-phase1.md) | Multi-site in Phase 1 | Multiple domains from initial release |
| [015](decisions/015-per-site-timeouts.md) | Per-site upstream timeouts with defaults | 5s connect / 60s request defaults, per-site overrides |
| [017](decisions/017-upstream-connection-defaults.md) | Upstream connection defaults | HTTP/1.1, no redirects, connection pooling |
| [018](decisions/018-body-size-limit.md) | Request body size limit | 100 MB default matching nginx, Gitea push compatibility |
| [021](decisions/021-x-forwarded-for-edge-proxy.md) | X-Forwarded-For edge proxy model | Replace, don't append — proxy is the edge, no trusted upstream proxies |

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). All
questions affecting this document have been resolved:

- ~~**OQ-06**: Should upstream timeouts be configurable per-site?~~ (resolved —
  ADR-015: per-site timeout overrides with defaults)
- ~~**OQ-08**: Should the `/health` path use a less common endpoint to avoid
  upstream collision?~~ (resolved — ADR-022: no `/health` route on the main
  listener; health checking is via port 9900 and admin socket only)