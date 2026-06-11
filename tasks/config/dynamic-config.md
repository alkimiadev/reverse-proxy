---
id: config/dynamic-config
name: Implement DynamicConfig with ArcSwap hot-reload and ConfigReloadHandle
status: completed
depends_on: [config/static-config]
scope: moderate
risk: medium
impact: component
level: implementation
---

## Description

Implement the dynamic configuration that can be hot-reloaded at runtime without restarting the process. This is the core of the config reload mechanism.

### DynamicConfig

Hot-reloadable at runtime via `ArcSwap`. Changes take effect for new connections immediately.

- `sites: Vec<SiteConfig>` — hostname → upstream mapping (collected from all listeners)
- `rate_limit: RateLimitConfig` — `requests_per_second: u32`, `burst: u32`
- `body_limit_bytes: u64` — max request body size

**RateLimitConfig**:
- `requests_per_second: u32` — required, > 0
- `burst: u32` — required, > 0

### ArcSwap Pattern

- `Arc<ArcSwap<DynamicConfig>>` provides lock-free reads on the request hot path
- `ConfigReloadHandle` with `reload(new_config)` method atomically swaps the entire config
- No partial updates — the entire DynamicConfig is swapped at once
- All request handlers read current config via `Arc` dereference (no lock contention)

### Reload Flow

1. Read the TOML config file from disk
2. Deserialize into full config (both static and dynamic portions)
3. Validate the full config (catches static misconfigurations early)
4. If valid, swap DynamicConfig via ArcSwap; log warnings for any static changes
5. If invalid, reject the reload and keep the old DynamicConfig

### Reload Serialization

Use `tokio::sync::Mutex` on the reload code path. If a reload is in progress and a second is requested, the second waits, re-reads the config file (getting the latest), then proceeds.

## Acceptance Criteria

- [ ] `DynamicConfig` struct defined with `sites`, `rate_limit`, and `body_limit_bytes` fields
- [ ] `RateLimitConfig` struct defined with `requests_per_second` and `burst`
- [ ] `Arc<ArcSwap<DynamicConfig>>` used for lock-free reads in handlers
- [ ] `ConfigReloadHandle` struct with `reload(DynamicConfig)` method
- [ ] Reload serialization via `tokio::sync::Mutex` prevents concurrent reload race conditions
- [ ] Static config change detection: if static fields differ from current, log warning listing changed fields
- [ ] Unit tests for ArcSwap swap (verify new config visible after reload)
- [ ] Unit tests for reload rejection on invalid config
- [ ] Unit tests for concurrent reload serialization

## References

- docs/architecture/config.md — DynamicConfig, ArcSwap pattern, reload flow
- docs/architecture/decisions/008-static-dynamic-config-split.md — ArcSwap rationale

## Notes

> The sites vector is collected from all listeners into a single global routing table. Hostname uniqueness validation happens in the validation step, not in DynamicConfig itself.

## Summary

> To be filled on completion