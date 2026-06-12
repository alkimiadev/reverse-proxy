---
id: fix/token-bucket-nanosecond
name: Fix token bucket refill to use nanosecond precision
status: pending
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [W6]
---

## Description

The token bucket refill calculation in `src/rate_limit/bucket.rs` uses `as_millis()` which truncates sub-millisecond time. Two requests arriving 500µs apart both see `0ms` elapsed and don't refill tokens. This can lead to token refill inaccuracies under high-frequency request bursts.

### Changes Required

**`src/rate_limit/bucket.rs`**:
- Change line 37 from:
  ```rust
  let elapsed = now.duration_since(self.last_refill).as_millis() as f64;
  let tokens_to_add = (elapsed * rate) / 1000.0;
  ```
  to:
  ```rust
  let elapsed = now.duration_since(self.last_refill).as_nanos() as f64;
  let tokens_to_add = (elapsed / 1_000_000_000.0) * rate;
  ```

This provides nanosecond-precision refill while keeping the math in floating point.

### Tests

- Verify existing token bucket tests still pass
- The `token_bucket_refills_over_time` test already validates refill behavior; it may need a longer sleep to notice the difference with nanosecond precision

## Acceptance Criteria

- [ ] Token refill uses nanosecond precision (`as_nanos()`)
- [ ] Math is `(elapsed_nanos / 1_000_000_000.0) * rate`
- [ ] Existing token bucket tests pass
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/002-implementation-review.md — W6 finding
- src/rate_limit/bucket.rs — current millisecond-based refill

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion