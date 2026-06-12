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
Incoming HTTPS request (HTTP/1.1 or HTTP/2)
        │
        ▼
┌─────────────────────────────────────────────┐
│  TLS Listener                                │
│  ALPN protocol detection:                    │
│  - h2  → hyper http2::Builder                │
│  - http/1.1 (or none) → auto::Builder        │
│  ConnectInfo<SocketAddr> from peer_addr       │
└───────┬──────────────────────────────────────┘
        │
        ▼
┌─────────────────┐
│  axum Router     │
│  (Host-based)    │
│                  │
│  match Host      │
│  header or       │
│  URI :authority  │
│  on incoming req │
└───────┬─────────┘
        │
        ▼
┌─────────────────┐
│ Rate Limiting    │  ← tower middleware layer
│ Middleware        │  ← IP from ConnectInfo only (ADR-025)
└───────┬─────────┘
        │
        ▼
┌─────────────────┐
│ Proxy Header     │  ← custom middleware / handler
│ Injection        │
│                  │
│ X-Real-IP        │  ← connect_info remote_addr
│ X-Forwarded-For  │  ← replace (edge proxy model)
│ X-Forwarded-Proto │  ← "https" (always, on TLS listener)
│ Host             │  ← original host (already set)
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
│    (HTTP/1.1)    │
│ 3. Stream        │
│    response back  │
└─────────────────┘
```

## Request Flow

### 1. Host-Based Routing

The axum router matches incoming requests to site definitions from
`DynamicConfig`. Sites are defined per-listener in the TOML configuration for
organizational purposes, but at runtime they are collected into a single global
routing table. The proxy looks up the host in this global table and either
proxies to the upstream or returns 404.

Host matching is **case-insensitive** per RFC 7230 §2.7.3. The host is
normalized to lowercase before matching. Site `host` values in configuration are
normalized to lowercase during validation.

The `Host` header port component (e.g., `git.alk.dev:443`) is stripped before
matching. Site `host` values must not include ports.

**HTTP/2 host resolution**: In HTTP/2, the host is conveyed via the
`:authority` pseudo-header rather than the `Host` header. Hyper represents this
as the URI host. The proxy handler resolves the host by first checking the
`Host` header, then falling back to `req.uri().host()`. This correctly handles
both HTTP/1.1 (which always has a `Host` header) and HTTP/2 (which uses
`:authority`/URI host). If neither is present, the proxy returns 400 Bad
Request. See ADR-023.

The proxy does not filter or restrict paths. All paths and query strings on a
known host are forwarded to the upstream without modification.

The proxy does **not** serve a `/health` route on the main listener. Health
checking is an operational concern handled by the dedicated local health check
port (default: 9900, bound to `127.0.0.1` only) and the admin socket's `status`
command — not by intercepting traffic on the public-facing proxy. See ADR-013
and ADR-022.

### 2. Rate Limiter IP Source

The rate limiting middleware runs **before** the proxy handler. At that point,
no proxy headers have been injected — any `X-Forwarded-For` header present is
from the client and is untrusted. The rate limiter must use
`ConnectInfo<SocketAddr>` as the **sole** source of client IP addresses.
Client-supplied `X-Forwarded-For` headers must not be consulted for rate
limiting. See ADR-025.

`ConnectInfo<SocketAddr>` is always present because each listener populates it
via `into_make_service_with_connect_info::<SocketAddr>()`. If `ConnectInfo`
is absent, the request must be rejected rather than falling back to an
untrusted header.

### 3. Proxy Header Injection

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

### 4. Request Forwarding

The proxy handler constructs a new request to the upstream:

1. Build the upstream URI using the site's `upstream_scheme` and `upstream`
   address, preserving the original path and query string. **If URI
   construction fails** (e.g., the resulting URI is malformed), the proxy must
   return 502 Bad Gateway and log the error at `warn` level. The proxy must
   never silently drop parts of the URI (such as the query string) — a
   malformed upstream URI is an error, not a recoverable condition.
2. Copy the request method, headers, and body from the original
3. Inject proxy headers (X-Real-IP, X-Forwarded-For, X-Forwarded-Proto)
4. Remove hop-by-hop headers (Connection, Keep-Alive, Transfer-Encoding, etc.)
5. Send the request via a shared hyper Client instance
6. Stream the response back to the client (chunk-by-chunk, not buffered)

   If the client disconnects while the upstream is still sending, the upstream
   connection is closed and the event is logged at `debug` level. If the
   upstream disconnects mid-stream, the client receives whatever data was
   already sent and the connection is closed.

The hyper Client is created once at startup and shared via axum's `State`. It
must be configured with (see ADR-017 for rationale):
- Connection pooling (hyper default behavior)
- HTTP/1.1 only for upstream connections (HTTP/2 proxying to upstreams is out
  of scope; see ADR-023 for the distinction between client-facing HTTP/2 and
  upstream HTTP/2)
- No redirect following (proxies should not follow redirects)
- Separate connect timeout and request timeout (see ADR-015, ADR-017)

Two client instances are created at startup:
- **HTTP client**: For upstream connections using `http://` scheme
- **HTTPS client**: For upstream connections using `https://` scheme (using
  `hyper-rustls` with system native TLS root certificates for certificate
  validation)

Per-site timeout overrides are available via `upstream_connect_timeout_secs`
and `upstream_request_timeout_secs` in `SiteConfig` (see ADR-015). When not
specified, defaults of 5s connect and 60s request are used. Both timeouts are
enforced using `tokio::time::timeout`, with the connect timeout nested inside
the request timeout to ensure the overall deadline is respected.

### 5. Header Handling

The proxy must handle request and response headers correctly to avoid security
issues and protocol violations.

**Headers removed before forwarding (hop-by-hop headers per RFC 7230 §6.1):**

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

**Response headers removed:**

- `Server`: The upstream's `Server` header is intentionally removed as a
  defense-in-depth measure. The proxy does not add its own `Server` header
  either. This hides upstream server identity from clients.

**Headers added or modified:**

See the Proxy Header Injection section above for the full list of proxy headers
(X-Real-IP, X-Forwarded-For, X-Forwarded-Proto, Host).

**Headers NOT added in Phase 1:**

- `Via`: Not added. The proxy is an edge proxy and `Via` is primarily for
  tracking proxy chains. Can be added in Phase 2 if needed.

**Response headers:**

Upstream response headers are forwarded to the client with the following
exceptions:
- Hop-by-hop headers listed above are removed
- The `Server` header is removed (defense-in-depth: hiding upstream identity)
- The proxy does not add a `Server` header to responses

### 6. Error Handling

All error responses use plain text bodies with no proxy version or identity
information. No upstream error details are included. Response format:

- Content-Type: `text/plain; charset=utf-8`
- Body: Brief status text matching the HTTP status (e.g., `Bad Gateway` for 502)

| Upstream Condition | Response | Body | Notes |
|-------------------|----------|------|-------|
| Upstream reachable | Stream response as-is | (upstream body) | Headers, status, body all forwarded (minus hop-by-hop and Server headers) |
| Upstream unreachable | 502 Bad Gateway | `Bad Gateway` | Logged at `warn` level |
| Upstream connect timeout | 504 Gateway Timeout | `Gateway Timeout` | Connect phase timed out; logged at `warn` level |
| Upstream request timeout | 504 Gateway Timeout | `Gateway Timeout` | Full request timed out; logged at `warn` level |
| Upstream TLS validation failure | 502 Bad Gateway | `Bad Gateway` | Upstream HTTPS cert validation failed |
| Request body too large | 413 Payload Too Large | `Payload Too Large` | From `DefaultBodyLimit` middleware |
| Rate limit exceeded | 429 Too Many Requests | `Too Many Requests` | Logged at `info` level |
| Unknown Host header | 404 Not Found | `Not Found` | No matching site definition |
| Missing Host header (and no URI host) | 400 Bad Request | `Bad Request` | Required for routing; HTTP/2 clients use `:authority` |

### 7. HTTP → HTTPS Redirect

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
services typically run on the same host (e.g., `127.0.0.1:3000`) or the same
Docker network (e.g., `gitea:3000`). The `upstream_scheme` field in each site's
configuration allows specifying `https://` for upstreams that require TLS
(e.g., separate hosts or secure internal services).

For the initial deployment, upstream connections use plain HTTP (e.g.,
`git.alk.dev` → `gitea:3000`, `alk.dev` → `app:8080`) since TLS between the
proxy and backend services on the same Docker network or loopback is
unnecessary.

When `upstream_scheme` is `"https"`, the proxy validates the upstream's TLS
certificate using the system's native TLS root certificates (via `rustls` root
cert store loaded by `rustls-native-certs`). Certificate validation failures
result in a 502 Bad Gateway response. No certificate pinning or custom CA
support is provided in Phase 1.

Two shared hyper Client instances handle upstream connections:
- **HTTP client** (`Client<HttpConnector, Body>`): For `http://` upstreams
- **HTTPS client** (`Client<HttpsConnector<HttpConnector>, Body>`): For
  `https://` upstreams, using `hyper-rustls` with system native certificates

Both clients use a shared `HttpConnector` with a connect timeout ceiling
(30 seconds) set via `HttpConnector::set_connect_timeout()`. This ceiling
ensures TCP connections cannot hang indefinitely even if the per-site
`tokio::time::timeout` wrapper fails. The per-site connect timeout (default
5s) is enforced by `tokio::time::timeout`, which fires at the correct
per-site threshold. The connector ceiling is a safety backstop, not the
primary enforcement mechanism. See ADR-026.

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
| [023](decisions/023-http2-client-facing.md) | HTTP/2 client-facing support | ALPN-based protocol detection; HTTP/2 to clients, HTTP/1.1 to upstreams |
| [025](decisions/025-rate-limiter-ip-source.md) | Rate limiter IP source | ConnectInfo only, never client-supplied X-Forwarded-For |
| [026](decisions/026-connector-timeout-ceiling.md) | Connector timeout ceiling | 30s ceiling on connector, per-site timeout via tokio::time::timeout |

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). Key
questions affecting this document:

- ~~**OQ-06**: Should upstream timeouts be configurable per-site?~~ (resolved —
  ADR-015: per-site timeout overrides with defaults)
- ~~**OQ-08**: Should the `/health` path use a less common endpoint to avoid
  upstream collision?~~ (resolved — ADR-022: no `/health` route on the main
  listener; health checking is via port 9900 and admin socket only)
- ~~**OQ-09**: How should `upstream_connect_timeout_secs` be enforced?~~
  (resolved — ADR-026: 30s connector ceiling, per-site timeout via
  `tokio::time::timeout`)
- **OQ-13**: Should `acme_contact` support multiple email addresses? (see
  [open-questions.md](open-questions.md))