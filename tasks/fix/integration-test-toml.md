---
id: fix/integration-test-toml
name: Fix double-nested listeners.listeners.sites in integration test TOML
status: completed
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [S10]
---

## Description

The `write_valid_config` helper in `tests/integration_test.rs:514` contains `[[listeners.listeners.sites]]` which is a double-nested TOML path that doesn't match the config schema. The correct key is `[[listeners.sites]]`.

The test passes because it's used as an invalid config fixture (the binary validates and exits with error), but the function name `write_valid_config` suggests it was intended to produce valid config.

### Changes Required

**`tests/integration_test.rs`**:
- Fix the TOML key from `[[listeners.listeners.sites]]` to `[[listeners.sites]]`
- If the function was intended to produce valid config, verify the test still passes with the corrected TOML
- If the function is deliberately producing invalid config for a validation test, rename it to `write_invalid_config` or add a comment explaining the intentional invalidity

## Acceptance Criteria

- [ ] TOML nesting is corrected to `[[listeners.sites]]`
- [ ] Test function name matches its purpose (valid or invalid config)
- [ ] All integration tests pass
- [ ] `cargo test` passes

## References

- docs/reviews/002-implementation-review.md — S10 finding
- tests/integration_test.rs — `write_valid_config` helper

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion