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

The token bucket uses **nodelay** semantics matching nginx's `limit_req burst
nodelay`: when the bucket is empty, the request is immediately rejected with
429 — requests are not queued. Tokens are added at a rate of
`requests_per_second` (1 token every 1000ms / requests_per_second), and the
bucket capacity is the `burst` value.

When a request exceeds the rate limit, the middleware returns `429 Too Many
Requests` and logs the event with structured fields.

### State Eviction

The per-IP token bucket state grows over time as new IPs are seen. A
background task runs every 60 seconds (configurable) and removes entries
whose last access timestamp is older than a configurable eviction age
(default: 300 seconds / 5 minutes). This prevents unbounded memory growth
while preserving recent entries that may still receive traffic.

### Config Reload Behavior

When rate limit parameters change (e.g., from 10 req/s burst 20 to 20 req/s
burst 40), the behavior is:

1. New `DynamicConfig` is swapped in via ArcSwap.
2. On the next request from an existing IP, the rate limiter reads the current
   `DynamicConfig` for rate/burst parameters.
3. The token bucket refills using the new rate, and its capacity is set to the
   new burst maximum.
4. If the current token count exceeds the new burst maximum, it is capped to
   the new burst maximum.

The HashMap is **not** cleared — this avoids creating a rate-limiting gap.
Existing buckets adopt new parameters on their next request. The eviction task
continues removing stale entries independently.

### IPv6 Rate Limiting

IPv6 addresses have a vastly larger address space than IPv4. Rate limiting per
individual IPv6 address (`/128`) is ineffective against attackers who can
generate many addresses within a `/64` prefix.

- **IPv4**: Rate limited per individual address (`/32`).
- **IPv6**: Rate limited per `/64` prefix. All addresses in the same `/64` share
  the same token bucket. This matches RFC 4941 privacy extension boundaries and
  common anti-abuse practice.

The rate limiter normalizes IPv6 addresses to their `/64` prefix before
bucket lookup.

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

Logs are written to two destinations simultaneously:
- **File** (primary): `/var/log/reverse-proxy/access.log` — the authoritative
  source for fail2ban consumption. File logging is always enabled when the
  `log_file_path` config is set. See ADR-020 for the rationale behind
  file-primary logging.
- **stdout/stderr**: Always-on, for `docker logs`, `journalctl`, and
  development use. Structured in the same format as the file output.

The `tracing-subscriber` layer configuration supports both simultaneously via
`Layer` composition.

### File Logging and fail2ban

File logging is the primary integration point for fail2ban. A log file on a
volume mount is simpler and more reliable than parsing Docker log drivers or
journald — no log driver configuration, no format conversion, no risk of
dropping events.

In container deployments, the log directory is volume-mounted so fail2ban on
the host can read it directly:

```yaml
volumes:
  - /var/log/reverse-proxy:/var/log/reverse-proxy
```

A corresponding fail2ban filter definition and jail configuration are provided
as part of the deployment documentation.

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

### Local Health Check Port

The primary health check endpoint is served on a separate local port (default:
9900), bound to `127.0.0.1` only. This ensures health checks work even when TLS
is misconfigured. See ADR-013 for the rationale.

```
GET http://127.0.0.1:9900/health → 200 OK (empty body)
```

The port is configurable via `health_check_port` in StaticConfig. Setting it
to `0` disables the separate health check listener.

### HTTPS Health Check (Fallback)

When the local health check port is enabled, `/health` is also available on the
main HTTPS listener for cases where TLS-level health verification is desired.
External monitoring should prefer the local health check for liveness checks
and can use the HTTPS endpoint for TLS verification.

### What It Checks

- Process is running and the tokio runtime is responsive
- TLS listener is accepting connections (HTTPS endpoint only)
- Config is loaded (StaticConfig and DynamicConfig are initialized)

It does **not** check upstream reachability. The health check answers "is the
proxy process healthy?", not "is the upstream reachable?" — upstream health is
a separate concern that would produce 502/504 responses in the proxy handler.

### Future Extensions

- `/health/ready` — readiness check that includes upstream reachability
- Prometheus metrics at `/metrics`

## Systemd Integration

The proxy can also run as a bare binary via systemd (alternative to container
deployment). The systemd unit file is provided for this use case.

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
  DynamicConfig if valid. No feedback on success or failure.
- **Admin socket reload**: Send `reload` command via the Unix domain socket
  (default: `/run/reverse-proxy/admin.sock`). Returns structured response
  indicating success or failure. See ADR-014 for details.

### SIGHUP for Config Reload

SIGHUP triggers config reload (see [config.md](config.md) for details). The
process does not exit on SIGHUP.

### Admin Socket for Config Reload

The admin Unix domain socket provides programmatic config reload with feedback.
This is useful for CI/CD pipelines and automation tools. See ADR-014 for the
rationale.

**Protocol:**

- **Connection lifecycle**: One command per connection. Client connects, sends
  one newline-terminated command, receives one newline-terminated JSON
  response, then the server closes the connection.
- **Message framing**: Newline-delimited (`\n`). Responses end with `\n`.
- **Commands**:
  - `reload` — Re-read config file, validate, and swap DynamicConfig. Returns
    `{"status": "ok"}` or `{"status": "error", "message": "..."}`.
  - `status` — Return basic process info. Returns
    `{"status": "ok", "uptime_secs": 1234, "sites": 2}`.
- **Error responses**: Unrecognized commands return
  `{"status": "error", "message": "unknown command: <cmd>"}`. Invalid or empty
  input returns `{"status": "error", "message": "invalid input"}`.
- **Concurrency**: Multiple clients can connect simultaneously, but reload
  operations are serialized (see Config Reload section in config.md).
- **Socket cleanup**: The proxy removes any existing socket file at startup
  before binding. If the file exists and another process is listening, a warning
  is logged and the admin socket is disabled (but the proxy continues starting).

### Shutdown Sequence

On SIGTERM or SIGINT, the proxy performs a graceful shutdown:

1. **Stop accepting new connections** — Close all TCP listening sockets. No new
   connections are accepted.
2. **Close idle keep-alive connections** — Send `Connection: close` on any idle
   connections in the keep-alive pool.
3. **Wait for in-flight requests** — Up to `shutdown_timeout_secs` (default: 30)
   for active requests to complete.
4. **Force-close remaining connections** — After the timeout, any remaining
   connections are forcefully closed via TCP RST.
5. **Cancel background tasks** — ACME renewal tasks, rate limiter eviction task,
   and admin socket listener are all cancelled.
6. **Exit with code 0**.

The `shutdown_timeout_secs` is configurable in StaticConfig (default: 30
seconds). See config.md for details.

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
  --allow-wildcard-bind  Permit 0.0.0.0 as a bind address (for container deployments)
  --help              Show help
  --version           Show version
```

## Container Deployment

### Rationale

The proxy runs in a minimal Docker container for defense-in-depth. Even if an
attacker finds a logic-level vulnerability, they must also escape the container
boundary. Combined with Rust's memory safety, this provides two independent
barriers against exploitation. See ADR-020 for the full rationale.

### Container Image

Multi-stage build: compile in `rust:alpine`, run in `alpine` (or `scratch` for
absolute minimum). The final image contains only the static binary and
necessary runtime files. No shell, no package manager, no unnecessary tools.

The binary is compiled against the `x86_64-unknown-linux-musl` target for
static linking. The `aws_lc_rs` crypto provider is statically linked — no
OpenSSL dependency.

### Networking

The proxy supports flexible upstream addressing — no assumption about upstream
localality:

| Deployment | Upstream Address | Example |
|------------|-----------------|---------|
| Same-host, shared Docker network | Docker DNS name | `gitea:3000` |
| Same-host, host networking | Loopback | `127.0.0.1:3000` |
| Different host, LAN | LAN IP | `10.0.0.5:3000` |
| Different host, VPN/tunnel | Tunnel endpoint | Varies by tunnel config |

In container deployments, the proxy binds `0.0.0.0` inside the container and
Docker publishes specific ports to the host IP. The `allow_wildcard_bind`
override is required for this configuration (see ADR-016, ADR-020).

### Volume Mounts

| Container Path | Host Path | Purpose |
|---------------|-----------|---------|
| `/etc/reverse-proxy/config.toml` | Config file (read-only) | Proxy configuration |
| `/var/lib/reverse-proxy/acme-cache/` | ACME state directory | Certificate persistence across restarts |
| `/var/log/reverse-proxy/` | Log directory | fail2ban reads from host |
| `/run/reverse-proxy/admin.sock` | Admin socket | Host-side config reload commands |

### Docker Compose Example

This example shows the reverse proxy alongside a Gitea container on a shared
Docker network. Real IPs, secrets, and domain names are replaced with
placeholders.

```yaml
services:
  reverse-proxy:
    build: .
    container_name: reverse-proxy
    restart: unless-stopped
    ports:
      - "203.0.113.10:80:80"     # HTTP redirect
      - "203.0.113.10:443:443"   # HTTPS
    volumes:
      - /etc/reverse-proxy/config.toml:/etc/reverse-proxy/config.toml:ro
      - /var/lib/reverse-proxy/acme-cache:/var/lib/reverse-proxy/acme-cache
      - /var/log/reverse-proxy:/var/log/reverse-proxy
      - /run/reverse-proxy:/run/reverse-proxy
    networks:
      - proxy-net
    healthcheck:
      test: ["CMD", "wget", "-q", "--spider", "http://127.0.0.1:9900/health"]
      interval: 30s
      timeout: 5s
      retries: 3

  gitea:
    image: gitea/gitea:latest
    container_name: gitea
    restart: unless-stopped
    ports:
      - "203.0.113.10:22:2222"    # Git SSH
    volumes:
      - /opt/gitea:/data
    networks:
      - proxy-net
      - gitea-db-net

  gitea-db:
    image: postgres:16-alpine
    container_name: gitea-db
    restart: unless-stopped
    environment:
      POSTGRES_USER: admin
      POSTGRES_PASSWORD: ${DB_PASSWORD}
      POSTGRES_DB: gitea
    volumes:
      - gitea-db:/var/lib/postgresql/data
    networks:
      - gitea-db-net

networks:
  proxy-net:
  gitea-db-net:

volumes:
  gitea-db:
```

Corresponding proxy config (inside the container):

```toml
allow_wildcard_bind = true
health_check_port = 9900
admin_socket_path = "/run/reverse-proxy/admin.sock"

[logging]
level = "info"
format = "text"
log_file_path = "/var/log/reverse-proxy/access.log"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "0.0.0.0"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["git.example.com"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"

[[listeners.sites]]
host = "git.example.com"
upstream = "gitea:3000"    # Docker DNS resolves this
```

### fail2ban Integration

In container deployments, fail2ban runs on the host and reads the proxy's log
file from the volume mount:

```
/var/log/reverse-proxy/access.log  →  fail2ban filter  →  iptables/nftables
```

This is simpler and more reliable than parsing Docker log drivers. The log
file is the authoritative source for rate limit events and access logs.

### Health Check

Docker's native `HEALTHCHECK` uses the local health endpoint:

```dockerfile
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
  CMD wget -q --spider http://127.0.0.1:9900/health || exit 1
```

No port publishing is needed — the health check runs inside the container.

### SSH Traffic

SSH traffic for Git operations is not proxied through the reverse proxy. It
continues to be routed directly to the Gitea container via Docker port
publishing (e.g., `203.0.113.10:22:2222`), matching the current deployment
pattern.

## Startup Sequence

The proxy starts components in a specific order to ensure fail-fast behavior
and correct dependency initialization:

1. **Parse and validate config** — Read the TOML config file, deserialize into
   `StaticConfig` and `DynamicConfig`, and validate all rules. If validation
   fails, exit with non-zero code and log errors. No ports are bound.

2. **Initialize DynamicConfig** — Load sites, rate limits, and body limits into
   `ArcSwap<DynamicConfig>`.

3. **Initialize shared state** — Create the rate limiter
   `HashMap<IpAddr, TokenBucket>`, the shared `hyper::Client`, and the
   `tracing-subscriber` with file and stdout layers.

4. **Bind health check port** (if enabled) — Bind `127.0.0.1:{health_check_port}`.
   Fail-fast if bind fails.

5. **Bind admin socket** (if enabled) — Remove any stale socket file first, then
   bind the Unix domain socket. If the socket file exists and another process is
   listening, log a warning and fail the admin socket (but continue starting —
   the admin socket is non-critical).

6. **Bind all listener ports** — For each listener: bind HTTP port (if enabled)
   and HTTPS port. If any bind fails, fail-fast and exit. All ports are bound
   before proceeding.

7. **Load TLS configuration** — For each listener: load manual certificates or
   initialize ACME state machine. If manual certificate loading fails, fail-fast
   and exit. For ACME: if no cached certificate exists and ACME provisioning
   fails, fail-fast and exit.

8. **Start TCP listeners** — Begin accepting connections on all bound ports.

9. **Start background tasks** — ACME renewal tasks (per listener in ACME mode),
   rate limiter eviction task, signal handler task, admin socket handler task.

10. **Signal readiness** — Send `sd_notify("READY=1")` to systemd (if running
    under systemd).

**Failure semantics**: **Fail-fast**. If any step fails, the process exits with
a non-zero code. The proxy does not partially start. All ports are bound before
any connections are accepted.

## Design Decisions

All design decisions are documented as ADRs in [decisions/](decisions/).

| ADR | Decision | Summary |
|-----|----------|---------|
| [001](decisions/001-rust-axum.md) | Rust with axum | Memory safety; single binary deployment |
| [006](decisions/006-rate-limiting-approach.md) | Token bucket rate limiting | In-memory per-IP token bucket matching nginx burst semantics |
| [007](decisions/007-custom-log-format.md) | Custom structured log format | key=value pairs with RATE_LIMIT prefix for fail2ban |
| [009](decisions/009-signal-handling.md) | Signal handling strategy | signal-hook for SIGTERM/SIGINT/SIGHUP |
| [013](decisions/013-health-check-port.md) | Health check on separate local port | Localhost-only HTTP health check, configurable port |
| [014](decisions/014-unix-socket-reload.md) | Unix domain socket config reload API | Programmatic reload with success/failure feedback |
| [020](decisions/020-container-deployment.md) | Container deployment model | Defense-in-depth via container isolation; file-primary logging |

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). Key
questions affecting this document:

- ~~**OQ-03**: Should the health check endpoint be on a separate port?~~ (resolved
  — ADR-013: separate local port, default 9900, localhost only)