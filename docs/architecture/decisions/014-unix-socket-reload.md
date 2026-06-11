# ADR-014: Unix Domain Socket Config Reload API

## Status

Accepted

## Context

The proxy supports config reload via SIGHUP (ADR-009). SIGHUP is simple and
well-understood, but has limitations:

1. No feedback — the sender doesn't know if the reload succeeded or failed
2. No structured input — you can only signal "reload", not specify which parts
   to reload or pass validation context
3. Requires process signal permissions — not all deployment tools can send
   signals

A Unix domain socket API would allow programmatic config reload with
success/failure status, enabling integration with CI/CD pipelines, admin
tools, and automated configuration management.

## Decision

Add a Unix domain socket API for config reload alongside SIGHUP. The socket
accepts commands and returns structured responses.

The socket path is configurable via `admin_socket_path` in StaticConfig
(default: `/run/reverse-proxy/admin.sock`). Setting it to empty string
disables the admin socket.

Initial commands:

- `reload` — Re-read config file, validate, and swap DynamicConfig. Returns
  `{"status": "ok"}` or `{"status": "error", "message": "..."}`.
- `status` — Return basic process info (uptime, config load time, site count).
  Returns `{"status": "ok", "uptime_secs": 1234, "sites": 2}`.

Future commands (not in Phase 1, but the protocol supports extension):

- `metrics` — Return Prometheus-compatible metrics
- `shutdown` — Graceful shutdown command

## Rationale

- Providing reload feedback is operationally valuable — CI/CD pipelines can
  verify config changes before proceeding
- The implementation cost is low — a Unix domain socket listener is ~50 lines
  of tokio code, and the command protocol is simple
- SIGHUP is retained as a fallback for environments where socket access is
  inconvenient
- This pattern will integrate naturally with alknet's admin interface when the
  projects merge
- Unix domain sockets are filesystem-permission-based, providing access control
  without additional authentication
- The socket path is configurable, allowing deployment-specific paths

## Consequences

**Positive:**
- Config reload with success/failure feedback
- Programmatic integration with CI/CD and admin tools
- Structured response format enables automation
- SIGHUP still works as fallback
- Natural path to future admin commands

**Negative:**
- Additional listener and command parsing logic (~100-150 lines)
- Socket file management (cleanup on startup, stale socket detection)
- One more config option (`admin_socket_path`)

## References

- [operations.md](../operations.md)
- [config.md](../config.md)
- ADR-009 (signal handling — SIGHUP retained as fallback)
- OQ-04 (now resolved)