---
id: fix/config-reload-toctou
name: Add mtime check to config reload to detect mid-write file changes (ADR-029)
status: completed
depends_on: []
scope: narrow
risk: low
impact: component
level: implementation
review_findings: [W2]
adr: [029]
---

## Description

Both the SIGHUP reload path (`src/shutdown.rs:handle_sighup_reload`) and the
admin HTTP reload path (`src/admin/socket.rs:handle_reload`, soon
`src/admin/handler.rs`) read the config file from disk with
`tokio::fs::read_to_string()`, then parse and apply it. If another process is
writing to the config file at the same time, the proxy could read a partially
written config.

ADR-029 specifies a simple mitigation: compare file modification timestamps
before and after reading. If mtime changed, reject the reload and return a
"please retry" message.

### Changes Required

**Shared reload function** — Extract the common file-read-and-validate logic
from `src/shutdown.rs:handle_sighup_reload()` and
`src/admin/socket.rs:handle_reload()` into a shared function (e.g.,
`src/config/dynamic_config.rs` or a new `src/config/reload.rs`):

```rust
pub async fn read_and_validate_config(
    config_path: &str,
    cli_allow_wildcard_bind: bool,
) -> Result<(StaticConfig, DynamicConfig), ReloadError> {
    let metadata_before = tokio::fs::metadata(config_path).await
        .map_err(ReloadError::Io)?;
    let config_content = tokio::fs::read_to_string(config_path).await
        .map_err(ReloadError::Io)?;
    let metadata_after = tokio::fs::metadata(config_path).await
        .map_err(ReloadError::Io)?;

    if metadata_before.modified().ok() != metadata_after.modified().ok() {
        return Err(ReloadError::FileChangedDuringRead);
    }

    let full_config = FullConfig::parse(&config_content)?;
    let (new_static, new_dynamic) = full_config.into_static_and_dynamic();
    validate(&new_static, &new_dynamic, cli_allow_wildcard_bind)?;

    Ok((new_static, new_dynamic))
}
```

**`src/shutdown.rs`** — Replace inline file read + parse + validate with a
call to `read_and_validate_config()`. On `ReloadError::FileChangedDuringRead`,
log a warning: "config file changed during read, please retry SIGHUP".

**`src/admin/handler.rs`** (after admin-http-api task) — Same call. On
`ReloadError::FileChangedDuringRead`, return
`{"status": "error", "message": "config file changed during read, please retry"}`.

**Error type** — Define `ReloadError` enum with variants:
- `Io(std::io::Error)`
- `Parse(toml::de::Error)`
- `Validation(String)`
- `FileChangedDuringRead`

## Acceptance Criteria

- [ ] Both SIGHUP and admin HTTP reload paths use the same file-reading logic
- [ ] mtime is checked before and after reading the config file
- [ ] If mtime changed, reload is rejected with a clear error message
- [ ] Error message in admin HTTP response is generic ("config file changed
      during read, please retry") — no filesystem paths leaked
- [ ] Full error details are logged server-side (path, mtime values)
- [ ] SIGHUP path logs the same error at warn level
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/decisions/029-config-reload-toctou.md — ADR-029
- docs/reviews/005-admin-socket-security-review.md — W2 finding
- src/shutdown.rs — handle_sighup_reload
- src/admin/socket.rs — handle_reload (to be replaced by admin/handler.rs)

## Notes

> This fix is independent of the admin socket → HTTP migration. It applies to
> both reload paths (SIGHUP and admin). The implementation should be done
> after or alongside the admin-http-api task since that task replaces
> socket.rs with handler.rs.

## Summary

Implemented `read_and_validate_config()` in `src/config/mod.rs` with mtime check
before/after reading. Both SIGHUP and admin HTTP reload paths use this shared
function. `ReloadError::FileChangedDuringRead` returns HTTP 409 Conflict from
the admin endpoint and logs a warning on SIGHUP. 7 new unit tests added.