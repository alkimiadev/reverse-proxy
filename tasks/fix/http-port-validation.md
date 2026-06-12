---
id: fix/http-port-validation
name: Add http_port range validation (0 or 1-65535)
status: completed
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [S1]
---

## Description

The `http_port` validation only checks for port conflicts and whether `http_port > 0` (for the conflict check). It doesn't validate that `http_port` is in the valid range: 0 (disabled) or 1-65535. A value like `65536` or `-1` would pass validation incorrectly. There's already a `HttpsPortInvalid` error for https_port, but no equivalent for http_port.

### Changes Required

**`src/config/validation.rs`**:
- Add a validation check after the existing `http_port` conflict logic:
  ```rust
  if listener.http_port > 65535 {
      errors.push(ValidationError::HttpPortInvalid {
          bind_addr: listener.bind_addr.clone(),
          http_port: listener.http_port,
      });
  }
  ```
- Note: `http_port = 0` is already treated as "disabled" and should be allowed, so only check `> 65535`

**Tests**:
- Add a test for `http_port = 65536` producing `HttpPortInvalid`
- Add a test for `http_port = 0` still being valid (disabled)

## Acceptance Criteria

- [ ] `http_port > 65535` produces a validation error
- [ ] `http_port = 0` (disabled) remains valid
- [ ] `http_port` in 1-65535 remains valid
- [ ] New `HttpPortInvalid` error variant with descriptive message
- [ ] Existing validation tests pass
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/002-implementation-review.md — S1 finding
- src/config/validation.rs — existing validation logic

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion