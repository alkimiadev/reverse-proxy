---
id: fix/admin-socket-reload-mutex-visibility
name: Gate AdminSocket::reload_mutex with #[cfg(test)] (W11)
status: completed
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [W11]
---

## Description

`AdminSocket::reload_mutex()` is a public method that exists solely for the
`test_reload_serialized_with_mutex` test. It exposes an internal synchronization
primitive, and the test acquires the mutex before sending a reload command —
coupling the test to implementation details.

### Changes Required

**`src/admin/socket.rs`**:
- Gate `reload_mutex()` with `#[cfg(test)]`:
  ```rust
  #[cfg(test)]
  pub fn reload_mutex(&self) -> Arc<Mutex<()>> {
      self.reload_mutex.clone()
  }
  ```
- The existing test `test_reload_serialized_with_mutex` already uses this
  method, so it will continue to work.

## Acceptance Criteria

- [ ] `reload_mutex()` is only available in test builds (`#[cfg(test)]`)
- [ ] The `test_reload_serialized_with_mutex` test still compiles and passes
- [ ] `cargo clippy` passes with no warnings in non-test build

## References

- docs/reviews/003-security-and-bug-review.md — W11 finding
- src/admin/socket.rs — `AdminSocket::reload_mutex()`, test

## Notes

> The review suggests alternatively removing the method entirely and testing
> serialization through observable behavior. For Phase 1, gating with
> `#[cfg(test)]` is the simpler fix that preserves the existing test.

## Summary

> To be filled on completion