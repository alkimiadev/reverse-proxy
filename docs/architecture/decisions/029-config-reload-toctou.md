# ADR-029: Config File TOCTOU Mitigation on Reload

## Status

Accepted

## Context

Both the SIGHUP reload path (`src/shutdown.rs:handle_sighup_reload`) and the
admin HTTP reload path (`src/admin/socket.rs:handle_reload`, soon
`src/admin/handler.rs`) read the config file from disk with
`tokio::fs::read_to_string()`, then parse and apply it. If another process is
writing to the config file at the same time (e.g., a configuration management
tool like Ansible writing a partial file), the proxy could read a partially
written config and either fail to parse it (resulting in a reload error) or,
in an unlikely worst case, parse a structurally valid but semantically wrong
config.

This is a filesystem-level time-of-check/time-of-use (TOCTOU) issue. The
window is small but the impact of applying a partial config is significant.

Security review #005 identified this as finding W2.

## Decision

Detect mid-write file changes by comparing file metadata before and after
reading. If the modification timestamp changes between the two `stat` calls,
reject the reload and return a retry message.

```rust
let metadata_before = tokio::fs::metadata(&config_path).await?;
let config_content = tokio::fs::read_to_string(&config_path).await?;
let metadata_after = tokio::fs::metadata(&config_path).await?;

if metadata_before.modified()? != metadata_after.modified()? {
    return Err("config file changed during read, please retry");
}
```

This applies to **both** the SIGHUP reload path and the admin HTTP reload path.

For operators, the documentation will recommend the atomic replacement pattern
(write to a temp file in the same directory, then `rename()` over the target).
This is the standard safe pattern for config file rotation and is what tools
like Ansible already do with `copy` module's `validate` parameter.

## Rationale

- **Simple and effective**: The mtime check catches the common case of a
  config management tool mid-write. It requires no changes to the config
  file format or directory layout.
- **No false negatives**: If mtime changed, the file definitely changed. If
  mtime did not change within the typical filesystem timestamp granularity
  (1 second on most Linux filesystems), the window is so small that a partial
  read is extremely unlikely.
- **Atomic rename is the gold standard**: Recommending it in documentation is
  better than trying to enforce it in code. The proxy can't control how
  operators write config files, but it can detect when a file might be
  inconsistent and ask for a retry.
- **Same pattern in both reload paths**: SIGHUP and admin HTTP share the same
  file-reading logic (or should — currently they duplicate it). This ADR
  ensures both paths are protected.

## Consequences

**Positive:**
- Config reload will reject a file that changed during the read, preventing
  partial or inconsistent configs from being applied.
- Clear error message ("config file changed during read, please retry") tells
  operators exactly what happened.
- Documenting the atomic replacement pattern gives operators a clear
  recommendation for safe config rotation.

**Negative:**
- In very rare cases, a legitimate config change that happens to land within
  the same filesystem timestamp granularity as the read could be falsely
  rejected. The operator would need to retry the reload, which is an
  acceptable trade-off for safety.
- The mtime check does not protect against all TOCTOU scenarios (e.g., a
  write that starts before the first `stat` and completes before the read).
  However, combined with the atomic replacement recommendation, this is a
  defense-in-depth measure, not a complete solution. A complete solution would
  require file locking or checksum verification, which adds complexity for
  marginal benefit.

## References

- [operations.md](../operations.md) — Config reload, admin HTTP endpoint
- [config.md](../config.md) — Config reload behavior
- [Review #005](../../reviews/005-admin-socket-security-review.md) — W2 finding