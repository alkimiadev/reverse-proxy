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
- **Resolution**: Yes. Per-site upstream timeouts with sensible defaults (5s
  connect, 60s request). Optional fields in SiteConfig that override global
  defaults when specified. See ADR-015.
- **Cross-references**: ADR-015, ADR-017