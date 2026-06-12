# reverse-proxy

A memory-safe reverse proxy built with Rust and axum, designed to replace
vulnerable nginx installations for TLS-terminated host-based routing.

## Why

nginx's C codebase has a long history of memory corruption vulnerabilities, and
the discovery rate is accelerating. CVE-2026-42945 ("NGINX Rift") is an
unauthenticated RCE via the `rewrite` module with a public PoC and active
exploitation — and 6 of 7 recent nginx CVEs are memory corruption bugs that
Rust eliminates by construction.

This proxy targets a specific use case: TLS termination, host-based routing,
and request forwarding to upstream services. It is not a general-purpose web
server or load balancer.

## Features

- **TLS termination** — ACME (Let's Encrypt) with automatic provisioning and
  renewal, or manual certificates
- **HTTP/2** — ALPN-based protocol detection on the client-facing side; upstream
  connections use HTTP/1.1
- **Multi-site routing** — host-based routing to multiple upstream services from
  a single process
- **Multiple listeners** — dedicated-IP (one IP per domain) or shared-IP
  (SAN certificate) deployment models
- **Rate limiting** — per-IP token bucket with fail2ban-compatible structured
  logging (IPv6 rate limited per /64 prefix)
- **Proxy headers** — X-Real-IP, X-Forwarded-For (edge proxy model), X-Forwarded-Proto
- **Hot config reload** — SIGHUP or admin Unix domain socket with success/failure
  feedback
- **Health check** — localhost-only endpoint on a separate port (default: 9900)
- **HTTP → HTTPS redirect** — per-listener redirect on port 80
- **Graceful shutdown** — SIGTERM with in-flight request drain
- **systemd integration** — `Type=notify` with `sd_notify`
- **Container-ready** — Docker deployment with health check and fail2ban volume
  mount
- **Restricted cipher suites** — ECDHE-AES-GCM for TLS 1.2, all TLS 1.3 suites
  (matching nginx scope)

## Quick Start

### Build

```bash
cargo build --release
```

Produces a static binary at `target/release/reverse-proxy`. For a fully static
binary (no libc dependency), build with the `x86_64-unknown-linux-musl` target.

### Minimal Config

Create `/etc/reverse-proxy/config.toml`:

```toml
health_check_port = 9900

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "0.0.0.0"

[listeners.tls]
mode = "acme"
acme_domains = ["example.com"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "staging"
acme_contact = "mailto:admin@example.com"

[[listeners.sites]]
host = "example.com"
upstream = "127.0.0.1:8080"
```

> **Note:** `bind_addr = "0.0.0.0"` requires the `--allow-wildcard-bind` flag or
> `allow_wildcard_bind = true` in config. This is intentional — see
> [Explicit bind address](#explicit-bind-address).

### Run

```bash
reverse-proxy --config /etc/reverse-proxy/config.toml
```

Or with Docker (see [Deployment](#deployment)).

### Validate Config

```bash
reverse-proxy --config /etc/reverse-proxy/config.toml --validate
```

## Configuration

Configuration uses TOML and is split into **static** (requires restart) and
**dynamic** (hot-reloadable) sections.

### Static Config (requires restart)

| Field | Default | Description |
|-------|---------|-------------|
| `listeners` | (required) | TLS listener definitions |
| `allow_wildcard_bind` | `false` | Allow `0.0.0.0` bind addresses |
| `health_check_port` | `9900` | Local health check port (`0` to disable) |
| `admin_socket_path` | `/run/reverse-proxy/admin.sock` | Admin Unix socket (empty string to disable) |
| `shutdown_timeout_secs` | `30` | Graceful shutdown timeout |
| `logging.level` | `"info"` | Log level |
| `logging.format` | `"text"` | Log format (`"text"` or `"json"`) |
| `logging.log_file_path` | (not set) | Path to log file for fail2ban |

### Dynamic Config (hot-reloadable via SIGHUP or admin socket)

| Field | Default | Description |
|-------|---------|-------------|
| `sites[].host` | (required) | Hostname to match |
| `sites[].upstream` | (required) | Upstream `host:port` address |
| `sites[].upstream_scheme` | `"http"` | Upstream protocol (`"http"` or `"https"`) |
| `sites[].upstream_connect_timeout_secs` | `5` | TCP connect timeout |
| `sites[].upstream_request_timeout_secs` | `60` | Full request timeout |
| `rate_limit.requests_per_second` | (required) | Per-IP request rate |
| `rate_limit.burst` | (required) | Burst capacity |
| `body.limit_bytes` | (required) | Max request body size |

### TLS Modes

**ACME** (automatic Let's Encrypt certificates):

```toml
[[listeners]]
bind_addr = "203.0.113.10"

[listeners.tls]
mode = "acme"
acme_domains = ["git.example.com", "example.com"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"
acme_contact = "mailto:admin@example.com"

[[listeners.sites]]
host = "git.example.com"
upstream = "gitea:3000"

[[listeners.sites]]
host = "example.com"
upstream = "app:8080"
```

**Manual** (bring your own certificates):

```toml
[[listeners]]
bind_addr = "203.0.113.11"

[listeners.tls]
mode = "manual"
cert_path = "/etc/ssl/example.com/fullchain.pem"
key_path = "/etc/ssl/example.com/privkey.pem"

[[listeners.sites]]
host = "example.com"
upstream = "127.0.0.1:8080"
```

### Explicit Bind Address

By default, `bind_addr` must be an explicit IP address. `0.0.0.0` is rejected
to prevent accidental exposure. For container deployments where the proxy binds
inside the container and Docker handles port publishing, enable wildcard binding
with either:

- Config: `allow_wildcard_bind = true`
- CLI: `--allow-wildcard-bind`

Either source enables it (OR logic, not AND).

## Deployment

### Docker

```yaml
services:
  reverse-proxy:
    build: .
    container_name: reverse-proxy
    restart: unless-stopped
    ports:
      - "203.0.113.10:80:80"
      - "203.0.113.10:443:443"
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
```

Container config must set `allow_wildcard_bind = true` and bind to `0.0.0.0`.

See [`deploy/docker-compose.yml`](deploy/docker-compose.yml) for a complete
example including Gitea and PostgreSQL.

### systemd

Install the binary and service file:

```bash
cp target/release/reverse-proxy /usr/local/bin/
cp deploy/reverse-proxy.service /etc/systemd/system/
```

Create config at `/etc/reverse-proxy/config.toml`, then:

```bash
systemctl enable --now reverse-proxy
```

See [`deploy/reverse-proxy.service`](deploy/reverse-proxy.service) for the
unit file with security hardening options.

### fail2ban

Install the filter and jail config:

```bash
cp deploy/fail2ban/filter.d/reverse-proxy.conf /etc/fail2ban/filter.d/
cp deploy/fail2ban/jail.d/reverse-proxy.conf /etc/fail2ban/jail.d/
systemctl restart fail2ban
```

The filter matches `RATE_LIMIT` log lines from the proxy's structured log
output. The jail bans IPs after 10 rate-limited requests within 60 seconds
(adjust `maxretry` and `findtime` to taste).

Rate-limited requests produce log lines like:

```
RATE_LIMIT client_ip=203.0.113.50 host=git.example.com path=/login status=429
```

For Docker deployments, mount the log directory so fail2ban on the host can
read it:

```yaml
volumes:
  - /var/log/reverse-proxy:/var/log/reverse-proxy
```

Enable file logging in config:

```toml
[logging]
log_file_path = "/var/log/reverse-proxy/access.log"
```

## Admin Socket

The admin Unix domain socket supports two commands:

```bash
# Reload config
echo "reload" | socat - UNIX-CONNECT:/run/reverse-proxy/admin.sock

# Check status
echo "status" | socat - UNIX-CONNECT:/run/reverse-proxy/admin.sock
```

Responses are newline-terminated JSON:

```json
{"status":"ok"}
{"status":"ok","uptime_secs":1234,"sites":2}
{"status":"error","message":"config validation failed: ..."}
```

Config can also be reloaded with `kill -SIGHUP $(pidof reverse-proxy)`, but
SIGHUP provides no feedback on success or failure.

## Health Check

```bash
curl http://127.0.0.1:9900/health
```

Returns `200 OK` with an empty body. Bound to localhost only — not exposed on
public ports.

## Architecture

```
                    ┌────────────────────────────────────┐
                    │  reverse-proxy (Rust/axum)         │
config.toml ──────► │  StaticConfig + DynamicConfig      │
                    │  (ArcSwap for hot-reload)           │
                    │                                      │
                    │  ┌─ Listener 1 ─────────────────┐   │
 bind_addr:80  ───► │  │  HTTP → 301 redirect           │   │
                    │  └────────────────────────────────┘   │
                    │                                      │
 bind_addr:443 ───► │  │  TLS listener (tokio-rustls)    │   │
                    │  │  ├─ ACME or Manual TLS config    │   │
                    │  │  └─ axum router (per-listener)   │   │
                    │  │     ├─ Host → global site lookup  │   │
                    │  │     ├─ Rate limiting, headers     │   │
                    │  │     └─ Proxy to upstream           │   │
                    │  └────────────────────────────────┘   │
                    │                                      │
                    │  /health → 200 OK (port 9900)        │
                    │  Admin socket (Unix domain)           │
                    └────────────────────────────────────┘
```

For full architecture documentation, see [`docs/architecture/`](docs/architecture/).

## Project Structure

```
src/
├── main.rs              # Entry point, server startup
├── cli.rs               # CLI argument parsing
├── lib.rs               # Library root
├── config/
│   ├── static_config.rs # Immutable startup configuration
│   ├── dynamic_config.rs# Hot-reloadable runtime configuration
│   └── validation.rs    # Config validation rules
├── proxy/
│   ├── handler.rs       # Core reverse proxy handler
│   ├── headers.rs       # Proxy header injection
│   ├── body_limit.rs    # Request body size limiting
│   └── error.rs         # Error response types
├── tls/
│   ├── acceptor.rs      # TLS acceptor setup
│   ├── acme.rs          # ACME certificate provisioning
│   ├── config.rs        # TLS configuration
│   └── redirect.rs      # HTTP → HTTPS redirect
├── rate_limit/
│   ├── mod.rs           # Rate limiting middleware
│   └── bucket.rs        # Token bucket implementation
├── admin/
│   ├── socket.rs        # Unix domain socket admin API
│   └── mod.rs
├── health.rs            # Health check endpoint
├── logging/
│   ├── mod.rs           # Logging initialization
│   └── format.rs        # Structured log formatting
├── server.rs            # HTTPS listener serving
├── shutdown.rs          # Graceful shutdown handling
└── utils.rs             # Shared utilities

deploy/
├── Dockerfile
├── docker-compose.yml
├── reverse-proxy.service
└── fail2ban/
    ├── filter.d/reverse-proxy.conf
    └── jail.d/reverse-proxy.conf

docs/
├── architecture/        # Full architecture documentation
│   ├── overview.md
│   ├── proxy.md
│   ├── tls.md
│   ├── config.md
│   ├── operations.md
│   └── decisions/       # Architecture Decision Records (ADRs)
└── research/
    └── threat-landscape.md
```

## Why Rust

6 of 7 recent nginx CVEs are memory corruption bugs (buffer overflows,
use-after-free, out-of-bounds reads) — the exact class of bugs Rust eliminates
by construction. Combined with rustls (pure Rust TLS, no OpenSSL dependency),
this proxy provides a fundamentally safer baseline than nginx.

Rust does not eliminate logic bugs. Rate limiting, header injection prevention,
and access control still require careful implementation. But it eliminates the
entire category of vulnerabilities that make nginx's C codebase a persistent
attack surface.

See [`docs/research/threat-landscape.md`](docs/research/threat-landscape.md)
for the full vulnerability analysis that motivated this project.

## License

Licensed under either of

- Apache License, Version 2.0
  ([http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))
- MIT License
  ([http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.