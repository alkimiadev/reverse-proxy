---
status: draft
last_updated: 2026-06-11
---

# Configuration

## What It Is

The configuration system defines how the proxy is configured, how configuration
is loaded, and how dynamic configuration can be reloaded without restarting the
process.

## Why It Exists

The proxy needs to be configurable without hard-coding domains, upstream
addresses, or TLS settings. The configuration system separates immutable
startup parameters (bind addresses, TLS mode) from runtime-adjustable
parameters (site definitions, rate limits) using the `ArcSwap` pattern proven
in the alknet project.

## Architecture

```
config.toml
    │
    ▼
┌──────────────────────┐
│  serde::Deserialize   │
│  (TOML → Config)     │
└──────────┬───────────┘
           │
           ▼
┌──────────────────────┐     ┌──────────────────────┐
│  StaticConfig         │     │  DynamicConfig        │
│  (immutable)         │     │  (hot-reloadable)     │
│                      │     │                       │
│  bind_addr           │     │  sites[]              │
│  http_port           │     │  rate_limit           │
│  https_port          │     │  body_limit           │
│  health_check_port   │     │  proxy_headers        │
│  admin_socket_path   │     │                       │
│  tls.mode            │     │  ← ArcSwap →          │
│  tls.acme_domains    │     │                       │
│  tls.cert_path       │     │  ← ArcSwap →          │
│  tls.key_path        │     │  ConfigReloadHandle    │
│  tls.cache_dir       │     │  .reload(new_config)  │
│  log_level           │     │                       │
│  log_format          │     └───────────────────────┘
└──────────────────────┘
```

## Static vs Dynamic Configuration

This split follows the pattern established in alknet (ADR-030) and adapted
for our simpler use case.

### StaticConfig

Immutable after startup. Changes require a process restart.

| Field | Type | Description |
|-------|------|-------------|
| `bind_addr` | `String` | IP address to bind to (must be explicit, no `0.0.0.0`; see ADR-016) |
| `http_port` | `u16` | Port for HTTP→HTTPS redirect (default: `80`; set to `0` to disable) |
| `https_port` | `u16` | Port for TLS listener (default: `443`) |
| `health_check_port` | `u16` | Port for local health check endpoint (default: `9900`; set to `0` to disable; see ADR-013) |
| `admin_socket_path` | `String` | Unix domain socket path for admin API (default: `/run/reverse-proxy/admin.sock`; empty string to disable; see ADR-014) |
| `tls.mode` | `"acme"` or `"manual"` | Certificate provisioning mode |
| `tls.acme_domains` | `Vec<String>` | Domains for ACME SAN certificate (ACME mode only) |
| `tls.acme_cache_dir` | `String` | ACME state cache directory |
| `tls.acme_directory` | `"production"` or `"staging"` | Let's Encrypt directory |
| `tls.cert_path` | `String` | Certificate file path (manual mode only) |
| `tls.key_path` | `String` | Private key file path (manual mode only) |
| `log_level` | `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"` | Logging verbosity |
| `log_format` | `"text"` or `"json"` | Log output format |

**Why these are static:** See ADR-008 for the rationale behind the
static/dynamic split. In summary: changing bind addresses, ports, or TLS mode
requires creating new listeners and TLS configurations — operations that
fundamentally require a restart.

### DynamicConfig

Hot-reloadable at runtime via `ArcSwap`. Changes take effect for new
connections immediately.

| Field | Type | Description |
|-------|------|-------------|
| `sites` | `Vec<SiteConfig>` | Site definitions (hostname → upstream mapping) |
| `rate_limit.requests_per_second` | `u32` | Rate limit per IP (global in Phase 1) |
| `rate_limit.burst` | `u32` | Burst capacity (global in Phase 1) |
| `body_limit_bytes` | `u64` | Max request body size in bytes (global in Phase 1) |

**SiteConfig:**

| Field | Type | Description |
|-------|------|-------------|
| `host` | `String` | Hostname to match (e.g., `"git.alk.dev"`) |
| `upstream` | `String` | Upstream address (e.g., `"127.0.0.1:3000"`) |
| `upstream_scheme` | `"http"` or `"https"` | Protocol for upstream connection (default: `"http"`) |
| `upstream_connect_timeout_secs` | `u64` | TCP connect timeout in seconds (default: `5`; see ADR-015, ADR-017) |
| `upstream_request_timeout_secs` | `u64` | Full request timeout in seconds (default: `60`; see ADR-015, ADR-017) | |

**Why these are dynamic:** See ADR-008 for the rationale. Site definitions
and rate limits are per-request concerns that should not require restarting
the proxy or dropping active connections. Rate limits and body limits are
global settings in Phase 1; per-site configuration for these is deferred to
Phase 2.

## Config Reload

### ArcSwap Pattern

`DynamicConfig` is wrapped in `Arc<ArcSwap<DynamicConfig>>`. This provides:

- **Lock-free reads**: Every handler reads the current config via a single
  `Arc` dereference — no lock contention on the request hot path.
- **Atomic writes**: `ConfigReloadHandle::reload(new_config)` swaps the entire
  config atomically. All new requests see the new config immediately.
- **No partial updates**: The entire config is swapped at once. There's no risk
  of reading a half-updated config.

See [ADR-008](decisions/008-static-dynamic-config-split.md) for the rationale
behind this split.

### Reload Trigger

Config reload is triggered by two mechanisms:

1. **SIGHUP**: Re-reads the config file, validates, and swaps DynamicConfig if
   valid. Simple and well-understood, but provides no feedback on success or
   failure.

2. **Admin socket**: The `reload` command via the admin Unix domain socket
   performs the same action as SIGHUP but returns a structured response
   indicating success or failure with an error message. See ADR-014 for
   details.

Both mechanisms converge on the same code path:
1. Re-read the config file from disk
2. Deserialize into `DynamicConfig`
3. Validate (check upstream reachability is optional)
4. Call `ConfigReloadHandle::reload(new_config)`

## TOML Config Format

```toml
# reverse-proxy config

[server]
bind_addr = "203.0.113.10"  # Replace with actual bind address
http_port = 80
https_port = 443
health_check_port = 9900     # Local health check (0 to disable)
admin_socket_path = "/run/reverse-proxy/admin.sock"  # Empty string to disable

[server.tls]
mode = "acme"                    # "acme" or "manual"
acme_domains = ["git.alk.dev", "alk.dev"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"    # "production" or "staging"

# Manual mode (uncomment and comment out ACME settings)
# mode = "manual"
# cert_path = "/etc/letsencrypt/live/git.alk.dev/fullchain.pem"
# key_path = "/etc/letsencrypt/live/git.alk.dev/privkey.pem"

[server.logging]
level = "info"
format = "text"                  # "text" or "json"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600          # 100 MB

[[sites]]
host = "git.alk.dev"
upstream = "127.0.0.1:3000"
upstream_scheme = "http"
# upstream_connect_timeout_secs = 5    # Default: 5s
# upstream_request_timeout_secs = 60   # Default: 60s

[[sites]]
host = "alk.dev"
upstream = "127.0.0.1:8080"
upstream_scheme = "http"
```

### Validation

On startup, the config is validated:

1. `bind_addr` is not `0.0.0.0` (must be explicit)
2. In ACME mode, `acme_domains` must be non-empty
3. In manual mode, `cert_path` and `key_path` must both be set and the files
   must be readable
4. Each site must have a `host` and `upstream`
5. Site `host` values must be unique (no duplicate hostnames)
6. `rate_limit.requests_per_second` must be > 0
7. `body.limit_bytes` must be > 0

On SIGHUP reload, the same validation applies. If the new config fails
validation, the reload is rejected and the old config remains active. An error
is logged.

**On startup**: If config validation fails, the process exits with a non-zero
code and logs the validation errors. The proxy will not start with an invalid
configuration.

## Design Decisions

All design decisions are documented as ADRs in [decisions/](decisions/).

| ADR | Decision | Summary |
|-----|----------|---------|
| [003](decisions/003-toml-config.md) | TOML configuration format | Rust-native, unambiguous, excellent serde support |
| [008](decisions/008-static-dynamic-config-split.md) | Static/dynamic config split | Immutable StaticConfig, hot-reloadable DynamicConfig via ArcSwap |
| [010](decisions/010-multi-site-phase1.md) | Multi-site in Phase 1 | Multiple domains from initial release |
| [011](decisions/011-multi-domain-tls.md) | Multi-domain TLS config | Single SAN certificate covering all domains |
| [013](decisions/013-health-check-port.md) | Health check on separate local port | Localhost-only HTTP health check, configurable port |
| [014](decisions/014-unix-socket-reload.md) | Unix domain socket config reload API | Programmatic reload with success/failure feedback |
| [015](decisions/015-per-site-timeouts.md) | Per-site upstream timeouts with defaults | 5s connect / 60s request defaults, per-site overrides |
| [016](decisions/016-explicit-bind-address.md) | Explicit bind address required | Rejects `0.0.0.0` to prevent accidental exposure |

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). Key
questions affecting this document:

- ~~**OQ-04**: Should config reload support a Unix domain socket API in addition
  to SIGHUP?~~ (resolved — ADR-014: Unix domain socket admin API added)
- **OQ-07**: Should per-site TLS overrides be supported for mixed ACME/manual
  domains? (open)