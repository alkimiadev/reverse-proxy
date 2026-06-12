---
id: fix/http-port-type-u16
name: Change http_port type from u32 to u16 per spec (W12)
status: completed
depends_on: []
scope: narrow
risk: low
impact: component
level: implementation
review_findings: [W12]
---

## Description

`http_port` is declared as `u32` in `ListenerConfig` but `https_port` is `u16`.
Both represent TCP port numbers (valid range 1–65535). The type inconsistency
means comparisons require casting (`listener.http_port == listener.https_port as
u32`) and `http_port` could theoretically hold values > 65535 caught by
validation rather than the type system.

The spec (config.md) now declares `http_port` as `u16`.

### Changes Required

**`src/config/static_config.rs`**:
- Change `http_port` field type from `u32` to `u16` in `ListenerConfig`
- Update `default_http_port()` return type to `u16`

**`src/config/validation.rs`**:
- Change `DuplicateHttpBind` error type: `http_port: u32` → `http_port: u16`
- Change `HttpsAndHttpPortSame` error type: `http_port: u32` → `http_port: u16`
- Change `HttpPortInvalid` error type: `http_port: u32` → `http_port: u16`
- Remove `as u32` casts — both `http_port` and `https_port` are now `u16`
- Remove the `http_port > 65535` check (impossible with `u16`, but keep `http_port > 0`
  for the "disabled" check)
- Update comparison: `listener.http_port == listener.https_port` (no cast needed)
- Update health check port comparison: remove `as u32` cast

**`src/main.rs`**:
- Update any `http_port` references that assume `u32`

**`src/cli.rs`**:
- Update `RawConfig.http_port` type from `u32` to `u16` (if `RawConfig` still
  exists after `fix/consolidate-config-types`; if not, this file is unaffected)

**`src/config/test_fixtures.rs`**:
- Update any test fixture `http_port` values from `u32` to `u16`

**`tests/integration_test.rs`**:
- Update any hardcoded `http_port` values

## Acceptance Criteria

- [ ] `http_port` is `u16` in `ListenerConfig`
- [ ] All `as u32` casts on `http_port` removed
- [ ] `http_port > 65535` validation check removed (impossible with u16)
- [ ] `http_port == https_port` comparison works without casting
- [ ] All validation tests pass
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/config.md — `http_port` type declaration
- docs/reviews/003-security-and-bug-review.md — W12 finding
- src/config/static_config.rs — `ListenerConfig` struct
- src/config/validation.rs — validation rules, error types

## Notes

> If `fix/consolidate-config-types` runs first and removes `RawConfig`, the
> `src/cli.rs` changes in this task are reduced. The two tasks are independent
> in terms of the type change itself.

## Summary

> To be filled on completion