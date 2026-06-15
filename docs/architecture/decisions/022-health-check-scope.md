# ADR-022: Health Check Scope — Local Port and Admin HTTP Endpoint Only

## Status

Accepted

## Context

The implementation served a `GET /health` route on the main HTTPS listener that
returned 200 OK regardless of the Host header. This route was evaluated before
host-based routing, meaning any upstream application using `/health` for its own
health checks would have those requests silently intercepted by the proxy and
never reach the upstream (implementation review finding W5).

The architecture already specified a separate local health check port (9900,
bound to 127.0.0.1 only) via ADR-013. The question was whether to keep the
main-listener `/health` route alongside the dedicated port (and possibly make
the path configurable), or to remove it entirely.

## Decision

The main HTTPS listener does **not** serve a `/health` route. Health checking is
handled exclusively by:

1. **Local health check port** (default: 9900, bound to `127.0.0.1`) — serves
   `GET /health → 200 OK`. This is the primary health check mechanism for
   container orchestration, load balancers, and monitoring systems.
 2. **Admin HTTP endpoint** (`GET /admin/status` with Bearer token) — returns
    process information including uptime and site count. See ADR-028.

The `/health` route is removed from the main listener entirely. No configurable
path is needed because the route simply does not exist on the public listener.

## Consequences

**Positive:**
- No collision with upstream applications that use `/health` for their own
  health checks
- The main listener's routing logic is simpler — all requests go through
  host-based routing, no special cases
- Clear separation of concerns: the main listener proxies, the local port
  answers health checks
- No configurable path needed — the problem disappears entirely

**Negative:**
- External monitoring that needs to verify TLS is working must connect to the
  HTTPS port directly and check for a successful TLS handshake or a 404
  response, rather than getting a 200 from `/health`. This is a minor
  inconvenience — any successful TLS response (even 404) confirms the proxy is
  serving TLS correctly.

## References

- ADR-013: Health check on separate local port
- ADR-028: Authenticated HTTP admin API (admin socket replaced by HTTP endpoint)
- OQ-08: Resolved by this ADR
- Implementation review finding W5 (hardcoded `/health` path)