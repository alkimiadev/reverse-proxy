---
id: ops/health-check
name: Implement health check endpoint on separate local port and HTTPS fallback
status: complete
depends_on: [config/static-config]
scope: narrow
risk: low
impact: component
level: implementation
---

## Description

Implement the health check endpoint on a separate local port (default: 9900, bound to `127.0.0.1` only) and as a fallback on the HTTPS listener.

### Local Health Check Port

- Binds to `127.0.0.1:{health_check_port}`
- `GET /health` returns `200 OK` with empty body
- `health_check_port = 0` disables the separate listener
- Port must not conflict with any listener's `http_port` or `https_port` on `127.0.0.1` (validated in config validation)

### HTTPS Health Check Fallback

When the local health check port is enabled, `/health` is also available on the HTTPS listener(s) for TLS-level health verification. External monitoring should prefer the local health check for liveness and can use the HTTPS endpoint for TLS verification.

### What Health Check Verifies

- Process is running and tokio runtime is responsive
- TLS listener is accepting connections (HTTPS endpoint only)
- Config is loaded (StaticConfig and DynamicConfig are initialized)

It does **NOT** check upstream reachability. The health check answers "is the proxy process healthy?", not "is the upstream reachable?"

## Acceptance Criteria

- [ ] Local health check binds to `127.0.0.1:{health_check_port}` only
- [ ] `GET /health` returns `200 OK` with empty body
- [ ] `health_check_port = 0` disables the listener
- [ ] Port conflict detection in config validation
- [ ] `/health` available on HTTPS listener(s) as fallback
- [ ] Health check does not verify upstream reachability
- [ ] Integration test: local health check responds 200
- [ ] Integration test: HTTPS health check responds 200

## References

- docs/architecture/operations.md — health check section
- docs/architecture/decisions/013-health-check-port.md — separate local port rationale

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion