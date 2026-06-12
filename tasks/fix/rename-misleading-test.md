---
id: fix/rename-misleading-test
name: Rename misleading health check test and fix dynamic config test (W8, W9)
status: completed
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [W8, W9]
---

## Description

Two test quality issues in `tests/integration_test.rs`:

**W8**: `test_health_check_disabled_when_port_zero` is misleading — port 0 means
"OS picks a random port" not "disabled". The test verifies that
`start_health_check_listener(0)` works (binds to a random port), which is
correct but the name implies it tests the disable path. The actual disable
logic is in `main.rs:94` where `health_check_port > 0` gates the call.

**W9**: `test_dynamic_config_with_limit` constructs `DynamicConfig` directly
with `routing_table: Default::default()` (empty HashMap), bypassing
`DynamicConfig::from_sites()`. The body limit tests only read
`body.limit_bytes` so it doesn't matter, but it's a latent trap for anyone
copying this pattern.

### Changes Required

**`tests/integration_test.rs`**:
- Rename `test_health_check_disabled_when_port_zero` to
  `test_health_check_binds_random_port_when_zero`
- Update `test_dynamic_config_with_limit` to use `DynamicConfig::from_sites()`
  instead of directly constructing with an empty routing table, OR add a
  comment explaining why the routing table is intentionally empty for this test

## Acceptance Criteria

- [ ] Test renamed to `test_health_check_binds_random_port_when_zero`
- [ ] `test_dynamic_config_with_limit` either uses `from_sites()` or has a
      comment explaining the intentional empty routing table
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/003-security-and-bug-review.md — W8, W9 findings
- tests/integration_test.rs — test names and implementations

## Notes

> To be filled on completion

## Summary

> To be filled on completion