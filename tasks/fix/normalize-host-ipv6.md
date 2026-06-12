---
id: fix/normalize-host-ipv6
name: Fix normalize_host to handle IPv6 bracket notation
status: completed
depends_on: []
scope: single
risk: low
impact: isolated
level: implementation
review_findings: [S3]
---

## Description

`normalize_host` in `src/config/dynamic_config.rs` uses `split(':').next()` to strip ports, but this fails for IPv6 addresses in brackets (e.g., `[::1]:443` would normalize to `[::1]` instead of `::1`). The `strip_port_from_host` function in `src/tls/redirect.rs` correctly handles this case.

### Changes Required

**`src/config/dynamic_config.rs`**:
- Replace the `normalize_host` implementation with a port-stripping function that handles IPv6 bracket notation
- Either extract a shared utility function from `strip_port_from_host` in `redirect.rs`, or make `normalize_host` use the same logic
- The function should:
  - If host starts with `[`, find the closing `]` and strip the port after it
  - Otherwise, use the existing `split(':').next()` logic
  - Convert to lowercase

**`src/tls/redirect.rs`**:
- Consider extracting `strip_port_from_host` into a shared module (e.g., `src/utils.rs` or inline in both places) to avoid code duplication

### Tests

- Add test cases for `normalize_host` with IPv6:
  - `[::1]:443` → `::1` (or `[::1]` depending on convention — check what the routing table expects)
  - `[2001:db8::1]:8080` → `2001:db8::1`
  - Plain hostname with port: `example.com:443` → `example.com`
  - Plain hostname without port: `example.com` → `example.com`

## Acceptance Criteria

- [ ] `normalize_host` correctly strips ports from IPv6 bracket notation
- [ ] Either a shared utility function is extracted, or both implementations use the same logic
- [ ] New test cases for IPv6 host normalization
- [ ] Existing routing tests pass
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/002-implementation-review.md — S3 finding
- src/config/dynamic_config.rs — `normalize_host` function
- src/tls/redirect.rs — `strip_port_from_host` function (correct implementation)

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion