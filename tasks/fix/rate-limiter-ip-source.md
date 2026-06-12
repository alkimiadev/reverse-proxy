---
id: fix/rate-limiter-ip-source
name: Fix rate limiter to use ConnectInfo only (ADR-025)
status: pending
depends_on: []
scope: narrow
risk: high
impact: component
level: implementation
review_findings: [C1]
---

## Description

The rate limiter currently extracts the client IP by checking the `X-Forwarded-For`
header **first**, then falling back to `ConnectInfo`. This is a security
vulnerability: since the rate limiter runs as middleware before the proxy
handler injects trusted headers, the `X-Forwarded-For` value is whatever the
client sent — completely untrusted. This enables two attack vectors:

1. **Rate limit bypass**: Attacker sends each request with a different random
   `X-Forwarded-For` value, evading per-IP token buckets entirely.
2. **DoS via IP spoofing**: Attacker sends requests with `X-Forwarded-For:
   <victim IP>`, depleting the victim's bucket.

ADR-025 establishes that the rate limiter must use `ConnectInfo<SocketAddr>` as
the **sole** source of client IP. If `ConnectInfo` is absent, the request must
be **rejected** (not fall back to an untrusted header).

### Changes Required

**`src/rate_limit/mod.rs`**:
- Replace the current IP extraction logic in `rate_limit_middleware` (lines 66-76)
  with ConnectInfo-first, no fallback:
  ```rust
  let client_ip = req
      .extensions()
      .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
      .map(|ci| ci.ip());
  ```
- When `ConnectInfo` is absent, return 429 (reject the request) rather than
  passing through without rate limiting. Per ADR-025: "If ConnectInfo is absent,
  the request must be rejected rather than falling back to an untrusted header."
  ```rust
  let Some(ip) = client_ip else {
      return (StatusCode::TOO_MANY_REQUESTS, "Too Many Requests").into_response();
  };
  ```
- Remove all `X-Forwarded-For` header parsing from the rate limiter middleware.

## Acceptance Criteria

- [ ] Rate limiter extracts client IP from `ConnectInfo<SocketAddr>` only
- [ ] No `X-Forwarded-For` header parsing in the rate limiter middleware
- [ ] Requests without `ConnectInfo` are rejected with 429 (not passed through)
- [ ] `cargo test` passes (note: integration tests that pass `X-Forwarded-For`
      for rate limiting will need to be updated in the separate test task
      `fix/rate-limiter-connectinfo-tests`)
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/decisions/025-rate-limiter-ip-source.md — ADR-025
- docs/architecture/operations.md — Rate limiting design, IP source section
- docs/architecture/proxy.md — Rate limiter IP source section
- docs/reviews/003-security-and-bug-review.md — C1 finding
- src/rate_limit/mod.rs — current implementation (lines 61-104)

## Notes

> This is the highest-priority security fix. After this change, the integration
> tests in `tests/integration_test.rs` that rely on `X-Forwarded-For` for rate
> limiting will need to be updated. That work is tracked separately in
> `fix/rate-limiter-connectinfo-tests`.

## Summary

> To be filled on completion