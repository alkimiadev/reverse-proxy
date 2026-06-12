---
id: fix/inflight-counter-increment
name: Fix InFlightCounter to increment before spawning task (C2 + drain interval)
status: completed
depends_on: []
scope: narrow
risk: medium
impact: component
level: implementation
review_findings: [C2]
---

## Description

`InFlightCounter::increment()` is never called anywhere in the codebase. The
`InFlightGuard` only decrements on drop. Since `count` stays at 0, the first
guard drop does `fetch_sub(1)` on an `AtomicUsize` with value 0, which wraps to
`usize::MAX`. `is_zero()` checks `count == 0`, which never becomes true again.
The drain logic in `drain_in_flight` always times out.

The spec (operations.md shutdown sequence) states: "each request **must**
increment the counter when it begins and decrement when it completes (via guard
drop). The increment must happen before the request task is spawned."

Additionally, the spec states the drain polls every 100ms, but the current
implementation uses 50ms. Align with the spec.

### Changes Required

**`src/server.rs`**:
- Fold the increment into `InFlightGuard::new()` so it's impossible to forget:
  ```rust
  impl InFlightGuard {
      fn new(counter: Arc<InFlightCounter>) -> Self {
          counter.increment();
          Self(counter)
      }
  }
  ```
- Update `serve_https_listener` to use `InFlightGuard::new(in_flight.clone())`
  instead of `InFlightGuard(in_flight.clone())`.
- Make `InFlightGuard`'s tuple struct private (if it isn't already) so callers
  must use `new()`.

**`src/server.rs` — `drain_in_flight`**:
- Change polling interval from 50ms to 100ms per the spec (operations.md):
  ```rust
  tokio::time::sleep(std::time::Duration::from_millis(100)).await;
  ```

## Acceptance Criteria

- [ ] `InFlightGuard::new()` calls `counter.increment()` before returning
- [ ] `InFlightGuard` is constructed via `new()` only, not the tuple struct
- [ ] `serve_https_listener` uses `InFlightGuard::new(in_flight.clone())`
- [ ] `drain_in_flight` polls every 100ms (not 50ms)
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/operations.md — shutdown sequence, in-flight counter
- docs/reviews/003-security-and-bug-review.md — C2 finding
- src/server.rs — InFlightCounter, InFlightGuard, drain_in_flight
- src/main.rs — drain_in_flight caller

## Notes

> The previous `fix/graceful-shutdown` task addressed the abort-vs-join logic
> but did not fix the increment bug. This task completes that work.

## Summary

> To be filled on completion