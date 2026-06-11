---
status: draft
last_updated: 2026-06-11
---

# Operations

## What It Is

The operations component covers everything related to running the proxy in
production: rate limiting, logging (fail2ban integration), health checks,
systemd integration, and graceful shutdown.

## Why It Exists

A reverse proxy that can't be monitored, rate-limited, or gracefully restarted
is not production-ready. These concerns are cross-cutting — they affect the
proxy handler, the TLS layer, and the config system.

## Rate Limiting

### Requirements

- Limit requests per IP address (replacing nginx's `limit_req_zone`)
- Default: 10 requests/second with burst of 20 (matching current nginx config)
- Configurable via DynamicConfig (no restart needed)
- Must produce logs that fail2ban can consume

### Design

The rate limiter runs as axum middleware before the proxy handler. It uses a
token bucket algorithm per client IP, matching nginx's `limit_req burst`
semantics.

Rate limits are global per-IP in Phase 1 (not per-site). A request from IP
address X counts against the same bucket regardless of which site it targets.
Per-site rate limits may be added in Phase 2.

When a request exceeds the rate limit, the middleware returns `429 Too Many
Requests` and logs the event with structured fields.

### State Eviction

The per-IP token bucket state grows over time as new IPs are seen. A
background task runs every 60 seconds (configurable) and removes entries
whose last access timestamp is older than a configurable eviction age
(default: 300 seconds / 5 minutes). This prevents unbounded memory growth
while preserving recent entries that may still receive traffic.

### Fail2ban Integration

Rate limit events are logged in a structured format that a custom fail2ban
filter can parse. See [ADR-007](decisions/007-custom-log-format.md) for the
format decision.

The log format uses `key=value` pairs with a `RATE_LIMIT` prefix:

```
RATE_LIMIT client_ip=203.0.113.50 host=Y.Z path=/W status=429
```

A corresponding fail2ban filter and jail configuration are provided as part
of the deployment documentation.

## Logging

### Structure

All logs use `tracing` with structured fields. The proxy outputs two types of
log entries:

1. **Access logs**: Every proxied request is logged at `info` level with
   structured fields.

```
REQUEST client_ip=203.0.113.50 host=git.alk.dev method=GET path=/user/repo status=200 upstream=127.0.0.1:3000 duration_ms=45
```

2. **Event logs**: Rate limits, TLS errors, upstream failures, config reloads,
   etc.

   ```
   RATE_LIMIT client_ip=203.0.113.50 host=git.alk.dev path=/login status=429
   UPSTREAM_ERROR host=git.alk.dev upstream=127.0.0.1:3000 error="connection refused"
   CONFIG_RELOAD status=success sites=1
   ```

### Output

Logs are written to:
- **stdout/stderr**: For systemd/journald integration
- **File** (optional): For fail2ban consumption at
  `/var/log/reverse-proxy/access.log`

The `tracing-subscriber` layer configuration supports both simultaneously via
`Layer` composition.

### Log Levels

| Level | Use |
|-------|-----|
| `error` | Unrecoverable failures (TLS handshake failure, config validation) |
| `warn` | Rate limit exceeded, upstream unreachable, upstream timeout |
| `info` | Access logs, config reloads, ACME events, startup/shutdown |
| `debug` | Request/response headers, connection details |
| `trace` | Detailed protocol-level information |

Configurable via `log_level` in StaticConfig.

## Health Check

### Endpoint

```
GET /health → 200 OK (empty body)
```

The health check endpoint is accessible on the main HTTPS listener. It returns
200 if the process is alive and serving requests.

**Limitation**: Since `/health` is served over TLS, it cannot detect TLS
configuration errors that prevent the TLS handshake from completing. External
monitoring should also check TCP connectivity to port 443 independently.

### What It Checks

- Process is running and the tokio runtime is responsive
- TLS listener is accepting connections
- Config is loaded (StaticConfig and DynamicConfig are initialized)

It does **not** check upstream reachability. The health check answers "is the
proxy process healthy?", not "is the upstream reachable?" — upstream health is
a separate concern that would produce 502/504 responses in the proxy handler.

### Future Extensions

- `/health/ready` — readiness check that includes upstream reachability
- Prometheus metrics at `/metrics`

## Systemd Integration

### Unit File

```ini
[Unit]
Description=Reverse Proxy
After=network.target
Wants=network-online.target

[Service]
Type=notify
NotifyAccess=all
ExecStart=/usr/local/bin/reverse-proxy --config /etc/reverse-proxy/config.toml
Restart=on-failure
RestartSec=5

# Security hardening
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=/var/lib/reverse-proxy /var/log/reverse-proxy

# ACME challenge cache directory
StateDirectory=reverse-proxy

[Install]
WantedBy=multi-user.target
```

The proxy signals readiness to systemd via `sd_notify` after binding listeners
and completing the initial configuration load.

## Graceful Shutdown

### Signal Handling

The proxy handles three signals via `signal-hook` (see [ADR-009](decisions/009-signal-handling.md)):

- **SIGTERM / SIGINT**: Graceful shutdown. Stop accepting new connections, wait
  for in-flight requests to complete (up to a configurable timeout), then exit.
- **SIGHUP**: Config reload. Re-read the config file, validate, and swap
  DynamicConfig if valid.

### SIGHUP for Config Reload

SIGHUP triggers config reload (see [config.md](config.md) for details). The
process does not exit on SIGHUP.

### Timeout

In-flight requests have a configurable shutdown timeout (default: 30 seconds).
After the timeout, remaining connections are forcefully closed and the process
exits.

## Deployment

### Binary

Single static binary, no runtime dependencies:

```bash
cargo build --release
# Produces: target/release/reverse-proxy
```

The binary is self-contained — no system libraries beyond libc for DNS
resolution. The `aws_lc_rs` crypto provider is statically linked.

### Configuration

```bash
# Config file
/etc/reverse-proxy/config.toml

# ACME cache directory
/var/lib/reverse-proxy/acme-cache/

# Log directory (optional, for fail2ban)
/var/log/reverse-proxy/
```

### CLI

```bash
reverse-proxy [OPTIONS]

Options:
  --config <PATH>      Path to config file [default: /etc/reverse-proxy/config.toml]
  --validate          Validate config and exit
  --help              Show help
  --version           Show version
```

## Design Decisions

All design decisions are documented as ADRs in [decisions/](decisions/).

| ADR | Decision | Summary |
|-----|----------|---------|
| [001](decisions/001-rust-axum.md) | Rust with axum | Memory safety; single binary deployment |
| [006](decisions/006-rate-limiting-approach.md) | Token bucket rate limiting | In-memory per-IP token bucket matching nginx burst semantics |
| [007](decisions/007-custom-log-format.md) | Custom structured log format | key=value pairs with RATE_LIMIT prefix for fail2ban |
| [009](decisions/009-signal-handling.md) | Signal handling strategy | signal-hook for SIGTERM/SIGINT/SIGHUP |

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). Key
questions affecting this document:

- **OQ-03**: Should the health check endpoint be on a separate port? (open)