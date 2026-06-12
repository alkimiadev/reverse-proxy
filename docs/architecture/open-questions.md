---
status: draft
last_updated: 2026-06-12
---

# Open Questions

## TLS

### ~~OQ-01: Should cipher suites be restricted beyond rustls defaults?~~

- **Origin**: [tls.md](tls.md)
- **Status**: resolved
- **Priority**: medium
- **Resolution**: Restrict cipher suites to match the nginx scope: four
  ECDHE-AES-GCM suites for TLS 1.2 plus all TLS 1.3 suites. This provides
  behavioral parity during migration. See ADR-012.
- **Cross-references**: ADR-005, ADR-012

### ~~OQ-02: What log format should fail2ban consume?~~

- **Origin**: [operations.md](operations.md), [proxy.md](proxy.md)
- **Status**: resolved
- **Priority**: high
- **Resolution**: Custom structured log format with `key=value` pairs and
  `RATE_LIMIT` prefix. A corresponding custom fail2ban filter will be provided.
  See ADR-007.
- **Cross-references**: ADR-007

### ~~OQ-07: Should per-site TLS overrides be supported for mixed ACME/manual domains?~~

- **Origin**: [tls.md](tls.md), [config.md](config.md)
- **Status**: resolved
- **Priority**: low
- **Resolution**: Resolved by introducing `[[listeners]]` configuration. Each
  listener is an independent TLS endpoint with its own bind address, TLS config,
  and site routing. This supports both deployment models: (1) shared-IP
  multi-domain (one listener, SAN certificate, SNI routing) and (2) dedicated-IP
  single-domain (multiple listeners, each with its own IP/cert/domain). Mixed
  ACME/manual configurations are naturally supported since each listener has its
  own TLS mode. See ADR-019.
- **Cross-references**: ADR-011, ADR-019

## Logging and Monitoring

### ~~OQ-03: Should the health check endpoint be on a separate port?~~

- **Origin**: [operations.md](operations.md)
- **Status**: resolved
- **Priority**: low
- **Resolution**: Add a configurable local health check port (default: 9900)
  bound to `127.0.0.1` only. Health checks work even when TLS is misconfigured.
  There is no `/health` route on the main HTTPS listener — health checking is
  handled exclusively by the local port and admin socket. See ADR-013 and
  ADR-022.
- **Cross-references**: ADR-013, ADR-022

## Configuration

### ~~OQ-04: Should config reload support a Unix domain socket API in addition to SIGHUP?~~

- **Origin**: [config.md](config.md)
- **Status**: resolved
- **Priority**: low
- **Resolution**: Yes. Add a Unix domain socket admin API alongside SIGHUP.
  The socket accepts a `reload` command and returns structured success/failure
  responses. SIGHUP is retained as a fallback. See ADR-014.
- **Cross-references**: ADR-014

## Deployment

### ~~OQ-05: Should the proxy bind to multiple addresses or just one?~~

- **Origin**: [overview.md](overview.md)
- **Status**: resolved
- **Priority**: low
- **Resolution**: A single `bind_addr` per listener entry is sufficient. ADR-019
  introduced `[[listeners]]`, where each listener has its own `bind_addr`. This
  supports multiple bind addresses in a single process — one per listener —
  without needing an array of addresses on a single listener. See ADR-016 and
  ADR-019.
- **Cross-references**: ADR-016, ADR-019

## Proxy

### ~~OQ-06: Should upstream timeouts be configurable per-site?~~

- **Origin**: [proxy.md](proxy.md)
- **Status**: resolved
- **Priority**: low
- **Resolution**: Resolved by ADR-015. Per-site upstream timeout overrides with
  sensible defaults (5s connect, 60s request). Optional fields in SiteConfig
  that override global defaults when specified.
- **Cross-references**: ADR-015, ADR-017

### ~~OQ-08: Should the `/health` path use a less common endpoint to avoid upstream collision?~~

- **Origin**: Implementation review finding W5, [proxy.md](proxy.md)
- **Status**: resolved
- **Priority**: medium
- **Resolution**: The `/health` route does not belong on the main listener at
  all. Health checking is an operational concern served by the dedicated local
  port (9900) and the admin socket's `status` command — not by intercepting
  traffic on the public-facing proxy. Serving `/health` on the main listener
  creates collision with upstream applications, requires special-case routing
  logic before host-based matching, and is architecturally wrong: the main
  listener's job is to proxy requests, not to serve operational endpoints. The
  local health check port (bound to `127.0.0.1:9900`) and the admin socket are
  the sole health/status mechanisms. See ADR-022.
- **Cross-references**: ADR-013, ADR-022

### ~~OQ-09: How should `upstream_connect_timeout_secs` be enforced?~~

- **Origin**: Implementation review finding W4, ADR-015, ADR-017, Security
  Review C3
- **Status**: resolved
- **Priority**: medium
- **Resolution**: Implemented using a two-phase `tokio::time::timeout` approach.
  The inner timeout uses the per-site `upstream_connect_timeout_secs` (default
  5s) for the connect + first-byte phase, and the outer timeout uses
  `upstream_request_timeout_secs` (default 60s) for the full request/response
  cycle. The shared `HttpConnector` uses a 30-second connect timeout ceiling
  via `set_connect_timeout()` — this is a safety backstop, not the primary
  enforcement mechanism. The per-site `tokio::time::timeout` enforces the
  actual connect timeout. This ensures per-site values >5s work correctly
  (previously the hardcoded 5s connector timeout silently capped them). See
  ADR-026.
- **Cross-references**: ADR-015, ADR-017, ADR-026

### ~~OQ-10: Should ACME contact email be a required config field?~~

- **Origin**: Implementation review finding C2, [tls.md](tls.md), [config.md](config.md)
- **Status**: resolved
- **Priority**: high
- **Resolution**: This is not an open question — the architecture already
  specifies `acme_contact` as a required field in ACME mode (config.md
  validation rule 19). The field is defined in the `ListenerConfig` table and
  shown in TOML examples. Let's Encrypt requires a contact email for production
  certificate requests. The implementation bug (C2: `contact: vec![]`) has been
  fixed — `acme_contact` is now correctly wired from config to the ACME state
  machine. No new ADR needed — the decision is already documented in config.md
  and tls.md.
- **Cross-references**: ADR-004

### ~~OQ-11: How should `X-Forwarded-Proto` be derived per-listener?~~

- **Origin**: Implementation review finding W14, [proxy.md](proxy.md)
- **Status**: resolved
- **Priority**: medium
- **Resolution**: The hardcoded `is_https: true` behavior is correct for a
  TLS-terminating reverse proxy. The proxy only proxies requests on the HTTPS
  listener, which always sets `X-Forwarded-Proto: https`. The HTTP redirect
  listener sends a 301 redirect and does NOT proxy requests, so
  `X-Forwarded-Proto` is not set there. This behavior is correct and matches
  the architecture spec (proxy.md). The implementation should add a comment
  documenting this rationale to prevent future "fixes" that would change the
  behavior. No ADR or spec change needed — just a code comment.
- **Cross-references**: ADR-021

## Operations

### ~~OQ-12: Should request access logging be mandatory or optional?~~

- **Origin**: Implementation review finding W13, [operations.md](operations.md)
- **Status**: resolved
- **Priority**: high
- **Resolution**: Access logging is mandatory and always-on at `info` level.
  The architecture spec (operations.md) already states: "Access logging is
  **always-on** — it is the primary observability mechanism for the proxy and
  is required for fail2ban integration. There is no configuration option to
  disable access logging." The `log_request!` macro exists in the codebase
  but is not called — this is an implementation gap (W13), not an
  architectural question. No ADR needed; ADR-007 already covers the log format.
- **Cross-references**: ADR-007

## Configuration

### OQ-13: Should `acme_contact` support multiple email addresses?

- **Origin**: Security Review S9, [config.md](config.md), [tls.md](tls.md)
- **Status**: open
- **Priority**: low
- **Details**: `acme_contact` is currently a single `String`, but ACME supports
  multiple contact emails. The `AcmeTlsConfig.contact` field in the
  implementation is already `Vec<String>`, and the single config value is
  wrapped in `vec![...]`. Changing `acme_contact` to `Vec<String>` in the
  config schema would provide consistency with the ACME protocol. However,
  this is a config format change that requires migration documentation and
  backward compatibility considerations. For Phase 1, a single email is
  sufficient.
- **Cross-references**: ADR-004

### OQ-14: Should rate limiter eviction interval and max age be configurable?

- **Origin**: Security Review S2, [operations.md](operations.md)
- **Status**: open
- **Priority**: low
- **Details**: The eviction task interval (60s) and max age (300s) are
  currently hardcoded. In high-traffic deployments, a shorter interval or
  longer max age might be desirable. These would be dynamic config fields
  (hot-reloadable via ArcSwap) if added. For Phase 1, the hardcoded values
  are reasonable defaults.
- **Cross-references**: ADR-006