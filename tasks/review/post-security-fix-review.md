---
id: review/post-security-fix-review
name: Review security fix implementations before production consideration
status: pending
depends_on:
  - fix/rate-limiter-ip-source
  - fix/inflight-counter-increment
  - fix/connector-timeout-ceiling
  - fix/json-format-without-logfile
  - fix/upstream-host-validation
  - fix/acme-contact-validation
  - fix/upstream-uri-error-handling
  - fix/admin-socket-resource-limits
  - fix/consolidate-config-types
  - fix/rate-limiter-connectinfo-tests
scope: moderate
risk: low
impact: project
level: review
---

## Description

Review all security and bug fix implementations from Review #003 before
considering them production-ready. Verify that the fixes correctly implement
the architecture decisions (ADR-025, ADR-026, ADR-027) and the updated spec
documents.

## Acceptance Criteria

- [ ] C1 fix: Rate limiter uses ConnectInfo only, rejects without it (ADR-025)
- [ ] C2 fix: InFlightCounter increments before task spawn, drain polls 100ms
- [ ] C3 fix: Connector ceiling is 30s, per-site timeouts work >5s (ADR-026)
- [ ] C4 fix: JSON format applied in stdout-only path
- [ ] W1 fix: Upstream host part validated (DNS name or IP, IPv6 brackets)
- [ ] W2 fix: ACME contact email validated (non-empty, contains @)
- [ ] W3 fix: URI parse failure returns 502, never drops query string silently
- [ ] W4 fix: Admin socket has 5s timeout and 4096 byte line limit (ADR-027)
- [ ] W6 fix: RawConfig eliminated, FullConfig used in both paths
- [ ] S10 fix: Rate limit tests use ConnectInfo, verify XFF is ignored
- [ ] All `cargo test` passes
- [ ] All `cargo clippy` passes with no warnings
- [ ] No regressions in integration tests

## References

- docs/reviews/003-security-and-bug-review.md — all findings
- docs/architecture/decisions/025-rate-limiter-ip-source.md — ADR-025
- docs/architecture/decisions/026-connector-timeout-ceiling.md — ADR-026
- docs/architecture/decisions/027-admin-socket-resource-limits.md — ADR-027

## Notes

> This review covers the critical security fixes and the sensitive config
> consolidation. It should be the last task before the generation 4+ code
> quality items are considered final.

## Summary

> To be filled on completion