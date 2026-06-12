---
id: fix/consolidate-config-types
name: Delete RawConfig and use FullConfig in load_config (W6, S5)
status: pending
depends_on: []
scope: narrow
risk: medium
impact: component
level: implementation
review_findings: [W6, S5]
---

## Description

`RawConfig` (in `src/cli.rs`) and `FullConfig` (in `src/config/mod.rs`) have
identical fields and identical serde attributes. They exist because the initial
load path manually constructs `StaticConfig` + `SerializableDynamicConfig`,
while the reload path uses `FullConfig::into_static_and_dynamic()`. Any new
config field must be added in two places.

The fix is to delete `RawConfig` and use `FullConfig` in `load_config`. The
`collect_sites` helper can also be removed since `into_static_and_dynamic`
already collects sites from all listeners.

### Changes Required

**`src/cli.rs`**:
- Delete the `RawConfig` struct (lines 49-65)
- Rewrite `load_config` to use `FullConfig`:
  ```rust
  pub fn load_config(cli: &Cli) -> Result<LoadedConfig> {
      let config_path = Path::new(&cli.config);
      let config_content = std::fs::read_to_string(config_path)
          .with_context(|| format!("failed to read config file: {}", cli.config))?;

      let full_config = crate::config::FullConfig::parse(&config_content)
          .with_context(|| format!("failed to parse config file: {}", cli.config))?;

      let (static_config, dynamic_config) = full_config.into_static_and_dynamic();

      let allow_wildcard_bind = static_config.allow_wildcard_bind || cli.allow_wildcard_bind;

      validate(&static_config, &dynamic_config, cli.allow_wildcard_bind).map_err(|errors| {
          anyhow::anyhow!(
              "config validation failed:\n{}",
              errors.iter()
                  .map(|e| format!("  - {}", e))
                  .collect::<Vec<_>>()
                  .join("\n")
          )
      })?;

      Ok(LoadedConfig {
          static_config,
          dynamic_config,
          allow_wildcard_bind,
      })
  }
  ```
- Delete the `collect_sites` helper function (lines 112-118)
- Remove the now-unused imports: `SerializableDynamicConfig`, `BodyConfig`,
  `RateLimitConfig` (if they're only used via `RawConfig`)

**`src/config/mod.rs`**:
- No changes needed — `FullConfig` already has `into_static_and_dynamic()`
- Verify `FullConfig::parse` and `into_static_and_dynamic` produce identical
  results to the old `RawConfig` path

## Acceptance Criteria

- [ ] `RawConfig` struct deleted from `src/cli.rs`
- [ ] `collect_sites` function deleted from `src/cli.rs`
- [ ] `load_config` uses `FullConfig::parse` + `into_static_and_dynamic`
- [ ] Startup config loading produces same results as before
- [ ] Config validation still runs on both startup and reload
- [ ] All existing `cli.rs` tests pass
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/config.md — config reload, FullConfig
- docs/reviews/003-security-and-bug-review.md — W6, S5 findings
- src/cli.rs — RawConfig, load_config, collect_sites
- src/config/mod.rs — FullConfig, into_static_and_dynamic

## Notes

> This changes the startup config parsing path. While the behavior should be
> identical (same fields, same serde attributes), this is a sensitive area. A
> review task follows the code quality generation.

## Summary

> To be filled on completion