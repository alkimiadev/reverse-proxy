---
status: draft
last_updated: 2026-06-14
reviewed_code:
  - src/admin/socket.rs
  - src/admin/mod.rs
  - src/main.rs
  - src/shutdown.rs
  - src/config/dynamic_config.rs
  - src/config/validation.rs
  - src/health.rs
  - src/proxy/handler.rs
  - src/proxy/headers.rs
  - src/proxy/mod.rs
  - src/rate_limit/mod.rs
  - src/server.rs
reviewer: code-reviewer
based_on: docs/reviews/004-post-fix-review.md
---

# Admin Socket Security Review #005

## Purpose

Focused security review of `src/admin/socket.rs` and related code paths,
triggered by unusual traffic patterns observed on the file in both public and
self-hosted git. The review examines the Unix domain socket admin interface for
vulnerabilities including symlink attacks, privilege escalation, information
disclosure, and DoS vectors. Broader codebase was also surveyed for related
issues.

## Severity Definitions

| Severity | Meaning |
|----------|---------|
| **Critical** | Will cause incorrect behavior or security issues in production |
| **Warning** | Could cause issues under specific conditions or represents a missed edge case |
| **Suggestion** | Code quality, style, or minor improvement opportunity |

---

## Critical Findings

### C1. Symlink Race in `cleanup_stale_socket` Enables Arbitrary File Deletion

**File**: `src/admin/socket.rs:143-161`

**Problem**: `cleanup_stale_socket` checks whether a socket file exists and
whether another process is listening on it, then removes the file. Between the
`is_socket_active()` check (which connects to the socket) and the
`remove_file()` call, an attacker with local access can replace the socket file
with a symlink pointing to an arbitrary path (e.g., `/etc/passwd`, a critical
database file). The `remove_file()` call then follows the symlink and deletes
the target:

```rust
async fn cleanup_stale_socket(path: &str) -> Result<(), AdminSocketError> {
    let socket_path = Path::new(path);
    if !socket_path.exists() {
        return Ok(());
    }

    if is_socket_active(path).await {         // check
        // ...
        return Err(AdminSocketError::SocketInUse(path.to_string()));
    }

    warn!("removing stale socket file: {}", path);
    tokio::fs::remove_file(path)              // act — follows symlinks!
        .await
        .map_err(AdminSocketError::Io)
}
```

This is a classic TOCTOU (time-of-check/time-of-use) race. The window is small
but exploitable with local access, which is exactly the threat model for a Unix
domain socket (any local user can reach it).

Additionally, `is_socket_active` works by **actually connecting** to the socket
(`src/admin/socket.rs:163-165`). If the path is a symlink to another service's
Unix socket, this creates a real connection in that service, which is an
unintended side effect that could be logged or trigger behavior in the other
service.

**Solution**: Replace the check-then-remove sequence with a safe alternative:

1. Use `std::fs::metadata()` to verify the file is actually a socket before
   removal (sockets cannot be symlinked to — `metadata` does not follow
   symlinks when `std::fs::symlink_metadata` is used, and sockets are a
   distinct file type).

2. Alternatively, use `std::fs::remove_file` only after verifying with
   `symlink_metadata` that the file type is `FileType::is_socket()`:

```rust
async fn cleanup_stale_socket(path: &str) -> Result<(), AdminSocketError> {
    let socket_path = Path::new(path);
    if !socket_path.exists() {
        return Ok(());
    }

    let metadata = std::fs::symlink_metadata(path)
        .map_err(AdminSocketError::Io)?;

    if metadata.file_type().is_symlink() {
        warn!("admin socket path {} is a symlink, refusing to remove", path);
        return Err(AdminSocketError::BindFailed(
            "socket path is a symlink, refusing to remove".to_string()
        ));
    }

    if !metadata.file_type().is_socket() {
        warn!("admin socket path {} is not a socket file, refusing to remove", path);
        return Err(AdminSocketError::BindFailed(
            "path exists but is not a socket".to_string()
        ));
    }

    if is_socket_active(path).await {
        warn!("socket file {} exists and another process is listening; disabling admin socket", path);
        return Err(AdminSocketError::SocketInUse(path.to_string()));
    }

    warn!("removing stale socket file: {}", path);
    tokio::fs::remove_file(path)
        .await
        .map_err(AdminSocketError::Io)
}
```

This prevents both symlink attacks (refuses to remove symlinks) and accidental
removal of non-socket files.

---

### C2. Admin Socket Has No Access Control — Any Local User Can Trigger Reload

**File**: `src/admin/socket.rs:108-133,254-305`

**Problem**: The admin socket accepts connections from any local user. There is
no authentication, no peer credential check, and no ownership/permission
restriction on the socket file itself. After `UnixListener::bind()`, the socket
inherits the process umask but no explicit restrictive permissions are set.

Any local user who can reach the socket path can:

1. **Trigger a config reload** (`reload` command) — re-reads the config file
   from disk and hot-swaps the live routing table. If an attacker can write to
   the config file path (e.g., via a separate misconfiguration or directory
   permission issue), they can chain this: write a malicious config, then send
   `reload` via the admin socket to activate it. This could redirect traffic
   to an attacker-controlled upstream.

2. **Read status** (`status` command) — reveals uptime and number of configured
   sites. Minor information disclosure but useful for reconnaissance.

The `reload` command is particularly dangerous because it reads the config file
from disk each time (`src/admin/socket.rs:257`). The config path is set at
startup from `StaticConfig` and cannot be changed at runtime, but the file
contents at that path can be modified by any process with write access. The
admin socket becomes a trigger mechanism for activating malicious configs.

**Solution**: Multi-layered defense:

1. Set restrictive permissions on the socket immediately after binding:

```rust
use std::os::unix::fs::PermissionsExt;

let listener = UnixListener::bind(socket_path)?;
let perms = std::fs::Permissions::from_mode(0o660); // owner + group only
std::fs::set_permissions(socket_path, perms)?;
```

   Or `0o600` for owner-only access. This requires the proxy to run under a
   dedicated user and for the admin tool (`socat`, etc.) to run as the same
   user or group.

2. Add peer credential checking using `SO_PEERCRED` on Linux:

```rust
use std::os::unix::net::UnixStream;

fn check_peer_uid(stream: &UnixStream) -> bool {
    use std::os::unix::io::AsRawFd;
    let uid = nix::sys::socket::getpeereid(stream.as_raw_fd())
        .map(|(_, uid, _)| uid);
    match uid {
        Ok(peer_uid) => peer_uid == 0 || peer_uid == get_current_uid(),
        Err(_) => false,
    }
}
```

3. Document that the socket path should be in a directory with restrictive
   permissions (e.g., `/run/reverse-proxy/` owned by the proxy user with mode
   `0700`).

---

### C3. Admin Socket `reload` Response Leaks Filesystem Paths and Error Details

**File**: `src/admin/socket.rs:257-265,268-276`

**Problem**: When the `reload` command fails, the error response includes the
full `std::io::Error` or `toml::de::Error` message, which can contain absolute
filesystem paths, file permissions, and internal config structure details:

```rust
Err(e) => {
    return serde_json::to_string(&ErrorResponse {
        status: "error",
        message: format!("failed to read config file: {}", e),  // leaks path
    })
    .unwrap();
}

Err(e) => {
    return serde_json::to_string(&ErrorResponse {
        status: "error",
        message: format!("failed to parse config file: {}", e),  // leaks structure
    })
    .unwrap();
}
```

The same applies to the `unknown command` response at line 247, which echoes
arbitrary input back without sanitization:

```rust
message: format!("unknown command: {}", command),
```

For an unauthenticated socket, this information disclosure helps an attacker
enumerate the system (filesystem layout, config syntax, software version via
error messages).

**Solution**: Return generic error messages to the socket client and log the
details server-side:

```rust
Err(e) => {
    tracing::error!(error = %e, "failed to read config file");
    serde_json::to_string(&ErrorResponse {
        status: "error",
        message: "reload failed".to_string(),
    }).unwrap()
}
```

For unknown commands, avoid echoing input:

```rust
_ => serde_json::to_string(&ErrorResponse {
    status: "error",
    message: "unknown command".to_string(),
}).unwrap(),
```

---

## Warning Findings

### W1. No Concurrency Limit on Admin Socket Connections

**File**: `src/admin/socket.rs:108-133`

**Problem**: Each accepted connection spawns a new tokio task (`tokio::spawn` at
line 114) with no concurrency limit. A local user with access to the socket can
open many simultaneous connections, spawning unlimited tasks. While the 5-second
read timeout and 4096-byte limit (added in review #004) mitigate the most
trivial DoS, a determined attacker can:

- Open many connections simultaneously, each sending data slowly (within the
  5-second timeout), consuming memory and task slots.
- Send data up to 4096 bytes per connection — with enough concurrent
  connections, this still consumes significant memory.

**Solution**: Add a `tokio::sync::Semaphore` with a reasonable limit (e.g., 10
concurrent connections):

```rust
let semaphore = Arc::new(tokio::sync::Semaphore::new(10));

// In the accept loop:
let permit = match semaphore.clone().acquire_owned().await {
    Ok(p) => p,
    Err(_) => {
        warn!("admin socket connection limit reached, dropping connection");
        continue;
    }
};
tokio::spawn(async move {
    let _permit = permit;
    handle_connection(stream, admin_socket).await;
});
```

---

### W2. Config File TOCTOU: Reload Reads File Without Atomicity

**File**: `src/admin/socket.rs:257`, `src/shutdown.rs:88`

**Problem**: Both `handle_reload` and `handle_sighup_reload` read the config
file with `tokio::fs::read_to_string()`, then parse it. If another process is
writing to the config file at the same time (e.g., a configuration management
tool like Ansible writing a partial file), the proxy could read a partially
written config and apply it. This is a filesystem-level TOCTOU issue.

Unlike the symlink race (C1), this is harder to exploit directly — the parse
will likely fail on a partial file, resulting in a reload error rather than a
bad config being applied. However, it could result in a brief window where the
operator sees confusing errors during config rotation.

**Solution**: Use atomic file replacement — write to a temporary file in the
same directory, then rename over the target. Document this pattern for
operators. Alternatively, compute a checksum or stat the file before and after
reading to detect mid-write changes:

```rust
let metadata_before = tokio::fs::metadata(&admin_socket.config_path).await?;
let config_content = tokio::fs::read_to_string(&admin_socket.config_path).await?;
let metadata_after = tokio::fs::metadata(&admin_socket.config_path).await?;

if metadata_before.modified()? != metadata_after.modified()? {
    return serde_json::to_string(&ErrorResponse {
        status: "error",
        message: "config file changed during read, please retry".to_string(),
    }).unwrap();
}
```

---

### W3. Admin Socket Path Is Not Validated or Sanitized

**File**: `src/admin/socket.rs:81-83`, `src/config/static_config.rs:22-24`

**Problem**: The `admin_socket_path` from the config file is used directly as a
filesystem path with no validation. A malicious or misconfigured path could
point to:

- A path on a critical filesystem (e.g., `/etc/passwd`)
- An extremely long path (potential buffer issues in downstream code)
- A path with directory traversal (e.g., `../../etc/cron.d/malicious`)

While the default `/run/reverse-proxy/admin.sock` is safe, the config is user-
controlled and loaded from disk. Combined with C2 (no authentication), a local
attacker who can write to the config file and trigger a reload could redirect
the admin socket to an arbitrary path.

This is partially mitigated by the fact that `admin_socket_path` is in
`StaticConfig` (requires restart, not hot-reloadable), but the startup config
still trusts the path.

**Solution**: Add validation that the socket path:

1. Ends with `.sock` or `.socket` (or at least doesn't end with a suspicious
   extension)
2. Is under a known-safe directory prefix (e.g., `/run/`, `/var/run/`, or a
   configurable base directory)
3. Does not contain path traversal components (`..`)
4. Has a reasonable length limit

```rust
fn validate_admin_socket_path(path: &str) -> Result<(), ValidationError> {
    if path.is_empty() { return Ok(()); } // disabled is valid
    if path.len() > 255 {
        return Err(ValidationError::AdminSocketPathTooLong);
    }
    if path.contains("..") {
        return Err(ValidationError::AdminSocketPathTraversal);
    }
    let path = Path::new(path);
    if path.is_absolute() {
        Ok(())
    } else {
        Err(ValidationError::AdminSocketPathRelative)
    }
}
```

---

### W4. `is_socket_active` Side-Effect on Other Processes

**File**: `src/admin/socket.rs:163-165`

**Problem**: This was flagged in review #003 (W7) and accepted as Phase 1.
However, in the context of the symlink attack in C1, this function becomes more
dangerous: if an attacker replaces the socket path with a symlink to another
service's socket, `is_socket_active` will connect to that service, which could
trigger behavior in the target service (e.g., accepting a connection, logging
it, starting a session). This amplifies the C1 symlink attack beyond file
deletion.

**Solution**: See C1 — the proposed fix uses `symlink_metadata()` and checks
`is_socket()` before calling `is_socket_active`, which eliminates the symlink
attack surface. Additionally, the `is_socket_active` check should only be
reached after verifying the file is not a symlink and is actually a socket
file type.

---

### W5. `reload` Command Does Not Validate Config Before Applying

**File**: `src/admin/socket.rs:268-279`, `src/config/dynamic_config.rs:134-158`

**Problem**: While `ConfigReloadHandle::reload()` calls `validate()` before
storing the new config, the validation does not check that the config file
itself hasn't been tampered with (e.g., via checksum). More critically, the
validation passes `cli_allow_wildcard_bind: false` during reload
(`validate(&new_static, &new_dynamic, false)` at line 141), but the startup
path may have passed `true` (via `--allow-wildcard-bind` CLI flag). This means
a config reload could tighten the wildcard bind restriction that was
intentionally relaxed via CLI, causing existing listeners on `0.0.0.0` to
continue running while the validation reports them as errors.

This is a corner case — the running listeners are not stopped — but it creates
a confusing state where the config is rejected on reload even though it was
accepted at startup.

**Solution**: Store the `cli_allow_wildcard_bind` flag in `ConfigReloadHandle`
or `StaticConfig` so that reload uses the same flag as startup:

```rust
pub struct ConfigReloadHandle {
    config: Arc<ArcSwap<DynamicConfig>>,
    static_config: ArcSwap<StaticConfig>,
    reload_mutex: Mutex<()>,
    cli_allow_wildcard_bind: bool,  // stored from startup
}
```

---

### W6. Reload Error Response Includes Static Config Change Warnings That Should Not Be Exposed

**File**: `src/admin/socket.rs:286-293`

**Problem**: On successful reload, if static config fields have changed, the
response includes a log at warning level but the admin socket only returns
`{"status": "ok"}`. However, the `reload_mutex` inside `ConfigReloadHandle`
already ensures only one reload runs at a time, but the `changed_fields` list
is only logged, not returned to the caller. An operator sending `reload` via
`socat` has no way to know if their reload actually changed static fields that
require a restart. This is an operational concern rather than a security issue.

**Solution**: Include `changed_fields` in the reload response when non-empty:

```rust
Ok(changed_fields) => {
    if !changed_fields.is_empty() {
        serde_json::to_string(&OkWithFieldsResponse {
            status: "ok",
            message: format!("static fields changed (restart required): {}", changed_fields.join(", ")),
        }).unwrap()
    } else {
        serde_json::to_string(&OkResponse { status: "ok" }).unwrap()
    }
}
```

---

### W7. Health Check Endpoint Exposes Service on Predictable Port

**File**: `src/health.rs:17-36`

**Problem**: The health check listener binds to `127.0.0.1` on the configured
port (default 9900). While binding to localhost is correct, the port is
predictable and not configurable to bind to a Unix socket instead. On shared
systems, any local process can connect to `127.0.0.1:9900/health` and confirm
the proxy is running. This is low risk (the response is just `200 OK` with no
body), but the endpoint is completely unauthenticated and could be used for
reconnaissance.

More importantly, there's no rate limiting on the health endpoint. A local
attacker could flood it to consume connection resources, though this is
mitigated by the fact that it's localhost-only and a simple GET.

**Solution**: Consider adding an option to bind the health check to a Unix
socket instead of a TCP port, or adding a shared-secret token header check.
For Phase 1, this is acceptable as-is since the endpoint is localhost-only and
returns minimal information.

---

## Suggestions

### S1. Add Connection Peer Logging to Admin Socket

**File**: `src/admin/socket.rs:112`

**Suggestion**: Log the peer credentials (UID, PID, GID) of each admin socket
connection using `SO_PEERCRED`. This provides an audit trail for reload
operations:

```rust
use std::os::unix::io::AsRawFd;

fn log_peer_credentials(stream: &tokio::net::UnixStream) {
    if let Ok((_, uid)) = nix::sys::socket::getpeereid(stream.as_raw_fd()) {
        info!(peer_uid = uid, "admin socket connection accepted");
    }
}
```

---

### S2. Use `AF_UNIX` Peer Credential Validation Instead of File Permissions

**File**: `src/admin/socket.rs`

**Suggestion**: Rather than (or in addition to) setting socket file permissions,
use `SO_PEERCRED`/`getpeereid` to validate the connecting process's UID at
accept time. This is more robust than filesystem permissions because:

1. It works regardless of the umask
2. It allows fine-grained control (e.g., allow root and the proxy user only)
3. It cannot be bypassed by group membership changes

Requires adding `nix` as a dependency (or using `libc` directly).

---

### S3. Set Socket File Permissions Immediately After Bind

**File**: `src/admin/socket.rs:90-102`

**Suggestion**: After the successful `UnixListener::bind()`, immediately set
restrictive permissions on the socket file:

```rust
let listener = UnixListener::bind(socket_path)?;
let perms = std::fs::Permissions::from_mode(0o660);
std::fs::set_permissions(socket_path, perms)?;
info!("admin socket listening on {} (mode 660)", socket_path);
```

This is the minimum defense-in-depth even if full peer credential checking
is not implemented in Phase 1.

---

### S4. Consider Adding a Simple Challenge-Response to the Admin Socket Protocol

**File**: `src/admin/socket.rs:167-252`

**Suggestion**: Even without full authentication, a simple shared-secret token
check would significantly raise the bar for local attackers. Add a `token`
field to `StaticConfig` (or an environment variable) and require it as a prefix
to commands:

```
<token>:reload\n
<token>:status\n
```

This prevents casual discovery of the socket path from enabling reload. The
token could be stored in an environment variable (`ADMIN_SOCKET_TOKEN`) that
the proxy reads at startup, keeping it out of the config file.

---

### S5. Move `is_socket_active` Check After File Type Verification

**File**: `src/admin/socket.rs:143-161`

**Suggestion**: This is the implementation detail of C1's fix. The current
order is:

1. Check if path exists
2. Check if socket is active (connect to it)
3. Remove the file

The safe order should be:

1. Check if path exists
2. Verify it's not a symlink (`symlink_metadata` + `is_symlink()`)
3. Verify it's a socket file type (`is_socket()`)
4. Check if socket is active
5. Remove the file

---

### S6. Document Admin Socket Security Model

**Suggestion**: Add a section to `docs/architecture/operations.md` (or similar)
that clearly states:

1. The admin socket is unauthenticated by design for Phase 1
2. The socket path must be in a directory with restrictive permissions
3. The socket file permissions should be `0600` or `0660`
4. Only trusted users should have filesystem access to the socket path
5. Future phases may add peer credential authentication or challenge-response

---

## Summary Statistics

| Severity | Count | Status |
|----------|-------|--------|
| Critical | 3 | Must fix before production |
| Warning | 7 | Should fix — security hardening |
| Suggestion | 6 | Consider for defense-in-depth |

## Recommended Fix Priority

1. **C1 (symlink race / arbitrary file deletion)** — Exploitable by any local
   user who can reach the socket directory. Fix with `symlink_metadata` + file
   type check before removal.
2. **C2 (no authentication / unrestricted socket permissions)** — Any local user
   can trigger config reload. Set socket permissions to 0o660 or 0o600 after
   bind as a minimum; add peer credential checking for full protection.
3. **C3 (error messages leak filesystem paths)** — Information disclosure on an
   unauthenticated socket. Return generic errors and log details server-side.
4. **W1 (no connection concurrency limit)** — DoS vector. Add a semaphore.
5. **W2 (config file read without atomicity)** — Partial-read risk during
   config rotation. Document atomic replacement pattern for operators.
6. **W3 (socket path not validated)** — Path traversal/symlink risk in config.
   Add basic validation.
7. **W4 (is_socket_active side effect)** — Amplifies C1. Fixed by C1's
   `symlink_metadata` guard.
8. **W5 (reload validation uses different wildcard flag)** — Inconsistency
   between startup and reload validation.
9. **Remaining W and S findings** — Fix opportunistically.

## Architectural Recommendation: Replace Unix Domain Socket with Authenticated HTTP Endpoint

### Problem

Every finding in this review (C1–C3, W1–W7, S1–S6) stems from the fundamental
design choice of using a Unix domain socket for the admin API. Unix sockets
bring an entire class of vulnerabilities that don't exist in HTTP endpoints:

- **Filesystem-based attack surface** — symlink races, stale socket cleanup,
  path traversal, permission management, directory mount issues in containers
- **No built-in authentication** — filesystem permissions are the only access
  control, which breaks in container environments (volume mounts, tmpfs quirks,
  umask variations) and provides no defense against any local user
- **Custom protocol** — line-based command protocol instead of standard HTTP,
  requiring `socat` or custom tooling instead of `curl`
- **Operational complexity** — socket cleanup on startup, stale socket detection,
  shutdown cleanup, container volume mounts for socket access

### Existing Infrastructure

The proxy already has a localhost-only HTTP listener (`src/health.rs`) bound to
`127.0.0.1:9900` that serves `/health`. This listener:

- Is already axum-based with a router
- Is already restricted to localhost
- Already supports middleware layers
- Already has integration tests

### Proposed Replacement

Replace the Unix domain socket admin API with authenticated HTTP endpoints on
the existing health check listener:

```
GET  http://127.0.0.1:9900/admin/reload  →  triggers config reload
GET  http://127.0.0.1:9900/admin/status   →  returns uptime + site count
```

Authentication via a Bearer token supplied at startup (config or env var
`ADMIN_KEY`), validated using constant-time comparison (`subtle` crate) to
prevent timing attacks. This is the same pattern used in
`/workspace/embedding_service/src/auth.rs`:

```rust
use subtle::ConstantTimeEq;

pub struct AdminAuthConfig {
    pub admin_key: Option<String>,  // None = admin endpoints disabled
}

pub async fn admin_auth_middleware(
    State(auth_config): State<Arc<AdminAuthConfig>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = auth_config.admin_key.as_deref().unwrap_or_default();
    let provided = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));

    match provided {
        Some(key) if key.as_bytes().ct_eq(expected.as_bytes()).into() => {
            request.headers_mut().remove(header::AUTHORIZATION);
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}
```

### Config Change

Replace `admin_socket_path` (StaticConfig) with `admin_key` (StaticConfig or
environment variable):

```toml
# Before
admin_socket_path = "/run/reverse-proxy/admin.sock"

# After
admin_key = ""   # empty = admin endpoints disabled (return 404)
                 # set to a token to enable admin endpoints
```

Or via environment variable:
```
ADMIN_KEY=your-secure-random-token-here
```

### Why This Eliminates Every Finding

| Finding | Socket-specific? | Eliminated by HTTP? |
|---------|-------------------|---------------------|
| C1 (symlink race) | Yes | No filesystem path management at all |
| C2 (no auth) | Yes | Bearer token with constant-time comparison |
| C3 (info leak) | Partially | Generic HTTP error responses, no paths |
| W1 (no conn limit) | Partially | axum/TCP backlog handles this naturally |
| W2 (config TOCTOU) | No | Still exists (same file read pattern) |
| W3 (path validation) | Yes | No socket path to validate |
| W4 (is_socket_active) | Yes | No stale socket detection needed |
| W5 (wildcard flag) | No | Still exists (reload validation) |
| W6 (changed_fields) | No | Still exists (operational UX) |
| W7 (health port recon) | Partially | Merged into admin auth |

### What Changes

**Removed:**
- `src/admin/socket.rs` — entire file (826 lines)
- `src/admin/mod.rs` — re-exports
- `admin_socket_path` config field
- Socket cleanup/stale detection logic
- `socat`-based operational procedures
- Docker volume mount for `/run/reverse-proxy`

**Added:**
- `src/admin/auth.rs` — Bearer token middleware with `subtle::ConstantTimeEq`
- `src/admin/handler.rs` — HTTP handlers for `/admin/reload` and `/admin/status`
- `admin_key` config field (or env var)
- Admin route registration on the health check listener

**Modified:**
- `src/health.rs` — add admin routes behind auth middleware
- `src/main.rs` — remove socket init, add admin route setup
- `src/config/static_config.rs` — replace `admin_socket_path` with `admin_key`
- `docs/architecture/decisions/014-unix-socket-reload.md` — superseded
- `deploy/docker-compose.yml` — remove socket volume mount
- `deploy/README.md` — replace `socat` commands with `curl` examples

**Unchanged:**
- `src/shutdown.rs` — SIGHUP reload path remains as-is
- `src/config/dynamic_config.rs` — `ConfigReloadHandle` still used by both
  SIGHUP and admin reload

### Operational Comparison

```bash
# Before (Unix socket)
echo "reload" | socat - UNIX-CONNECT:/run/reverse-proxy/admin.sock
echo "status" | socat - UNIX-CONNECT:/run/reverse-proxy/admin.sock

# After (HTTP with Bearer token)
curl -H "Authorization: Bearer $ADMIN_KEY" http://127.0.0.1:9900/admin/reload
curl -H "Authorization: Bearer $ADMIN_KEY" http://127.0.0.1:9900/admin/status
```

HTTP is universally understood by CI/CD tools, load balancer health checks,
and monitoring systems. No special client (`socat`) required.

### SIGHUP Still Works

This change does **not** remove SIGHUP as a reload mechanism. SIGHUP reload
(`src/shutdown.rs:handle_sighup_reload`) continues to work exactly as before.
The admin HTTP endpoint provides the same functionality with the addition of
structured feedback and authentication.

### Recommendation

Mark C1, C2, C3, W1, W3, W4, S1–S6 as **resolved by architectural change**.
Implement W2 and W5 as standalone fixes regardless. Schedule the socket-to-HTTP
migration as the primary implementation task for the next development round.

## Additional Notes

The unusual traffic on this file was likely driven by interest in C1 and C2.
The symlink race (C1) is the most directly exploitable vulnerability — it
requires only local filesystem access to the socket directory and timing to
replace the socket with a symlink. Combined with C2 (no authentication), a
local attacker who can write to the socket directory can both delete arbitrary
files (C1) and trigger config reloads with attacker-controlled content (C2 +
config file write access).