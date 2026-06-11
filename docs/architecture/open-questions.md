---
status: draft
last_updated: 2026-06-11
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
  The main HTTPS `/health` endpoint remains available as a fallback. See
  ADR-013.
- **Cross-references**: ADR-013

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

### OQ-08: Should the `/health` path use a less common endpoint to avoid upstream collision?

- **Origin**: Implementation review finding W5, [proxy.md](proxy.md)
- **Status**: open
- **Priority**: medium
- **Resolution**: None yet. The proxy currently intercepts `GET /health` on all
  hosts before host-based routing, which means any upstream application that
  uses `/health` for its own health checks will have those requests silently
  intercepted. Options: (1) Use a less common path like `/__health` or
  `/healthz`; (2) Only intercept `/health` when the Host header doesn't match
  any known site (fallthrough); (3) Make the health check path configurable
  via `StaticConfig`. Option 1 is simplest for Phase 1. Option 3 is most
  flexible long-term. The architecture spec (proxy.md, ADR-013) currently
  specifies `/health` as a top-level route regardless of Host.
- **Cross-references**: ADR-013

### OQ-09: How should `upstream_connect_timeout_secs` be enforced?

- **Origin**: Implementation review finding W4, ADR-015, ADR-017
- **Status**: open
- **Priority**: medium
- **Resolution**: None yet. The architecture (ADR-015, ADR-017) specifies a
  5-second default connect timeout separate from the request timeout, and
  `SiteConfig` includes `upstream_connect_timeout_secs`. However, the
  implementation only applies `upstream_request_timeout_secs` as a blanket
  timeout covering the entire exchange. The hyper client handles TCP connect
  internally, making a two-phase timeout harder to implement without custom
  connect logic. Need to decide: (1) implement a two-phase timeout using
  `tokio::time::timeout` for connect phase then request phase; (2) configure
  the hyper client's `connect_timeout` parameter; or (3) accept the current
  behavior for Phase 1 and add connect timeout enforcement in Phase 2.
- **Cross-references**: ADR-015, ADR-017

## Configuration

### OQ-10: Should ACME contact email be a required config field?

- **Origin**: Implementation review finding C2, [tls.md](tls.md), [config.md](config.md)
- **Status**: open
- **Priority**: high
- **Resolution**: None yet. Let's Encrypt requires a contact email for production
  certificate requests. The current architecture spec does not include an
  `acme_contact` field in `TlsConfig` or `ListenerConfig`. Without it, ACME
  registration with Let's Encrypt production will fail. Options: (1) Add a
  required `acme_contact` field to the TLS config within each `[[listeners]]`
  entry that uses ACME mode; (2) Add a global `acme_contact` field shared
  across all ACME listeners. Per-listener is more flexible but adds config
  noise. Global is simpler for typical deployments. Need to update config.md
  and tls.md.
- **Cross-references**: ADR-004

### OQ-11: How should `X-Forwarded-Proto` be derived per-listener?

- **Origin**: Implementation review finding W14, [proxy.md](proxy.md)
- **Status**: open
- **Priority**: medium
- **Resolution**: None yet. The architecture spec (proxy.md) states
  `X-Forwarded-Proto` should be "determined by which listener port received the
  request" — `https` for requests on the listener's `https_port`, `http` for
  requests on the listener's `http_port`. The implementation hardcodes
  `is_https: true` in `ProxyState`. For a TLS-terminating reverse proxy this
  is correct (all TLS connections arrive on the HTTPS port), but the HTTP
  redirect listener should set `X-Forwarded-Proto: https` since it redirects to
  HTTPS. Need to clarify: (1) The HTTPS listener always sets `X-Forwarded-Proto:
  https` (correct, since it terminates TLS); (2) The HTTP redirect listener
  sends a 301 redirect and does NOT proxy, so `X-Forwarded-Proto` on the
  redirect response is not applicable. The hardcoded behavior is correct but
  should be documented.
- **Cross-references**: ADR-021

## Operations

### OQ-12: Should request access logging be mandatory or optional?

- **Origin**: Implementation review finding W13, [operations.md](operations.md)
- **Status**: open
- **Priority**: high
- **Resolution**: None yet. The architecture spec (operations.md) defines an
  access log format (`REQUEST client_ip=... host=... method=... path=...
  status=... upstream=... duration_ms=...`) and a `log_request!` macro, but
  the implementation does not emit access logs. Without request-level logging,
  the proxy is operationally blind — there is no observability into traffic,
  response codes, or upstream latency. This also blocks fail2ban integration
  for access-log-based jails. The question is whether to: (1) Make access
  logging mandatory (always-on at `info` level); (2) Make it configurable
  (e.g., `access_log` boolean in `LoggingConfig`); or (3) Tie it to the
  existing `log_file_path` setting. The architecture spec implies it's always
  on.
- **Cross-references**: ADR-007