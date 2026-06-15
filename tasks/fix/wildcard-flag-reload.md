---
id: fix/wildcard-flag-reload
name: Store cli_allow_wildcard_bind in ConfigReloadHandle for consistent reload validation (ADR-030)
status: open
depends_on: []
scope: narrow
risk: low
impact: component
level: implementation
review_findings: [W5]
adr: [030]
---

## Description

When the proxy starts with `--allow-wildcard-bind` (or `allow_wildcard_bind =
true` in config), bind addresses using `0.0.0.0` are accepted. But on config
reload, `validate()` is called with `cli_allow_wildcard_bind: false` — a
hardcoded value in `ConfigReloadHandle::reload()`. This means a config that was
valid at startup will be rejected on reload because the flag that enabled
wildcard binding is not preserved.

ADR-030 specifies storing `cli_allow_wildcard_bind` in `ConfigReloadHandle` at
construction time and using the stored value during reload validation.

### Changes Required

**`src/config/dynamic_config.rs`** — `ConfigReloadHandle` struct:
- Add `cli_allow_wildcard_bind: bool` field
- Update `ConfigReloadHandle::new()` to accept and store the flag:
  ```rust
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
  ```
- In `reload()`, pass `self.cli_allow_wildcard_bind` to `validate()` instead
  of `false`:
  ```rust
  validate(&new_static, &new_dynamic, self.cli_allow_wildcard_bind)?;
  ```

**`src/main.rs`** — Update `ConfigReloadHandle::new()` call to pass
`cli_allow_wildcard_bind` from the loaded config:
```rust
let reload_handle = Arc::new(ConfigReloadHandle::new(
    config_arc.clone(),
    loaded_config.static_config.clone(),
    loaded_config.cli_allow_wildcard_bind,  // or args.allow_wildcard_bind
));
```

The `cli_allow_wildcard_bind` value should be the OR of the config flag and
the CLI flag, matching the startup validation logic. Check `src/cli.rs` for
how the flag is currently handled.

**`src/admin/socket.rs`** (or `src/admin/handler.rs` after migration) — Same
change: pass the flag through to `ConfigReloadHandle::new()`.

**`src/config/validation.rs`** — No changes needed; `validate()` already
accepts `cli_allow_wildcard_bind: bool` and uses it correctly.

**Tests** — Update all `ConfigReloadHandle::new()` calls to include the new
parameter. Add a test that verifies:
1. A config with `0.0.0.0` bind address is accepted on reload when
   `cli_allow_wildcard_bind: true`
2. A config with `0.0.0.0` bind address is rejected on reload when
   `cli_allow_wildcard_bind: false`

## Acceptance Criteria

- [ ] `ConfigReloadHandle` has a `cli_allow_wildcard_bind: bool` field
- [ ] `ConfigReloadHandle::new()` accepts and stores `cli_allow_wildcard_bind`
- [ ] `reload()` passes `self.cli_allow_wildcard_bind` to `validate()`
      (not hardcoded `false`)
- [ ] All `ConfigReloadHandle::new()` call sites pass the correct flag
- [ ] Config with `0.0.0.0` bind address is accepted on reload when flag is
      true (test)
- [ ] Config with `0.0.0.0` bind address is rejected on reload when flag is
      false (test)
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/decisions/030-wildcard-flag-consistency.md — ADR-030
- docs/reviews/005-admin-socket-security-review.md — W5 finding
- docs/architecture/config.md — validation rules, allow_wildcard_bind
- src/config/dynamic_config.rs — ConfigReloadHandle
- src/config/validation.rs — validate()
- src/cli.rs — CLI flag handling

## Notes

> This fix is independent of the admin socket → HTTP migration. It should be
> applied to `ConfigReloadHandle` regardless of which admin interface is used.
> The implementation is straightforward: add a field, pass it through.
>
> The flag value should be `allow_wildcard_bind || cli_allow_wildcard_bind`
> (OR logic) matching the startup behavior documented in config.md.

## Summary

> To be filled on completion