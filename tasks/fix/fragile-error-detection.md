---
id: fix/fragile-error-detection
name: Replace fragile string matching for incomplete message error detection
status: pending
depends_on: []
scope: single
risk: low
impact: isolated
level: implementation
review_findings: [W3]
---

## Description

In `src/server.rs:95-97`, the check `e.to_string().contains("incomplete message")` silently suppresses connection errors by matching on the error description string. This is fragile — it can break across hyper versions, locale changes, or error message reformatting.

### Changes Required

**`src/server.rs`**:
- Check if hyper exposes a typed error variant for client-disconnect errors. If `hyper::Error::is_incomplete_message()` exists or a similar method is available, use that instead of string matching.
- If no typed variant exists, add a clear comment explaining why string matching is used and which version(s) of hyper produce this message:
  ```rust
  // Client disconnected before completing the request. hyper returns
  // "incomplete message" errors for this case, which we suppress since
  // it's not an error condition from the proxy's perspective. This is
  // checked via string matching because hyper doesn't expose a typed
  // variant for this error. Verified with hyper v1.x.
  if e.to_string().contains("incomplete message") {
      return;
  }
  ```

## Acceptance Criteria

- [ ] Error detection uses typed matching if available in hyper's API
- [ ] If string matching is kept, a comment documents why and which hyper version produces the message
- [ ] Existing connection error suppression still works
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/002-implementation-review.md — W3 finding
- src/server.rs — connection error handling

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion