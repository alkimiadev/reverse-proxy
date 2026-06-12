---
id: fix/config-reload-static-drift
name: Fix ConfigReloadHandle static config drift causing stale diff warnings
status: pending
depends_on: []
scope: narrow
risk: medium
impact: component
level: implementation
review_findings: [C4]
---

## Description

Fix the `ConfigReloadHandle::reload()` method which computes a diff of static config fields but never updates the stored `StaticConfig`. This means every reload compares against the original static config (from process start), not the last-reloaded one, causing repeated "static config fields changed" warnings for the same fields on every reload.

The architecture spec in config.md explicitly states: "The `ConfigReloadHandle` must track the last-known StaticConfig so that it can correctly detect changes on subsequent reloads. After each successful reload, the stored StaticConfig is updated with the new value."

### Changes Required

**`src/config/dynamic_config.rs`**:
- Change `ConfigReloadHandle.static_config` from `StaticConfig` to `ArcSwap<StaticConfig>`
- Update `ConfigReloadHandle::new()` to wrap the static config in `ArcSwap`
- In `reload()`, after computing the diff, store the new static config: `self.static_config.store(Arc::new(new_static))`
- Add `pub fn static_config(&self) -> Arc<StaticConfig>` accessor if needed by other code
- Update `diff_static_config` to work with references (it already does — just need to use `self.static_config.load()` instead of `&self.static_config`)

**Tests**:
- Update `ConfigReloadHandle` tests to verify that a second reload only reports changes since the first reload, not changes since startup
- Verify that the `static_config_diff_detects_changes` test still works with `ArcSwap<StaticConfig>`

### Implementation Note

The `ArcSwap<StaticConfig>` approach is already used for `DynamicConfig` in the same struct. The pattern is well-established in the codebase. `ArcSwap` provides lock-free reads (important since reloads are rare but reads could be concurrent), and `Arc<StaticConfig>` is cheap to clone for comparison purposes.

## Acceptance Criteria

- [ ] `ConfigReloadHandle.static_config` is `ArcSwap<StaticConfig>` (not plain `StaticConfig`)
- [ ] After a successful reload, `self.static_config` is updated with the new value
- [ ] A second reload with no further static config changes reports an empty diff
- [ ] A second reload with different static config changes reports only the new changes
- [ ] Existing reload tests pass
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/config.md — Static config changes during reload, ArcSwap pattern
- docs/reviews/002-implementation-review.md — C4 finding
- src/config/dynamic_config.rs — ConfigReloadHandle implementation

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion