# ADR-013: Health Check on Separate Local Port

## Status

Accepted

## Context

The health check endpoint (`/health`) needs to be accessible for monitoring
without requiring TLS. Serving it on the main HTTPS listener would mean:

1. TLS handshake must succeed for the health check to respond
2. External monitoring tools need to handle TLS
3. A TLS configuration error would make the health check unreachable, creating
   a false-negative monitoring signal
4. It creates collision with upstream applications that use `/health` for their
   own health checks (see ADR-022)

Three options were considered (see OQ-03):

1. **Separate unencrypted port on localhost (chosen)**: Simple, works with
   standard monitoring tools, health checks work even when TLS is misconfigured
2. **Main HTTPS listener only**: Would require TLS for health checks, creating
   a circular dependency — TLS config errors would make health checks unreachable
3. **Admin port with its own listener**: Most flexible but adds complexity
   beyond what's needed for a simple health check

## Decision

Add a configurable health check port that binds to `127.0.0.1` only (localhost),
serving `/health` over plain HTTP. This is a separate listener from the main
HTTP and HTTPS listeners.

The port is configurable via `health_check_port` in StaticConfig. The default
value is `9900` (enabled, localhost only). Setting it to `0` disables the
health check listener entirely — there is no `/health` route on the main HTTPS
listener (see ADR-022).

## Rationale

- A local-only health check port is the standard pattern for reverse proxies
  and service meshes (envoy, haproxy, k8s health probes all use this pattern)
- Health checks should work even when TLS is misconfigured — that's the whole
  point of monitoring
- Binding to `127.0.0.1` only means the health check is not exposed to the
  internet — only local monitoring tools (systemd, scripts, load balancers on
  the same host) can reach it
- Configurable port allows different deployment scenarios (some monitoring runs
  on different ports)
- Disabling via `health_check_port = 0` removes the health check entirely —
   the admin HTTP endpoint's `/admin/status` (with Bearer token) remains available
   as an alternative health/status mechanism (ADR-028)
- When this project is folded into alknet, the health check will use alknet's
  existing patterns, making the separate port unnecessary in that context

## Consequences

**Positive:**
- Health checks work even when TLS is misconfigured
- Standard pattern that monitoring tools expect
- Not exposed to the internet (localhost only)
- Configurable — can be disabled if not needed
- systemd can use it for `NotifyAccess` readiness checks

**Negative:**
- Additional listener to manage (minimal complexity)

## References

- [operations.md](../operations.md)
- [ADR-022](022-health-check-scope.md) — Health check scope (no `/health` on main listener)
- [ADR-028](028-admin-http-api.md) — Authenticated HTTP admin API (admin endpoints on health check port)
- OQ-03 (now resolved)