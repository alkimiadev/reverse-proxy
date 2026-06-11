---
status: draft
last_updated: 2026-06-11
---

# Open Questions

## TLS

### OQ-01: Should cipher suites be restricted beyond rustls defaults?

- **Origin**: [tls.md](tls.md)
- **Status**: open
- **Priority**: medium
- **Context**: Our current nginx config explicitly restricts cipher suites to
  four ECDHE-AES-GCM suites. rustls 0.23 with `aws_lc_rs` defaults to a
  conservative set that excludes all weak ciphers (no SHA-1, no 3DES, no RC4,
  no CBC-mode suites, no RSA key exchange). The defaults include TLS 1.3 suites
  which nginx also allows. Restricting further would reduce compatibility with
  older clients; not restricting means accepting a wider (but still safe) set
  than the current nginx config.
- **Cross-references**: ADR-005

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

### OQ-03: Should the health check endpoint be on a separate port?

- **Origin**: [operations.md](operations.md)
- **Status**: open
- **Priority**: low
- **Context**: Currently the health check is on the main HTTPS listener at
  `/health`. Alternatives: (a) separate unencrypted port for health checks
  (simpler for load balancers but less secure), (b) admin port with its own
  listener (more complex but isolates operational traffic), (c) on the main
  listener (simplest, proposed approach). For a single-server deployment behind
  no external load balancer, the main listener is fine.
- **Cross-references**: None

## Configuration

### OQ-04: Should config reload support a Unix domain socket API in addition to SIGHUP?

- **Origin**: [config.md](config.md)
- **Status**: open
- **Priority**: low
- **Context**: Phase 1 uses SIGHUP for config reload, which is simple and proven.
  A Unix domain socket API would allow programmatic reload (e.g., from an admin
  tool or CI/CD pipeline) and could return success/failure status. This adds
  complexity and is not needed for Phase 1.
- **Cross-references**: None

## Deployment

### ~~OQ-05: Should the proxy bind to multiple addresses or just one?~~

- **Origin**: [overview.md](overview.md)
- **Status**: resolved
- **Priority**: low
- **Resolution**: A single `bind_addr` is sufficient. The proxy binds to one
  explicit IP address (not `0.0.0.0`). Multi-address binding is not needed for
  this single-server deployment. If needed in the future, `bind_addr` could be
  extended to an array. See config.md for the `bind_addr` field.
- **Cross-references**: None

## Proxy

### OQ-06: Should upstream timeouts be configurable per-site?

- **Origin**: [proxy.md](proxy.md)
- **Status**: open
- **Priority**: low
- **Context**: Phase 1 uses global defaults (5s connect timeout, 60s request
  timeout) for all upstream connections. Per-site timeout configuration would
  allow tuning for different upstream services (e.g., a slow database-backed
  API vs. a fast static site). Not needed for Phase 1 with a single upstream.
- **Cross-references**: None