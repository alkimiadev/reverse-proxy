# ADR-013: Health Check on Separate Local Port

## Status

Accepted

## Context

The health check endpoint (`/health`) needs to be accessible for monitoring
without requiring TLS. Currently the design places it on the main HTTPS
listener, which means:

1. TLS handshake must succeed for the health check to respond
2. External monitoring tools need to handle TLS
3. A TLS configuration error would make the health check unreachable, creating
   a false-negative monitoring signal

Three options were considered (see OQ-03):

1. **Main HTTPS listener only**: Simplest, but TLS config errors make health
   checks unreachable
2. **Separate unencrypted port on localhost**: Simple, works with standard
   monitoring tools, but health checks bypass TLS
3. **Admin port with its own listener**: Most flexible but adds complexity

## Decision

Add a configurable health check port that binds to `127.0.0.1` only (localhost),
serving `/health` over plain HTTP. This is a separate listener from the main
HTTP and HTTPS listeners.

The port is configurable via `health_check_port` in StaticConfig. The default
value is `9900` (enabled, localhost only). Setting it to `0` disables the
separate health check listener, and `/health` remains available on the main
HTTPS listener as a fallback.

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
- Disabling via `health_check_port = 0` keeps the main HTTPS `/health` endpoint
  available for cases where a separate port isn't needed
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
- Two health check endpoints exist when the separate port is enabled (the
  local one and the HTTPS one) — monitoring should prefer the local one

## References

- [operations.md](../operations.md)
- OQ-03 (now resolved)