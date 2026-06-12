---
id: fix/logging-test-global-subscriber
name: Fix logging test that conflicts with global tracing subscriber
status: completed
depends_on: []
scope: single
risk: low
impact: isolated
level: implementation
review_findings: [W9]
---

## Description

The test `init_creates_log_directory_and_file` in `src/logging/mod.rs` calls `init()` which sets a global default tracing subscriber. When tests run in parallel, this conflicts with other tests that may also set a subscriber, causing the test to fail with "a global default trace dispatcher has already been set."

### Changes Required

**`src/logging/mod.rs`**:
- Change `init()` or the test to use `tracing_subscriber::util::SubscriberInitExt::try_init()` which returns an error if already set rather than panicking
- Alternatively, use `std::sync::OnceLock` to guard against double-initialization in tests
- The production code should still use `init()` (which panics on double-init — that's correct behavior for a production binary), but the test should handle the case where a subscriber is already set

### Approach

The cleanest approach is to make `init()` return `Result<()>` and use `try_init()` internally. If initialization fails because a subscriber is already set, return an error rather than panicking. This way:
- Production code: `init()` is called once at startup; if it fails, that's a real error
- Tests: Can call `init()` in a test helper and gracefully handle the "already initialized" case

Alternatively, keep `init()` as-is and only change the test to use `try_init()` directly.

## Acceptance Criteria

- [ ] Logging test no longer panics when run in parallel with other tests
- [ ] Production `init()` behavior is unchanged (sets global subscriber)
- [ ] All tests pass (including parallel `cargo test`)
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/002-implementation-review.md — W9 finding
- src/logging/mod.rs — `init()` function and `init_creates_log_directory_and_file` test

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion