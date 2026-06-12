---
id: fix/access-logging
name: Wire up request access logging in the proxy handler
status: pending
depends_on: []
scope: moderate
risk: medium
impact: component
level: implementation
review_findings: [W13]
---

## Description

The `log_request!` macro exists in `src/logging/format.rs` but is never called anywhere in the codebase. The architecture spec (operations.md) states: "Access logging is **always-on** — it is the primary observability mechanism for the proxy and is required for fail2ban integration. There is no configuration option to disable access logging."

Every proxied request must produce an access log line in the format:
```
REQUEST client_ip=203.0.113.50 host=git.alk.dev method=GET path=/user/repo status=200 upstream=127.0.0.1:3000 duration_ms=45
```

Additionally, upstream errors must produce `UPSTREAM_ERROR` log lines and rate-limited requests already produce `RATE_LIMIT` lines (those work correctly).

### Changes Required

**`src/proxy/handler.rs`**:
- Add `std::time::Instant` tracking at the start of `proxy_handler`
- Call `log_request!` macro on every proxied request (success path)
- Include: `client_ip`, `host`, `method`, `path`, `status`, `upstream`, `duration_ms`
- Call `log_upstream_error!` on upstream connection failures and bad gateway errors
- The `duration_ms` should measure from request entry to response sent

**`src/proxy/mod.rs`**:
- Ensure `log_request!` and `log_upstream_error!` macros are accessible (they should be via `crate::log_request!`)

### Log Format Details

From operations.md and the existing macro definitions:

**Access log** (every proxied request):
```
REQUEST client_ip=203.0.113.50 host=git.alk.dev method=GET path=/user/repo status=200 upstream=127.0.0.1:3000 duration_ms=45
```

**Upstream error** (connection refused, timeout, etc.):
```
UPSTREAM_ERROR host=git.alk.dev upstream=127.0.0.1:3000 error="connection refused"
```

The `log_upstream_error!` macro should be called in the error branches of `proxy_handler` where upstream connections fail or time out.

## Acceptance Criteria

- [ ] `log_request!` is called for every successfully proxied request
- [ ] `log_request!` is called for proxied requests that receive non-2xx upstream responses (4xx/5xx from upstream are still logged as access logs with the upstream status code)
- [ ] `log_upstream_error!` is called when upstream is unreachable (502)
- [ ] `log_upstream_error!` is called when upstream times out (504)
- [ ] `duration_ms` accurately measures request-to-response time
- [ ] Log format matches the `REQUEST` prefix format with `key=value` pairs
- [ ] Access logging is always-on — no configuration to disable it
- [ ] Existing tests pass
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/operations.md — logging section, access log format
- docs/reviews/002-implementation-review.md — W13 finding
- src/logging/format.rs — log_request!, log_upstream_error! macros
- docs/architecture/decisions/007-custom-log-format.md — log format rationale

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion