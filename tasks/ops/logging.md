---
id: ops/logging
name: Implement structured logging with tracing, file output, and fail2ban-compatible format
status: pending
depends_on: [setup/project-init]
scope: moderate
risk: low
impact: component
level: implementation
---

## Description

Implement structured logging using `tracing` and `tracing-subscriber` with dual output (file + stdout) and fail2ban-compatible log format.

### Log Types

1. **Access logs** (every proxied request, `info` level):
   ```
   REQUEST client_ip=203.0.113.50 host=git.alk.dev method=GET path=/user/repo status=200 upstream=127.0.0.1:3000 duration_ms=45
   ```

2. **Event logs** (rate limits, TLS errors, upstream failures, config reloads):
   ```
   RATE_LIMIT client_ip=203.0.113.50 host=git.alk.dev path=/login status=429
   UPSTREAM_ERROR host=git.alk.dev upstream=127.0.0.1:3000 error="connection refused"
   CONFIG_RELOAD status=success sites=1
   ```

### Dual Output

- **File** (primary): Written to `log_file_path` when configured. This is the authoritative source for fail2ban.
- **stdout/stderr** (always-on): For `docker logs`, `journalctl`, and development.

Use `tracing-subscriber` `Layer` composition to write to both simultaneously.

### Log Levels

| Level | Use |
|-------|-----|
| `error` | Unrecoverable failures (TLS handshake failure, config validation) |
| `warn` | Rate limit exceeded, upstream unreachable, upstream timeout |
| `info` | Access logs, config reloads, ACME events, startup/shutdown |
| `debug` | Request/response headers, connection details |
| `trace` | Detailed protocol-level information |

Configurable via `log_level` in StaticConfig.

### Configuration

- `logging.level`: Log verbosity (default: `"info"`)
- `logging.format`: `"text"` or `"json"` (default: `"text"`)
- `logging.log_file_path`: Optional file path; when set, logs are written to this file in addition to stdout

### File Logging and fail2ban

File logging is the primary integration point for fail2ban. In container deployments, the log directory is volume-mounted so fail2ban on the host can read it directly.

A corresponding fail2ban filter definition and jail configuration will be provided in the deployment task.

## Acceptance Criteria

- [ ] `tracing` and `tracing-subscriber` initialized with dual output (file + stdout)
- [ ] File output enabled when `log_file_path` is configured
- [ ] Stdout output always enabled
- [ ] Custom event format with `key=value` pairs
- [ ] `REQUEST` prefix for access logs
- [ ] `RATE_LIMIT` prefix for rate limit events
- [ ] `UPSTREAM_ERROR` prefix for upstream failures
- [ ] `CONFIG_RELOAD` prefix for config reload events
- [ ] Log level configurable via `logging.level`
- [ ] JSON format output when `logging.format = "json"`
- [ ] Text format output when `logging.format = "text"` (default)
- [ ] `duration_ms` field in access logs for response time
- [ ] Unit tests for log format output

## References

- docs/architecture/operations.md — logging section, log format
- docs/architecture/decisions/007-custom-log-format.md — custom log format rationale
- docs/architecture/decisions/020-container-deployment.md — file-primary logging rationale

## Notes

> The fail2ban filter and jail configuration are a separate deployment task. This task focuses on producing the correct log format.

## Summary

> To be filled on completion