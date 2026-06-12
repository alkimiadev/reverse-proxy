---
id: fix/connector-timeout-ceiling
name: Raise connector timeout ceiling to 30s per ADR-026
status: pending
depends_on: []
scope: single
risk: low
impact: component
level: implementation
review_findings: [C3]
---

## Description

The HTTP connector's `set_connect_timeout` is hardcoded to 5 seconds. Per-site
connect timeout values > 5s are silently capped because the connector's internal
timeout fires before the `tokio::time::timeout` wrapper.

ADR-026 establishes a 30-second ceiling on the connector. The per-site
`tokio::time::timeout` enforces the actual per-site connect timeout. The
connector ceiling is a safety backstop, not the primary enforcement mechanism.

### Changes Required

**`src/proxy/handler.rs`**:
- Change `DEFAULT_CONNECT_TIMEOUT_SECS` from 5 to 30:
  ```rust
  const CONNECT_TIMEOUT_CEILING_SECS: u64 = 30;
  ```
- Rename the constant to make its role clear (ceiling, not the default connect
  timeout for sites).
- Update both `create_http_client` and `create_https_client` to use the renamed
  constant.

## Acceptance Criteria

- [ ] Connector `set_connect_timeout` is set to 30 seconds
- [ ] Constant is named to reflect its ceiling role (not "default")
- [ ] Per-site connect timeout (default 5s) via `tokio::time::timeout` is unchanged
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/decisions/026-connector-timeout-ceiling.md — ADR-026
- docs/architecture/proxy.md — Upstream connection section
- docs/reviews/003-security-and-bug-review.md — C3 finding
- src/proxy/handler.rs — `create_http_client`, `create_https_client`

## Notes

> The previous `fix/connect-timeout` task wired the two-phase timeout approach.
> This task completes that work by raising the connector ceiling so per-site
> timeouts > 5s actually work.

## Summary

> To be filled on completion