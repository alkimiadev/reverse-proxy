# AGENTS.md

Guidance for LLM agents (and humans) working on this project.

## Project Overview

A memory-safe reverse proxy built with Rust/axum that replaces vulnerable nginx
installations. Terminates TLS, routes requests by Host header to upstream
services, enforces rate limits, and injects proxy headers. See `README.md` and
`docs/architecture/` for full details.

## Build & Run

```bash
cargo build                          # debug build
cargo build --release                # release build
cargo test                           # run all tests (unit + integration)
cargo test -- --nocapture            # run tests with stdout visible
cargo clippy                         # lint
reverse-proxy --config config.toml   # run (defaults to /etc/reverse-proxy/config.toml)
reverse-proxy --validate --config config.toml  # validate config only
```

For a static binary with no libc dependency:

```bash
cargo build --release --target x86_64-unknown-linux-musl
```

## Project Structure

```
src/
├── main.rs              # Entry point, server startup, listener binding
├── cli.rs               # CLI parsing (clap), config loading, validation
├── lib.rs               # Library root, module declarations
├── config/
│   ├── static_config.rs # StaticConfig — immutable, requires restart
│   ├── dynamic_config.rs# DynamicConfig — hot-reloadable via ArcSwap
│   ├── validation.rs    # Config validation rules (called at startup and reload)
│   └── test_fixtures.rs # Test config generation helpers
├── proxy/
│   ├── handler.rs       # Core reverse proxy handler (forward requests to upstream)
│   ├── headers.rs       # Proxy header injection (X-Real-IP, X-Forwarded-For, etc.)
│   ├── body_limit.rs    # Request body size limiting middleware
│   ├── error.rs         # Error response types (502, 504, 429, etc.)
│   └── mod.rs           # Router construction, client creation
├── tls/
│   ├── acceptor.rs      # TLS acceptor setup (manual + ACME)
│   ├── acme.rs          # ACME certificate provisioning via rustls-acme
│   ├── config.rs        # TLS ServerConfig construction, cipher suites
│   └── redirect.rs     # HTTP → HTTPS 301 redirect listener
├── rate_limit/
│   ├── mod.rs           # Rate limiting middleware, eviction task
│   └── bucket.rs        # Token bucket implementation (IPv4 /32, IPv6 /64)
├── admin/
│   ├── socket.rs        # Unix domain socket admin API (reload, status)
│   └── mod.rs
├── health.rs            # Health check endpoint on localhost:9900
├── logging/
│   ├── mod.rs           # Logging init (file + stdout, ANSI disabled)
│   └── format.rs       # Structured log format (REQUEST, RATE_LIMIT, etc.)
├── server.rs            # HTTPS listener serving with ALPN detection
├── shutdown.rs          # Graceful shutdown (SIGTERM, SIGINT) + SIGHUP reload
└── utils.rs             # Shared utilities
```

## Key Architecture Concepts

- **StaticConfig vs DynamicConfig**: Static config (bind addresses, TLS,
  ports) requires a restart. Dynamic config (sites, rate limits, body limits)
  can be reloaded at runtime via SIGHUP or admin socket, using `ArcSwap` for
  lock-free reads.
- **Multi-listener**: `[[listeners]]` in TOML — each listener has its own bind
  address, TLS config, and site routing. Sites are collected into a global
  routing table at runtime.
- **Edge proxy model**: The proxy is the edge — X-Forwarded-For is replaced
  (not appended), X-Real-IP is set from the connection's remote address.
- **No `/health` on public listener**: Health checking is localhost:9900 only.
  The main listener does not intercept any paths.
- **HTTP/2 client-facing only**: ALPN detects h2 vs http/1.1. Upstream
  connections are always HTTP/1.1.
- **IPv6 rate limiting**: IPv6 addresses are normalized to /64 prefixes so
  addresses within the same /64 share a token bucket.

## Config Format

TOML. See `docs/architecture/config.md` for full schema. Key validation rules:

- `bind_addr` must be explicit (no `0.0.0.0`) unless `allow_wildcard_bind` is
  enabled via config or `--allow-wildcard-bind` CLI flag (OR logic)
- Site `host` values must be unique across all listeners
- `upstream` must be in `host:port` format (e.g., `gitea:3000`, `127.0.0.1:3000`)
- ACME mode requires `acme_domains` (non-empty) and `acme_contact` (valid
  `mailto:` URI)
- Manual mode requires `cert_path` and `key_path` pointing to readable files
- `rate_limit.requests_per_second` and `rate_limit.burst` must be > 0
- `body.limit_bytes` must be > 0
- `http_port` must be 0 (disabled) or 1–65535; `https_port` must be 1–65535
- `health_check_port` must not conflict with any listener's http_port or
  https_port on the same bind address

## Testing

Tests use `rcgen` for self-signed certificate generation and `reqwest` for
HTTP client requests. Integration tests are in `tests/integration_test.rs`
with helpers in `tests/helpers/`.

```bash
cargo test                    # all tests
cargo test --test integration # integration tests only
cargo test --lib               # unit tests only
```

## Code Style

- No comments unless explicitly requested
- Error handling uses `anyhow` for application code and `thiserror` for
  library error types
- Structured logging with `tracing` — always `with_ansi(false)`
- Config types implement `serde::Deserialize` for TOML parsing
- All network operations use `tokio` async runtime

## Deployment Files

`deploy/` contains production-ready deployment configs:

- `Dockerfile` — multi-stage build (rust:alpine → alpine)
- `docker-compose.yml` — complete setup with Gitea example
- `reverse-proxy.service` — systemd unit file with security hardening
- `fail2ban/` — filter and jail config for rate limit log parsing

See `deploy/README.md` for step-by-step setup instructions.

## Common Modifications

### Adding a new site

Add a `[[listeners.sites]]` entry to config and reload:

```bash
echo "reload" | socat - UNIX-CONNECT:/run/reverse-proxy/admin.sock
```

### Changing rate limits

Update `[rate_limit]` in config and reload (no restart needed).

### Changing bind address or TLS config

These are in StaticConfig — require a full process restart.

### Adding per-site timeouts

Set `upstream_connect_timeout_secs` and `upstream_request_timeout_secs` on a
site definition. Defaults are 5s connect, 60s request.