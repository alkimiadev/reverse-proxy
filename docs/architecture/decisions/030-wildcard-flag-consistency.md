# ADR-030: Store cli_allow_wildcard_bind Flag in ConfigReloadHandle

## Status

Accepted

## Context

When the proxy starts, `cli_allow_wildcard_bind` can be set to `true` via
the `--allow-wildcard-bind` CLI flag or the `allow_wildcard_bind = true` config
option. The startup validation uses this flag to decide whether `0.0.0.0` bind
addresses are allowed.

However, when a config reload is triggered (via SIGHUP or admin HTTP), the
`validate()` call is invoked with `cli_allow_wildcard_bind: false` — hardcoded
in `ConfigReloadHandle::reload()`. This means that a config that was accepted
at startup (because the CLI flag was set) will be rejected on reload, even
though the running process has `allow_wildcard_bind = true` in effect.

Security review #005 identified this as finding W5. The consequence is that
an operator who started the proxy with `--allow-wildcard-bind` cannot reload
the config without getting a validation error about `0.0.0.0` bind addresses
— even though those bind addresses are currently active and working.

## Decision

Store the `cli_allow_wildcard_bind` flag in `ConfigReloadHandle` at
construction time, and use the stored value during reload validation instead
of hardcoding `false`.

```rust
pub struct ConfigReloadHandle {
    config: Arc<ArcSwap<DynamicConfig>>,
    static_config: ArcSwap<StaticConfig>,
    reload_mutex: Mutex<()>,
    cli_allow_wildcard_bind: bool,
}

impl ConfigReloadHandle {
    pub fn new(
        config: Arc<ArcSwap<DynamicConfig>>,
        static_config: StaticConfig,
        cli_allow_wildcard_bind: bool,
    ) -> Self {
        Self {
            config,
            static_config: ArcSwap::from_pointee(static_config),
            reload_mutex: Mutex::new(()),
            cli_allow_wildcard_bind,
        }
    }
}
```

In `reload()`, pass `self.cli_allow_wildcard_bind` to `validate()` instead of
`false`:

```rust
validate(&new_static, &new_dynamic, self.cli_allow_wildcard_bind)?;
```

This ensures reload validation uses the same flag as startup validation. The
flag is immutable — it's set once at startup and never changed — so storing it
in `ConfigReloadHandle` is safe.

## Rationale

- **Consistency**: Startup and reload should apply the same validation rules.
  If `0.0.0.0` was allowed at startup, it should be allowed on reload.
- **The flag is immutable**: `cli_allow_wildcard_bind` is set once from CLI
  args and never changes. Storing it in `ConfigReloadHandle` is a simple,
  correct solution.
- **No config file change needed**: The `allow_wildcard_bind` config option is
  already in `StaticConfig`. The CLI flag is a separate override. The fix is
  purely in how the reload path uses the flag.
- **OR logic preserved**: The validation uses OR logic (`config_flag ||
  cli_flag`). If either is true, wildcard binds are allowed. This is unchanged.

## Consequences

**Positive:**
- Config reload will no longer reject valid configs that were accepted at
  startup due to the `--allow-wildcard-bind` CLI flag.
- Consistent validation between startup and reload paths.

**Negative:**
- `ConfigReloadHandle::new()` gains an additional parameter. This is a minor
  API change but affects all construction sites.
- The flag cannot be changed at runtime. If an operator wants to remove
  `--allow-wildcard-bind`, they must restart the process. This is correct
  behavior — wildcard bind is a security-sensitive setting that should
  require a restart.

## References

- [config.md](../config.md) — Validation rules, allow_wildcard_bind
- [Review #005](../../reviews/005-admin-socket-security-review.md) — W5 finding
- `src/config/dynamic_config.rs` — ConfigReloadHandle, reload()
- `src/config/validation.rs` — validate()