---
id: ops/signals-and-shutdown
name: Implement signal handling (SIGTERM/SIGINT/SIGHUP) and graceful shutdown sequence
status: pending
depends_on: [config/dynamic-config, ops/admin-socket]
scope: moderate
risk: medium
impact: component
level: implementation
---

## Description

Implement signal handling for SIGTERM, SIGINT, and SIGHUP, plus the graceful shutdown sequence.

### Signal Handling

Using `signal-hook` crate (per ADR-009):

- **SIGTERM / SIGINT**: Graceful shutdown
- **SIGHUP**: Config reload (same code path as admin socket `reload` command)

### Graceful Shutdown Sequence

On SIGTERM or SIGINT:

1. **Stop accepting new connections** — Close all TCP listening sockets
2. **Close idle keep-alive connections** — Send `Connection: close` on idle connections
3. **Wait for in-flight requests** — Up to `shutdown_timeout_secs` (default: 30)
4. **Force-close remaining connections** — After timeout, TCP RST
5. **Cancel background tasks** — ACME renewal, rate limiter eviction, admin socket
6. **Exit with code 0**

### SIGHUP for Config Reload

SIGHUP triggers the same config reload as the admin socket `reload` command:

1. Re-read the config file from disk
2. Deserialize into full config (static + dynamic)
3. Validate the full config
4. If valid: swap DynamicConfig, log warnings for any static changes
5. If invalid: reject reload, log error, keep old DynamicConfig

SIGHUP provides no feedback on success or failure — it just logs. The admin socket is the programmatic alternative with structured responses.

### Shutdown Timeout

Configurable via `shutdown_timeout_secs` in StaticConfig (default: 30 seconds).

## Acceptance Criteria

- [ ] `signal-hook` handles SIGTERM, SIGINT, SIGHUP
- [ ] SIGTERM/SIGINT triggers graceful shutdown sequence
- [ ] SIGHUP triggers config reload (same code path as admin socket)
- [ ] Graceful shutdown: close listening sockets first
- [ ] Graceful shutdown: close idle keep-alive connections
- [ ] Graceful shutdown: wait for in-flight requests up to timeout
- [ ] Graceful shutdown: force-close remaining connections after timeout
- [ ] Cancel background tasks (ACME, eviction, admin socket) on shutdown
- [ ] Exit code 0 on graceful shutdown
- [ ] `shutdown_timeout_secs` configurable in StaticConfig
- [ ] SIGHUP reload converges on same code path as admin socket reload
- [ ] Integration test: send SIGTERM, verify graceful shutdown sequence

## References

- docs/architecture/operations.md — signal handling, shutdown sequence
- docs/architecture/decisions/009-signal-handling.md — signal handling strategy

## Notes

> The shutdown sequence must be carefully ordered. Closing listening sockets before waiting for in-flight requests ensures no new connections arrive while existing ones drain.

## Summary

> To be filled on completion