---
id: fix/admin-http-api
name: Replace Unix domain socket admin API with authenticated HTTP admin API (ADR-028)
status: open
depends_on: []
scope: broad
risk: high
impact: component
level: implementation
review_findings: [C1, C2, C3, W1, W3, W4, S1, S2, S3, S4, S5, S6]
adr: [028]
---

## Description

Replace the Unix domain socket admin API (`src/admin/socket.rs`) with
authenticated HTTP endpoints on the existing health check listener. This
eliminates the entire class of filesystem-based vulnerabilities identified in
security review #005 (C1 symlink race, C2 no authentication, C3 info leak, W1
no concurrency limit, W3 path validation, W4 is_socket_active side effect, and
S1–S6 suggestions).

ADR-028 defines the replacement design. The health check listener on
`127.0.0.1:9900` already runs an axum router. Admin endpoints are added behind
Bearer token authentication middleware.

### Changes Required

**Remove:**
- `src/admin/socket.rs` — entire file (826 lines of Unix socket code)
- `src/admin/mod.rs` — current re-exports (`AdminSocket`, `AdminSocketError`,
  `start_admin_socket`)

**Add:**
- `src/admin/auth.rs` — Bearer token middleware:
  - `AdminAuthConfig` struct holding `Option<String>` for the SHA-256 hash of
    the admin key (or `None` to disable admin endpoints)
  - `admin_auth_middleware` axum middleware that validates `Authorization:
    Bearer <token>` against the stored hash using `subtle::ConstantTimeEq`
  - Returns 404 when admin is disabled (empty `admin_key_path`), 401 on
    missing/wrong token, passes through on valid token
  - `load_admin_key(path: &str) -> Result<Option<[u8; 32]>, AdminKeyError>`
    function that reads the key file, hashes it with SHA-256, and returns
    the hash. Returns `None` if path is empty (disabled). Logs a warning
    and returns `None` if the file doesn't exist or is unreadable (admin
    endpoints disabled, process continues starting).
- `src/admin/handler.rs` — HTTP handlers:
  - `reload_handler(State<...>) -> Json<ReloadResponse>` — **POST** `/admin/reload`.
    Triggers `ConfigReloadHandle::reload()`, returns `{"status": "ok"}` or
    `{"status": "error", "message": "reload failed"}`. Generic error
    messages only; details logged server-side.
  - `status_handler(State<...>) -> Json<StatusResponse>` — **GET**
    `/admin/status`. Returns
    `{"status": "ok", "uptime_secs": N, "sites": N}`
  - `rotate_key_handler(State<...>) -> Json<RotateKeyResponse>` — **POST**
    `/admin/rotate-key`. Generates new 256-bit random key, returns plaintext
    in response, replaces stored hash in memory. Returns
    `{"status": "ok", "key": "<hex>"}`.

**Modify:**
- `src/admin/mod.rs` — re-export `AdminAuthConfig`, `AdminKeyError`,
  `admin_auth_middleware`, `load_admin_key`, and the handler functions
- `src/health.rs` — expand `health_router()` to `admin_router()` that nests
  admin routes under `/admin` with auth middleware. Merge into the health
  check listener. The full router becomes:
  ```
  /health     → health_handler (GET, no auth)
  /admin/*    → auth middleware → admin handlers (POST for state-changing, GET for read-only)
  ```
  The `start_health_check_listener` function signature changes to accept
  `Option<Arc<AdminAuthConfig>>` and `Arc<ConfigReloadHandle>` and
  `Arc<ArcSwap<[u8; 32]>>` for key rotation. If `AdminAuthConfig` is `None`,
  `/admin/*` routes return 404.
- `src/main.rs` — remove admin socket initialization entirely (lines 102-127).
  Add admin key loading step after config parsing:
  ```rust
  let admin_auth = if !static_config.admin_key_path.is_empty() {
      match admin::load_admin_key(&static_config.admin_key_path) {
          Ok(Some(hash)) => Some(Arc::new(AdminAuthConfig { admin_key_hash: hash })),
          Ok(None) => None, // disabled
          Err(e) => {
              warn!("admin key load failed, disabling admin endpoints: {}", e);
              None
          }
      }
  } else {
      None
  };
  ```
  Pass `admin_auth`, `reload_handle`, and `start_time` to
  `start_health_check_listener`.
- `src/config/static_config.rs` — replace `admin_socket_path` field with
  `admin_key_path`:
  ```rust
  #[serde(default = "default_admin_key_path")]
  pub admin_key_path: String,
  ```
  Default: `"/etc/reverse-proxy/admin-key"`. Empty string disables admin
  endpoints.
- `src/config/dynamic_config.rs` — `ConfigReloadHandle` gains
  `cli_allow_wildcard_bind: bool` field (see task `fix/wildcard-flag-reload`).
  No other changes needed — `reload()` method stays the same.
- `src/config/validation.rs` — add validation that `admin_key_path` is empty
  or an absolute path (no `..` traversal, no relative paths). This is a new
  validation rule.
- `Cargo.toml` — add `subtle` and `sha2` dependencies (already in overview.md)

**Tests:**
- Replace all `src/admin/socket.rs` tests with HTTP-based tests using
  `reqwest` (already a dev dependency). Test:
  - POST `/admin/reload` with valid Bearer token returns `{"status": "ok"}`
  - POST `/admin/reload` with wrong token returns 401
  - POST `/admin/reload` with no token returns 401
  - POST `/admin/reload` when admin disabled returns 404
  - GET `/admin/status` with valid token returns uptime and site count
  - POST `/admin/rotate-key` with valid token returns new key and updates stored
    hash
  - POST `/admin/rotate-key` subsequent requests use the new key (old key returns
    401)
  - GET `/health` always returns 200 regardless of auth state

**Deployment:**
- `deploy/docker-compose.yml` — remove `/run/reverse-proxy` socket volume,
  add `/etc/reverse-proxy/admin-key:/etc/reverse-proxy/admin-key:ro` volume
- `deploy/reverse-proxy.service` — remove any socket directory setup
- `deploy/README.md` — replace `socat` commands with `curl` examples

## Acceptance Criteria

- [ ] `src/admin/socket.rs` is deleted entirely
- [ ] `src/admin/auth.rs` implements Bearer token auth with constant-time
      comparison and SHA-256 hashing
- [ ] `src/admin/handler.rs` implements `/admin/reload` (POST),
      `/admin/status` (GET), `/admin/rotate-key` (POST)
- [ ] `src/health.rs` serves both `/health` (no auth) and `/admin/*`
      (auth required) on port 9900
- [ ] `src/config/static_config.rs` uses `admin_key_path` (not
      `admin_socket_path`)
- [ ] `src/main.rs` loads admin key at startup, passes auth config to
      health check listener
- [ ] Admin disabled (`admin_key_path` empty or file missing) → `/admin/*`
      returns 404
- [ ] Wrong/missing Bearer token → 401
- [ ] Error responses are generic (no filesystem paths, no config details)
- [ ] Full error details logged server-side only
- [ ] Key rotation works in-memory (new key replaces stored hash, old key
      rejected)
- [ ] Key rotation does not persist across restarts (documented behavior)
- [ ] SIGHUP reload continues to work unchanged
- [ ] All existing tests pass (minus deleted socket tests)
- [ ] New HTTP-based admin tests pass
- [ ] `cargo clippy` passes with no warnings
- [ ] Deployment files updated (docker-compose, systemd, README)

## References

- docs/architecture/decisions/028-admin-http-api.md — ADR-028
- docs/architecture/decisions/014-unix-socket-reload.md — superseded ADR
- docs/architecture/decisions/027-admin-socket-resource-limits.md — deprecated
- docs/architecture/operations.md — admin HTTP endpoint, key management
- docs/architecture/config.md — admin_key_path, StaticConfig
- docs/architecture/overview.md — crate dependencies, architecture diagram
- docs/reviews/005-admin-socket-security-review.md — C1, C2, C3, W1, W3, W4
- src/admin/socket.rs — code to remove
- src/health.rs — code to extend
- src/main.rs — admin socket init to remove/replace
- src/config/static_config.rs — field rename

## Notes

> This is the primary implementation task for the admin socket → HTTP API
> migration. It directly implements ADR-028 and resolves findings C1, C2, C3,
> W1, W3, W4, S1–S6 from security review #005.
>
> W2 (config TOCTOU) and W5 (wildcard flag) are independent fixes tracked in
> separate tasks.
>
> The `subtle` and `sha2` crates are already listed in the architecture spec
> (overview.md crate dependencies). Add them to `Cargo.toml` with appropriate
> versions.

## Summary

> To be filled on completion