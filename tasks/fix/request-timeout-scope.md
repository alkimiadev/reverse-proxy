---
id: fix/request-timeout-scope
name: Document request timeout scope and add Server header removal comment
status: pending
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [S8]
---

## Description

Review finding S8 notes that the request timeout applies to the entire HTTP exchange, not just the connection or first-byte response. For large file downloads or slow upstreams, this means a 60-second timeout kills the response even if the upstream is actively sending data.

The review marks this as "acceptable for Phase 1" and the timeout name (`upstream_request_timeout_secs`) is documented as applying to the full request. However, the code should clearly document this behavior so it's not surprising.

### Changes Required

**`src/proxy/handler.rs`**:
- Add a comment above the timeout wrapping in `proxy_handler`:
  ```rust
  // The timeout covers the entire HTTP round-trip including response body
  // streaming. For large file downloads or slow upstreams, this means the
  // timeout kills the response even if the upstream is actively sending data.
  // A more precise timeout would apply only to connect + first-byte, then
  // stream the body without a timeout. The `upstream_connect_timeout_secs`
  // field in SiteConfig exists for a separate connect timeout (see W4).
  // For Phase 1, this full-request timeout is acceptable.
  ```

## Acceptance Criteria

- [ ] Comment documents the timeout scope
- [ ] No code behavior change
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/002-implementation-review.md — S8 finding
- src/proxy/handler.rs — timeout wrapping

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion