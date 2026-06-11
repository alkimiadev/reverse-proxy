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

### OQ-07: Should per-site TLS overrides be supported for mixed ACME/manual domains?

- **Origin**: [tls.md](tls.md), [config.md](config.md)
- **Status**: open
- **Priority**: low
- **Context**: Phase 1 uses a single TLS configuration (ACME or manual) for all
  domains. All domains share the same ACME config and certificate. If a future
  domain needs a manual certificate (e.g., a corporate CA cert) while other
  domains use ACME, a per-site TLS override would be needed. This would require
  a custom `ResolvesServerCert` that combines ACME-provisioned certs with
  manually loaded certs. For now, all proxied domains use the same ACME config,
  so this is not needed.
- **Cross-references**: ADR-011

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
- **Resolution**: A single `bind_addr` is sufficient. The proxy binds to one
  explicit IP address (not `0.0.0.0`). Multi-address binding is not needed for
  this single-server deployment. If needed in the future, `bind_addr` could be
  extended to an array. See config.md for the `bind_addr` field.
- **Cross-references**: ADR-016

## Proxy

### ~~OQ-06: Should upstream timeouts be configurable per-site?~~

- **Origin**: [proxy.md](proxy.md)
- **Status**: resolved
- **Priority**: low
- **Resolution**: Yes. Per-site upstream timeouts with sensible defaults (5s
  connect, 60s request). Optional fields in SiteConfig that override global
  defaults when specified. See ADR-015.
- **Cross-references**: ADR-015, ADR-017