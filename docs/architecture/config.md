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
┌──────────────────────┐
│  StaticConfig         │
│  (immutable)         │
│                      │
│  health_check_port   │
│  admin_socket_path   │
│  log_level           │
│  log_format          │
│                      │
│  listeners[]         │
│  ┌────────────────┐  │
│  │ Listener 1      │  │
│  │ bind_addr       │  │
│  │ http_port       │  │
│  │ https_port      │  │
│  │ tls.mode        │  │
│  │ tls.acme_domains│  │
│  │ tls.acme_cache_dir│ │
│  │ tls.acme_directory│ │
│  │ tls.cert_path   │  │
│  │ tls.key_path    │  │
│  └────────────────┘  │
│  ┌────────────────┐  │
│  │ Listener N      │  │
│  │ ...             │  │
│  └────────────────┘  │
└──────────────────────┘

┌──────────────────────┐
│  DynamicConfig        │
│  (hot-reloadable)     │
│                       │
│  sites[]              │
│  rate_limit           │
│  body_limit           │
│                       │
│  ← ArcSwap →          │
│  ConfigReloadHandle    │
│  .reload(new_config)  │
└───────────────────────┘
```

## Static vs Dynamic Configuration

This split follows the pattern established in alknet (ADR-030) and adapted
for our simpler use case. See ADR-019 for the rationale behind the
`[[listeners]]` configuration format.

### StaticConfig

Immutable after startup. Changes require a process restart.

| Field | Type | Description |
|-------|------|-------------|
| `listeners` | `Vec<ListenerConfig>` | Independent TLS endpoints, each with its own bind address and TLS config (see ADR-019) |
| `health_check_port` | `u16` | Port for local health check endpoint (default: `9900`; set to `0` to disable; see ADR-013) |
| `admin_socket_path` | `String` | Unix domain socket path for admin API (default: `/run/reverse-proxy/admin.sock`; empty string to disable; see ADR-014) |
| `log_level` | `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"` | Logging verbosity |
| `log_format` | `"text"` or `"json"` | Log output format |

**ListenerConfig** (per-listener static config):

| Field | Type | Description |
|-------|------|-------------|
| `bind_addr` | `String` | IP address to bind to (must be explicit, no `0.0.0.0`; see ADR-016) |
| `http_port` | `u16` | Port for HTTP→HTTPS redirect (default: `80`; set to `0` to disable) |
| `https_port` | `u16` | Port for TLS listener (default: `443`) |
| `tls.mode` | `"acme"` or `"manual"` | Certificate provisioning mode |
| `tls.acme_domains` | `Vec<String>` | Domains for ACME SAN certificate (ACME mode only) |
| `tls.acme_cache_dir` | `String` | ACME state cache directory |
| `tls.acme_directory` | `"production"` or `"staging"` | Let's Encrypt directory |
| `tls.cert_path` | `String` | Certificate file path (manual mode only) |
| `tls.key_path` | `String` | Private key file path (manual mode only) |

**Why listeners are static:** Each listener requires binding a TCP socket and
constructing a TLS acceptor — operations that fundamentally require a restart.
Changing a listener's bind address, TLS mode, or certificate configuration
cannot be done without creating new listeners. See ADR-008 and ADR-019.

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
| `upstream_request_timeout_secs` | `u64` | Full request timeout in seconds (default: `60`; see ADR-015, ADR-017) |

Sites are defined per listener in the `[[listeners]]` entries. Each listener
routes its own sites independently. The `DynamicConfig` collects all sites
across all listeners for hot-reload via `ArcSwap`. When a config reload
occurs, all listener site mappings are updated atomically.

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

### Multi-Config (Dedicated-IP Per Domain)

The primary deployment model — each listener on its own IP with its own TLS
certificate:

```toml
# reverse-proxy config

# Global settings
health_check_port = 9900     # Local health check (0 to disable)
admin_socket_path = "/run/reverse-proxy/admin.sock"  # Empty string to disable

[logging]
level = "info"
format = "text"                  # "text" or "json"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600          # 100 MB

# Listener 1: git.alk.dev on its own IP
[[listeners]]
bind_addr = "203.0.113.10"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["git.alk.dev"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache-git"
acme_directory = "production"

[[listeners.sites]]
host = "git.alk.dev"
upstream = "127.0.0.1:3000"
upstream_scheme = "http"
# upstream_connect_timeout_secs = 5    # Default: 5s
# upstream_request_timeout_secs = 60    # Default: 60s

# Listener 2: alk.dev on its own IP with a manual certificate
[[listeners]]
bind_addr = "203.0.113.11"
http_port = 80
https_port = 443

[listeners.tls]
mode = "manual"
cert_path = "/etc/ssl/alk.dev/fullchain.pem"
key_path = "/etc/ssl/alk.dev/privkey.pem"

[[listeners.sites]]
host = "alk.dev"
upstream = "127.0.0.1:8080"
upstream_scheme = "http"
```

### Shared-IP Multi-Domain (SAN Certificate)

A single listener serving multiple domains with one SAN certificate:

```toml
# Global settings
health_check_port = 9900
admin_socket_path = "/run/reverse-proxy/admin.sock"

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

# Single listener with multi-domain SAN certificate
[[listeners]]
bind_addr = "203.0.113.10"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["git.alk.dev", "alk.dev"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"

[[listeners.sites]]
host = "git.alk.dev"
upstream = "127.0.0.1:3000"

[[listeners.sites]]
host = "alk.dev"
upstream = "127.0.0.1:8080"
```

### Validation

On startup, the config is validated:

1. At least one `[[listeners]]` entry must exist
2. Each listener's `bind_addr` is not `0.0.0.0` (must be explicit; see ADR-016)
3. Each listener's `bind_addr` and `https_port` combination must be unique
4. In ACME mode, `acme_domains` must be non-empty
5. In manual mode, `cert_path` and `key_path` must both be set and the files
   must be readable
6. Each site must have a `host` and `upstream`
7. Site `host` values must be unique across all listeners (no duplicate
   hostnames, even across different listeners). Duplicate hostnames would create
   ambiguous routing — the proxy would not know which listener's upstream to
   route a request to when the `Host` header matches multiple sites.
8. `rate_limit.requests_per_second` must be > 0
9. `body.limit_bytes` must be > 0

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
| [019](decisions/019-multi-config-listeners.md) | Multi-config listeners | `[[listeners]]` supporting both dedicated-IP and shared-IP deployment models |

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). Key
questions affecting this document:

- ~~**OQ-04**: Should config reload support a Unix domain socket API in addition
  to SIGHUP?~~ (resolved — ADR-014: Unix domain socket admin API added)
- ~~**OQ-07**: Should per-site TLS overrides be supported for mixed ACME/manual
  domains?~~ (resolved — ADR-019: `[[listeners]]` with per-listener TLS config)