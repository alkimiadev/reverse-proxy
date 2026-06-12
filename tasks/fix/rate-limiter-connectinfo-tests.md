---
id: fix/rate-limiter-connectinfo-tests
name: Update rate limiter tests to use ConnectInfo instead of X-Forwarded-For (S10)
status: completed
depends_on: [fix/rate-limiter-ip-source]
scope: narrow
risk: medium
impact: component
level: implementation
review_findings: [S10]
---

## Description

After `fix/rate-limiter-ip-source` removes X-Forwarded-For parsing from the rate
limiter, existing integration tests that pass client IPs via `X-Forwarded-For`
headers will no longer work correctly. The rate limiter now reads exclusively
from `ConnectInfo<SocketAddr>`, so tests must provide `ConnectInfo` in request
extensions.

This task also addresses review finding S10: "No test verifies the ConnectInfo
extraction path, which is the primary path after C1 is fixed."

### Changes Required

**`tests/integration_test.rs`**:
- Find all rate limit integration tests (lines 90-163 and any others) that set
  `X-Forwarded-For` headers for rate limiting purposes
- Update those tests to set `ConnectInfo<SocketAddr>` in request extensions
  instead of (or in addition to) `X-Forwarded-For`
- Verify that `X-Forwarded-For` headers are now **ignored** by the rate limiter
  — add a test that sends requests with different `X-Forwarded-For` values from
  the same `ConnectInfo` IP and confirms they all count against the same bucket
- Add a test that verifies requests **without** `ConnectInfo` are rejected with
  429 (per ADR-025)

**`src/rate_limit/mod.rs`** (test module):
- Update any unit tests that rely on `X-Forwarded-For` extraction
- Add a test that verifies `ConnectInfo`-based rate limiting works without any
  `X-Forwarded-For` header

## Acceptance Criteria

- [ ] All rate limit integration tests use `ConnectInfo` for client IP
- [ ] Tests verify `X-Forwarded-For` headers are ignored by rate limiter
- [ ] New test: requests without `ConnectInfo` are rejected with 429
- [ ] New test: different `X-Forwarded-For` values from same `ConnectInfo` IP
      count against the same bucket (proving XFF is ignored)
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/decisions/025-rate-limiter-ip-source.md — ADR-025
- docs/reviews/003-security-and-bug-review.md — S10 finding
- tests/integration_test.rs — rate limit tests
- src/rate_limit/mod.rs — rate limiter middleware and tests

## Notes

> This task depends on `fix/rate-limiter-ip-source` because the code changes
> must be in place before the tests can be updated. Attempting this before the
> C1 fix would require writing tests for the old (broken) behavior.

## Summary

> To be filled on completion