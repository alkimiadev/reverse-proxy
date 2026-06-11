---
status: draft
last_updated: 2026-06-11
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
│  (Host-based)    │─── /health → 200 OK
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
definitions from `DynamicConfig`. Each site definition maps a hostname to an
upstream address.

Where `host_based_proxy` reads the `Host` header, looks up the site in
`DynamicConfig.sites`, and either proxies to the upstream or returns 404.

### 2. Proxy Header Injection

Headers are injected before forwarding. The handler reads connection metadata
from axum's `ConnectInfo` and the original request:

| Header | Value Source | Notes |
|--------|-------------|-------|
| `Host` | Original request `Host` header | Already present; preserved as-is |
| `X-Real-IP` | `ConnectInfo<SocketAddr>` remote IP | Set to client's IP address |
| `X-Forwarded-For` | Client IP, appended if header exists | Comma-separated list of proxies |
| `X-Forwarded-Proto` | Determined by listener | `https` on port 443, `http` on port 80 |

The `X-Forwarded-For` handling must append the client IP to any existing value
(rather than replacing it), to support chained proxies correctly.

### 3. Request Forwarding

The proxy handler constructs a new request to the upstream:

1. Build the upstream URI using the site's `upstream_scheme` and `upstream`
   address, preserving the original path and query string
2. Copy the request method, headers, and body from the original
3. Inject proxy headers (X-Real-IP, X-Forwarded-For, X-Forwarded-Proto)
4. Send the request via a shared hyper Client instance
5. Stream the response back to the client

The hyper Client is created once at startup and shared via axum's `State`. It
must be configured with (see ADR-017 for rationale):
- Connection pooling (hyper default behavior)
- HTTP/1.1 only for upstream connections (HTTP/2 proxying is out of scope)
- No redirect following (proxies should not follow redirects)

Per-site timeout overrides are available via `upstream_connect_timeout_secs`
and `upstream_request_timeout_secs` in `SiteConfig` (see ADR-015). When not
specified, defaults of 5s connect and 60s request are used.

### 4. Error Handling

| Upstream Condition | Response | Notes |
|-------------------|----------|-------|
| Upstream reachable | Stream response as-is | Headers, status, body all forwarded |
| Upstream unreachable | 502 Bad Gateway | Logged at `warn` level |
| Upstream timeout | 504 Gateway Timeout | Logged at `warn` level |
| Request body too large | 413 Payload Too Large | From `DefaultBodyLimit` middleware |
| Rate limit exceeded | 429 Too Many Requests | Logged at `info` level |
| Unknown Host header | 404 Not Found | No matching site definition |

### 5. HTTP → HTTPS Redirect

A separate HTTP listener on port 80 (per listener) handles redirect. It reads
the `Host` header from the incoming request and returns a 301 Permanent Redirect
to the HTTPS equivalent URL (preserving the path and query string).

Each listener has its own HTTP redirect on its own bind address.

## Upstream Connection

The upstream connection scheme defaults to `http://` since the proxy and backend
services typically run on the same host (e.g., `127.0.0.1:3000`). The
`upstream_scheme` field in each site's configuration allows specifying `https://`
for upstreams that require TLS (e.g., separate hosts or secure internal services).

For the initial deployment, upstream connections use plain HTTP (e.g.,
`git.alk.dev` → `127.0.0.1:3000`, `alk.dev` → `127.0.0.1:8080`) since TLS
between the proxy and backend services on loopback is unnecessary.

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

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). Key
questions affecting this document:

- ~~**OQ-06**: Should upstream timeouts be configurable per-site?~~ (resolved —
  ADR-015: per-site timeout overrides with defaults)