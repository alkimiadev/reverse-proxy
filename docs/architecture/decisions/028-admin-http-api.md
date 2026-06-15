# ADR-028: Authenticated HTTP Admin API (Replacing Unix Domain Socket)

## Status

Accepted

## Context

The proxy has a Unix domain socket admin API (ADR-014) that provides two
commands: `reload` (trigger config reload with success/failure feedback) and
`status` (return uptime and site count). Security review #005 identified three
critical and seven warning-level vulnerabilities in the socket implementation:

- **C1**: Symlink race in stale socket cleanup enables arbitrary file deletion
- **C2**: No authentication — any local user can trigger config reload
- **C3**: Error responses leak filesystem paths and config structure details
- **W1**: No connection concurrency limit
- **W3**: Socket path not validated or sanitized
- **W4**: `is_socket_active` side-effect on other processes
- **W5**: Reload validation uses different `cli_allow_wildcard_bind` flag than
  startup

These vulnerabilities stem from the fundamental design choice of using a Unix
domain socket. The socket introduces an entire class of filesystem-based attack
surface that does not exist with an HTTP endpoint: symlink races, stale socket
cleanup, path traversal, permission management, and directory mount issues in
containers.

Additionally, the socket requires `socat` for interaction — a non-standard tool
that must be installed separately, complicating container images and CI/CD
pipelines.

The proxy already has a localhost-only HTTP listener (`src/health.rs`) bound to
`127.0.0.1:9900` that serves `/health`. This listener is axum-based, supports
middleware layers, and has integration tests. Co-locating admin endpoints on
this listener is the natural replacement.

## Decision

Replace the Unix domain socket admin API with authenticated HTTP endpoints on
the existing health check listener. Authentication uses a Bearer token verified
against a SHA-256 hash stored in memory.

### Admin Key Management

The admin key is stored in a file on disk (specified by `admin_key_path` in
StaticConfig). The proxy reads this file once at startup, hashes its contents
with SHA-256, and stores only the hash in memory. The plaintext key is never
held in memory after startup initialization.

Key file setup:

```bash
openssl rand -hex 32 > /etc/reverse-proxy/admin-key
chmod 600 /etc/reverse-proxy/admin-key
```

Setting `admin_key_path` to an empty string disables admin endpoints entirely.

### Authentication

Admin endpoints require a Bearer token in the `Authorization` header:

```
Authorization: Bearer <key>
```

The provided token is SHA-256 hashed and compared against the stored hash using
constant-time comparison (`subtle::ConstantTimeEq`) to prevent timing attacks.

Error behavior by auth state:

| Scenario | Response |
|----------|----------|
| Admin disabled (`admin_key_path` empty) | 404 (endpoint does not exist) |
| Missing `Authorization` header | 401 |
| Wrong token | 401 |
| Correct token | Proceed to handler |

Returning 404 when admin is disabled prevents discovery of the endpoint's
existence. Returning 401 for wrong tokens (rather than 404) allows operators to
confirm the endpoint is available without revealing information to attackers
who lack any valid token.

### Endpoints

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| GET | `/health` | None | Health check (unchanged) |
| POST | `/admin/reload` | Bearer token | Trigger config reload |
| GET | `/admin/status` | Bearer token | Return uptime and site count |
| POST | `/admin/rotate-key` | Bearer token | Generate and return a new random admin key |

**`/admin/reload`** — Triggers the same config reload as SIGHUP. Returns
structured JSON:

```json
{"status": "ok"}
```

On error:

```json
{"status": "error", "message": "reload failed"}
```

Error messages are generic — no filesystem paths, no config structure details.
Full error information is logged server-side only.

**`/admin/status`** — Returns process information:

```json
{"status": "ok", "uptime_secs": 1234, "sites": 2}
```

**`/admin/rotate-key`** — Generates a new 256-bit random key using
`rand::RngCore`, returns it in the response, and replaces the stored hash in
memory with the SHA-256 hash of the new key:

```json
{"status": "ok", "key": "<new-plaintext-key-hex>"}
```

The operator should capture this key and update the key file on disk for
subsequent restarts. In-memory rotation does **not** persist across restarts —
on restart, the proxy re-reads the key file. This is by design: the file on
disk is the source of truth for the admin key, and runtime rotation is a
temporary override.

If an attacker has the current admin key, they could rotate it to lock out the
legitimate operator. But if an attacker has the admin key, they can already
trigger config reloads — the most dangerous operation. Rotation is strictly
less damaging than what they could already do.

### Config Change

Replace `admin_socket_path` (StaticConfig) with `admin_key_path` (StaticConfig):

```toml
# Before (ADR-014)
admin_socket_path = "/run/reverse-proxy/admin.sock"    # empty = disabled

# After (ADR-028)
admin_key_path = "/etc/reverse-proxy/admin-key"        # empty = disabled
```

Default: `"/etc/reverse-proxy/admin-key"`.

### Comparison with Previous Design

| Aspect | Unix Socket (ADR-014) | HTTP Admin (ADR-028) |
|--------|-----------------------|----------------------|
| Authentication | None (filesystem permissions only) | Bearer token with constant-time comparison |
| Attack surface | Filesystem: symlinks, stale cleanup, path traversal | File: read once at startup, no management |
| Client tool | `socat` (non-standard) | `curl` (universal) |
| Error leakage | Paths and config details in responses | Generic messages, details logged server-side |
| Container setup | Volume mount for socket directory | Volume mount for key file (single file, `:ro`) |
| Feedback | Structured JSON responses | Structured JSON responses (same) |
| SIGHUP fallback | Yes (both work) | Yes (both work) |

### What This Eliminates from Review #005

| Finding | Eliminated? | Reason |
|---------|------------|--------|
| C1 (symlink race) | Yes | No socket file management at all |
| C2 (no authentication) | Yes | Bearer token with constant-time comparison |
| C3 (info leak) | Yes | Generic error messages, no paths |
| W1 (no conn limit) | Yes | axum/TCP backlog handles this naturally |
| W3 (path validation) | Yes | No socket path to validate; key file path is read-only, no creation/cleanup |
| W4 (is_socket_active) | Yes | No stale socket detection needed |
| W5 (wildcard flag) | No | Still exists (separate fix) |
| W2 (config TOCTOU) | No | Still exists (separate fix) |

### Remaining Findings

W2 (config file TOCTOU on reload) and W5 (reload validation uses different
`cli_allow_wildcard_bind` flag) still apply to both the SIGHUP and HTTP admin
reload paths. These are independent of the admin interface choice and require
separate fixes.

## Rationale

- **Eliminates an attack surface class**: Every critical finding in review #005
  stems from the socket being a filesystem object. Removing the socket removes
  the class.
- **Read-once semantics**: The proxy reads the key file once at startup and
  never manages it — no creation, no cleanup, no stale detection. This is
  fundamentally different from the socket, which required bind, listen, accept,
  cleanup-on-startup, cleanup-on-shutdown, and stale detection.
- **Standard tooling**: `curl` is available everywhere. `socat` requires
  separate installation in container images and CI environments.
- **Authentication**: Bearer tokens are the standard pattern for HTTP APIs.
  Constant-time comparison prevents timing attacks. SHA-256 hashing means the
  plaintext key is never held in memory after startup.
- **Key file is lower-risk than socket**: Reading a file is a single syscall.
  The socket required managing a filesystem object across the entire process
  lifecycle. If an attacker can read the key file, they can also read the
  config file — the key file does not expand the trust boundary.

## Consequences

**Positive:**
- Eliminates C1, C2, C3, W1, W3, W4 from security review #005
- Authentication for admin operations (the socket had none)
- Universal client tooling (`curl` instead of `socat`)
- Simpler container setup (single file mount vs. directory mount)
- No socket lifecycle management (startup cleanup, shutdown cleanup, stale
  detection)
- Generic error responses prevent information disclosure

**Negative:**
- Key file must exist on disk for admin endpoints to work
- Key file must be readable by the proxy process
- In-memory key rotation does not persist across restarts (operator must
  update the key file separately)
- Adds `subtle` and `sha2` crate dependencies
- Admin endpoints share the health check port (operational port serves both
  authenticated and unauthenticated routes)

## References

- [operations.md](../operations.md)
- [config.md](../config.md)
- [overview.md](../overview.md)
- [ADR-014](014-unix-socket-reload.md) — Superseded by this ADR
- [ADR-027](027-admin-socket-resource-limits.md) — Deprecated (no longer needed)
- [Review #005](../../reviews/005-admin-socket-security-review.md)
- [Review #006](../../reviews/006-attack-surface-review.md)