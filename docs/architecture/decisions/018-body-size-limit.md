# ADR-018: Request Body Size Limit

## Status

Accepted

## Context

The proxy enforces a maximum request body size to protect against resource
exhaustion attacks. The default limit must balance security with usability.

Gitea push operations can involve large Git pack files. The current nginx
configuration uses `client_max_body_size 100m`, and Gitea's documentation
recommends allowing up to 100 MB for push operations.

## Decision

Set the default request body size limit to 100 MB (104,857,600 bytes),
matching our current nginx configuration. The limit is global in Phase 1
(configurable via `body.limit_bytes` in DynamicConfig).

## Rationale

- 100 MB matches the current nginx `client_max_body_size 100m`, ensuring
  behavioral parity during migration
- Gitea push operations with large repositories regularly exceed 50 MB
- 100 MB is large enough for any legitimate Git operation while still
  providing protection against resource exhaustion (a 100 MB body is not
  enough to exhaust memory on modern servers, but prevents unbounded uploads)
- The limit is configurable — operators can reduce it for deployments that
  don't need large uploads
- In Phase 2, per-site limits will allow different limits for different
  upstreams (e.g., a lower limit for alk.dev, the current limit for
  git.alk.dev)

## Consequences

**Positive:**
- Behavioral parity with current nginx configuration
- Gitea push operations work without configuration changes
- Configurable for deployments with different needs

**Negative:**
- 100 MB is a generous default — some deployments may want a lower limit
  (mitigated by configurability)
- Global limit means all sites share the same maximum (mitigated by Phase 2
  per-site limits)

## References

- [proxy.md](../proxy.md)
- [config.md](../config.md)
- nginx `client_max_body_size` documentation