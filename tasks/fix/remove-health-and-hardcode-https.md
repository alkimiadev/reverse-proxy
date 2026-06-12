---
id: fix/remove-health-and-hardcode-https
name: Remove /health from main listener and hardcode X-Forwarded-Proto to https
status: pending
depends_on: []
scope: narrow
risk: low
impact: component
level: implementation
review_findings: [W5, W14]
adr: [022]
---

## Description

Two related changes that simplify the proxy handler:

1. **W5 / ADR-022**: Remove the `/health` route from the main HTTPS listener. Health checking is an operational concern served exclusively by the dedicated local port (9900) and the admin socket's `status` command. Serving `/health` on the main listener creates collision with upstream applications and requires special-case routing before host matching. The route must be removed entirely — no configurable path replacement.

2. **W14**: Remove the `is_https` field from `ProxyState` and hardcode `X-Forwarded-Proto: https` in `inject_proxy_headers`. The proxy only proxies requests on the HTTPS listener (which always uses TLS), and the HTTP redirect listener sends 301 redirects rather than proxying. The `is_https` field was always `true` and was a latent bug for non-TLS contexts. Since there is no non-TLS proxying path, hardcoding `"https"` with a clear comment is correct.

### Changes Required

**`src/proxy/handler.rs`**:
- Remove `health_handler` function
- Remove the `/health` early return in `proxy_handler` (lines 37-39)
- Remove `is_https` field from `ProxyState` struct
- Remove `is_https` parameter from `proxy_router()` — it's no longer needed
- Remove the `/health` route from `proxy_router()` — `Router::new().fallback(proxy_handler).with_state(state)` instead of `Router::new().route("/health", get(health_handler)).fallback(proxy_handler).with_state(state)`

**`src/proxy/headers.rs`**:
- Change `inject_proxy_headers` signature to remove `is_https: bool` parameter
- Hardcode `X-Forwarded-Proto` to `"https"` with a comment explaining why:
  ```
  // X-Forwarded-Proto is always "https" because this proxy only forwards requests
  // received on the TLS listener. The HTTP listener redirects to HTTPS and does not
  // proxy requests, so X-Forwarded-Proto is never set for HTTP connections.
  ```

**`src/main.rs`**:
- Remove `is_https: true` from `ProxyState` initialization
- Update any calls to `proxy_router()` or `build_router()` that pass `is_https`

**`src/proxy/mod.rs`**:
- Update `build_router` signature if it references `is_https`

**Tests**:
- Remove `health_path_returns_200_regardless_of_host` test
- Remove `health_with_unknown_host_returns_200` test
- Update `make_proxy_state` helper — remove `is_https` field
- Update `inject_proxy_headers` tests — remove `is_https` parameter, verify `X-Forwarded-Proto` is always `"https"`
- The health check endpoint on port 9900 remains independently tested in `src/health.rs`

## Acceptance Criteria

- [ ] No `/health` route on the main HTTPS listener
- [ ] `ProxyState` struct no longer has `is_https` field
- [ ] `inject_proxy_headers` always sets `X-Forwarded-Proto: https` (no parameter)
- [ ] Code comment explains why `X-Forwarded-Proto` is always `"https"`
- [ ] `proxy_handler` has no special-case path matching before Host lookup
- [ ] Port 9900 health check (`src/health.rs`) is unchanged and working
- [ ] All existing tests pass (minus removed health-on-main-listener tests)
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/decisions/022-health-check-scope.md — ADR-022
- docs/architecture/proxy.md — updated request flow (no /health route)
- docs/architecture/operations.md — health check is port 9900 only
- docs/reviews/002-implementation-review.md — W5, W14 findings
- docs/architecture/open-questions.md — OQ-08, OQ-11

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion