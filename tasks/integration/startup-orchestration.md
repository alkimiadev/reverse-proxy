---
id: integration/startup-orchestration
name: Wire startup sequence with all components and sd_notify readiness signaling
status: complete
depends_on: [config/cli-parsing, config/validation, config/dynamic-config, tls/tls-listener-setup, tls/http-redirect, proxy/host-routing, proxy/headers-and-forwarding, proxy/error-responses, ops/rate-limiting, ops/logging, ops/health-check, ops/admin-socket, ops/signals-and-shutdown, ops/body-size-limit]
scope: broad
risk: high
impact: project
level: implementation
---

## Description

Wire together all components in the correct startup sequence from operations.md. This is the `main.rs` that orchestrates everything.

### Startup Sequence (per operations.md)

1. **Parse and validate config** — CLI args, TOML deserialization, validation
2. **Initialize DynamicConfig** — Load into `ArcSwap<DynamicConfig>`
3. **Initialize shared state** — Rate limiter HashMap, shared hyper Client, tracing subscriber
4. **Bind health check port** (if enabled) — Fail-fast if bind fails
5. **Bind admin socket** (if enabled) — Remove stale socket, warn if occupied
6. **Bind all listener ports** — HTTP and HTTPS for each listener. Fail-fast if any bind fails
7. **Load TLS configuration** — Manual certs or ACME init. Fail-fast on error
8. **Start TCP listeners** — Begin accepting connections on all bound ports
9. **Start background tasks** — ACME renewal, rate limiter eviction, signal handler, admin socket handler
10. **Signal readiness** — `sd_notify("READY=1")` if running under systemd

### Component Wiring

- **axum Router per listener**: Each listener gets its own Router with its own middleware stack
- **Shared State**: `Arc<ArcSwap<DynamicConfig>>`, `Arc<Mutex<HashMap<IpAddr, TokenBucket>>>`, `Arc<hyper::Client>` shared via axum State
- **Middleware order**: Rate limiting → Body size limit → Proxy header injection → Host routing → Proxy handler
- **Health endpoint**: On both the local health check port and the HTTPS listener(s)

### Fail-Fast Behavior

If any step in the startup sequence fails, the process exits with a non-zero code. The proxy does not partially start. All ports are bound before any connections are accepted.

## Acceptance Criteria

- [ ] Startup sequence follows the exact order from operations.md
- [ ] Fail-fast behavior: exit with non-zero code on any startup failure
- [ ] All ports bound before any connections accepted
- [ ] Per-listener axum Router with shared State
- [ ] Middleware stack in correct order: rate limiting → body limit → headers → routing → proxy
- [ ] `ConnectInfo<SocketAddr>` propagated to routers for `X-Real-IP`
- [ ] `sd_notify("READY=1")` sent after all listeners started
- [ ] Graceful shutdown on SIGTERM/SIGINT
- [ ] SIGHUP triggers config reload
- [ ] Admin socket accepts `reload` and `status` commands
- [ ] Health check endpoint responds on local port and HTTPS
- [ ] Integration test: full proxy startup with test config, verify all endpoints work
- [ ] Integration test: config reload via SIGHUP updates routing
- [ ] Integration test: config reload via admin socket updates routing with feedback

## References

- docs/architecture/operations.md — startup sequence, component wiring
- docs/architecture/overview.md — architecture diagram
- docs/architecture/config.md — StaticConfig, DynamicConfig, reload flow
- docs/architecture/proxy.md — middleware stack, request flow

## Notes

> This is the critical integration task. All other implementation tasks must be complete before this one can start. The implementation agent should follow the startup sequence from operations.md precisely and ensure fail-fast behavior at every step.

## Summary

> To be filled on completion