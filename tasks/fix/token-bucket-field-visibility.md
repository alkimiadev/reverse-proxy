---
id: fix/token-bucket-field-visibility
name: Make TokenBucket fields private except last_access (W10, S6)
status: completed
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [W10, S6]
---

## Description

All `TokenBucket` fields are `pub` but only `last_access` is read externally (by
`evict_stale` in `rate_limit/mod.rs`). The other fields (`tokens`, `last_refill`,
`rate`, `max`) should be private to prevent accidental direct mutation that
bypasses `try_consume`/`refill` logic.

### Changes Required

**`src/rate_limit/bucket.rs`**:
- Make `tokens`, `last_refill`, `rate`, `max` private (remove `pub`)
- Keep `last_access` as `pub(crate)` for `evict_stale` access
- `TokenBucket::new()` already exists as a constructor, so no changes needed there
- Update any unit tests that directly access private fields. The tests in
  `bucket.rs` are in the same module so they have access to private fields.
  Tests in `mod.rs` may need adjustment if they access `bucket.tokens` etc.

## Acceptance Criteria

- [ ] `tokens`, `last_refill`, `rate`, `max` fields are private
- [ ] `last_access` is `pub(crate)`
- [ ] `new()` constructor is the only way to create a `TokenBucket` externally
- [ ] `evict_stale` still compiles and works (uses `last_access`)
- [ ] All unit tests pass (in-module tests can still access private fields)
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/003-security-and-bug-review.md — W10, S6 findings
- src/rate_limit/bucket.rs — TokenBucket struct
- src/rate_limit/mod.rs — evict_stale

## Notes

> To be filled on completion

## Summary

> To be filled on completion