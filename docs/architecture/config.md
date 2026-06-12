---
status: reviewed
last_updated: 2026-06-12
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
| `allow_wildcard_bind` | `bool` | Allow `0.0.0.0` as a bind address. Required for container deployments. Default: `false` (see ADR-016, ADR-020) |
| `health_check_port` | `u16` | Port for local health check endpoint (default: `9900`; set to `0` to disable; bound to `127.0.0.1` only; see ADR-013, ADR-022) |
| `admin_socket_path` | `String` | Unix domain socket path for admin API (default: `/run/reverse-proxy/admin.sock`; empty string to disable; see ADR-014) |
| `shutdown_timeout_secs` | `u64` | Maximum seconds to wait for in-flight requests during graceful shutdown (default: `30`) |
| `logging` | `LoggingConfig` | Logging configuration (see below) |

**LoggingConfig** (nested in `[logging]` TOML section):

| Field | Type | Description |
|-------|------|-------------|
| `level` | `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"` | Logging verbosity |
| `format` | `"text"` or `"json"` | Log output format |
| `log_file_path` | `String` | Path to log file. When set, structured logs are written to this file in addition to stdout/stderr. Strongly recommended for fail2ban integration in container deployments (see ADR-020). Default: not set (file logging disabled) |

**Note**: All log output uses `with_ansi(false)` to disable ANSI escape codes.
This is critical for fail2ban regex matching and Docker log output (see ADR-024).
Both text and JSON formats produce plain-text output without color codes.

**Note**: The entire `LoggingConfig` (including `log_file_path`) is static and
requires a process restart to change. Log file path changes require reopening
file handles, which is complex and low-value for Phase 1. Log rotation (Phase 2)
will be handled via signal-based or built-in rotation.

**ListenerConfig** (per-listener static config):

| Field | Type | Description |
|-------|------|-------------|
| `bind_addr` | `String` | IP address to bind to (must be explicit, no `0.0.0.0`; see ADR-016) |
| `http_port` | `u32` | Port for HTTP→HTTPS redirect (default: `80`; set to `0` to disable; valid values: 0 or 1–65535) |
| `https_port` | `u16` | Port for TLS listener (default: `443`) |
| `tls.mode` | `"acme"` or `"manual"` | Certificate provisioning mode |
| `tls.acme_domains` | `Vec<String>` | Domains for ACME SAN certificate (ACME mode only) |
| `tls.acme_cache_dir` | `String` | ACME state cache directory |
| `tls.acme_directory` | `"production"` or `"staging"` | Let's Encrypt directory |
| `tls.acme_contact` | `String` | Contact email for ACME registration (e.g., `"mailto:admin@example.com"`). Required for production; Let's Encrypt rejects registrations without a contact email. See OQ-10. |
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
| `upstream` | `String` | Upstream address. Supports Docker DNS (`gitea:3000`), loopback (`127.0.0.1:3000`), LAN IPs, and tunnel endpoints. No assumption about upstream locality (see ADR-020) |
| `upstream_scheme` | `"http"` or `"https"` | Protocol for upstream connection (default: `"http"`) |
| `upstream_connect_timeout_secs` | `u64` | TCP connect timeout in seconds (default: `5`; see ADR-015, ADR-017) |
| `upstream_request_timeout_secs` | `u64` | Full request timeout in seconds (default: `60`; see ADR-015, ADR-017) |

Sites are defined per listener in the `[[listeners]]` entries for organizational
purposes, but at runtime they are collected into a single global routing table
in `DynamicConfig`. The proxy looks up the `Host` header in this global table to
route requests. Hostnames must be unique across all listeners — a `Host` header
can only match one site definition, regardless of which listener received the
request. See ADR-019 for the rationale behind the `[[listeners]]` configuration
format.

**Why these are dynamic:** See ADR-008 for the rationale. Site definitions
and rate limits are per-request concerns that should not require restarting
the proxy or dropping active connections. Rate limits and body limits are
global settings in Phase 1; per-site configuration for these is deferred to
Phase 2.

### Default Values

| Field | Type | Default | Required |
|-------|------|---------|----------|
| `allow_wildcard_bind` | `bool` | `false` | No |
| `health_check_port` | `u16` | `9900` | No |
| `admin_socket_path` | `String` | `/run/reverse-proxy/admin.sock` | No |
| `shutdown_timeout_secs` | `u64` | `30` | No |
| `logging.level` | `String` | `"info"` | No |
| `logging.format` | `String` | `"text"` | No |
| `logging.log_file_path` | `String` | (not set) | No |
| `listeners[].http_port` | `u16` | `80` | No |
| `listeners[].https_port` | `u16` | `443` | No |
| `listeners[].tls.acme_directory` | `String` | `"production"` | No |
| `listeners[].tls.acme_contact` | `String` | — | Yes (ACME mode only) |
| `sites[].upstream_scheme` | `String` | `"http"` | No |
| `sites[].upstream_connect_timeout_secs` | `u64` | `5` | No |
| `sites[].upstream_request_timeout_secs` | `u64` | `60` | No |
| `rate_limit.requests_per_second` | `u32` | — | Yes |
| `rate_limit.burst` | `u32` | — | Yes |
| `body.limit_bytes` | `u64` | — | Yes |

Fields without defaults are required and must be specified in the config file.

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

### Static Config Changes During Reload

When the config file is reloaded (via SIGHUP or admin socket), the entire file
is read and validated — both static and dynamic portions. This provides early
error detection for misconfigurations that would prevent a restart from
succeeding.

If the full config fails validation, the reload is rejected and the old
DynamicConfig remains active.

If the full config passes validation but static fields have changed, the
DynamicConfig is swapped normally and a warning is logged listing the changed
static fields and noting that a restart is required for those changes to take
effect. This gives operators early feedback about config drift.

Only the DynamicConfig portion is swapped via ArcSwap. StaticConfig changes
require a process restart to take effect.

**Important**: The `ConfigReloadHandle` must track the last-known StaticConfig
so that it can correctly detect changes on subsequent reloads. After each
successful reload, the stored StaticConfig is updated with the new value (via
`ArcSwap<StaticConfig>` or similar interior mutability). This prevents stale
warnings: if the same static config change is present on two consecutive
reloads, the operator should see the warning only once, not on every reload.

### Reload Serialization

Reload operations are serialized using a `tokio::sync::Mutex` on the reload
code path. If a reload is in progress (triggered by SIGHUP or admin socket) and
a second reload is requested, the second request waits for the first to
complete, then re-reads the config file (getting the latest version) and
proceeds. This prevents race conditions where two concurrent reloads could apply
an older config over a newer one.

### Out of Scope: File Watching

Automatic file watching (inotify, fsnotify, etc.) is out of scope for Phase 1.
Config reload is triggered explicitly by SIGHUP or admin socket command. File
watching adds complexity (debouncing, handling atomic renames, handling editor
swap files) that is not justified for a single-instance proxy with infrequent
config changes.

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
# log_file_path = "/var/log/reverse-proxy/access.log"  # Optional; always-on when set

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
acme_contact = "mailto:admin@alk.dev"

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
# log_file_path = "/var/log/reverse-proxy/access.log"  # Optional; always-on when set

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
acme_contact = "mailto:admin@alk.dev"

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
2. Each listener's `bind_addr` is not `0.0.0.0` unless `allow_wildcard_bind` is enabled. This can be enabled via config (`allow_wildcard_bind = true`) or CLI flag (`--allow-wildcard-bind`). Either source enables it — it is an OR relationship, not AND. The CLI flag does not override the config value; if either is set, wildcard binding is allowed.
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
10. Each listener's `bind_addr` and `http_port` combination must be unique
    (prevents bind-time errors, same as rule 3 for `https_port`)
11. Within a listener, `http_port` and `https_port` must differ
12. `https_port` must be 1–65535 (required — TLS needs a port)
13. `http_port` must be 0 (disabled) or 1–65535
14. `health_check_port` must not conflict with any listener's `http_port` or
    `https_port` on the same bind address
15. Site `host` values must not include a port number (e.g., `git.alk.dev`,
    not `git.alk.dev:443`)
16. Site `host` values must be valid hostnames (not IP addresses, not
    including ports). Hostnames are normalized to lowercase during validation.
17. `upstream` must be in `host:port` format where `port` is a required integer
    1–65535. Examples: `gitea:3000`, `127.0.0.1:3000`, `[::1]:3000`. Invalid
    examples: `gitea` (missing port), `http://gitea:3000` (includes scheme),
    `10.0.0.5` (missing port). The `upstream_scheme` field handles the protocol.
18. `upstream_scheme` values are case-sensitive: only `"http"` or `"https"`
    (lowercase). Default is `"http"`.
19. In ACME mode, `tls.acme_contact` must be a valid `mailto:` URI
    (e.g., `"mailto:admin@example.com"`). Let's Encrypt requires a contact
    email for production certificate requests.

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
| [020](decisions/020-container-deployment.md) | Container deployment model | Flexible upstream addressing; `allow_wildcard_bind` override for containers |

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). Key
questions affecting this document:

- ~~**OQ-04**: Should config reload support a Unix domain socket API in addition
  to SIGHUP?~~ (resolved — ADR-014: Unix domain socket admin API added)
- ~~**OQ-07**: Should per-site TLS overrides be supported for mixed ACME/manual
  domains?~~ (resolved — ADR-019: `[[listeners]]` with per-listener TLS config)