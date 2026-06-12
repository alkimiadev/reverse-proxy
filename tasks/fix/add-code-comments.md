---
id: fix/add-code-comments
name: Add clarifying code comments for correct-but-non-obvious behaviors
status: completed
depends_on: [fix/remove-health-and-hardcode-https]
scope: narrow
risk: trivial
impact: component
level: implementation
review_findings: [C3, W8, W10, W11, S9]
---

## Description

Several review findings identified behaviors that are correct but need clarifying comments to prevent future "fixes" that would change correct behavior. This task adds documentation comments to these code locations.

### Comments to Add

**C3 — X-Forwarded-For replace semantics** (`src/proxy/headers.rs`):
Add a comment above `headers.insert(HeaderName::from_static("x-forwarded-for"), ip_value)`:
```rust
// X-Forwarded-For is SET (not appended) because this proxy is the outermost
// edge proxy. Any existing X-Forwarded-For from the client is untrusted and
// must be replaced with the actual client IP from ConnectInfo.
// See ADR-021 for the edge proxy model rationale.
```

**W8 — Server header removal** (`src/proxy/handler.rs`):
Add a comment above `parts.headers.remove("server")`:
```rust
// The upstream Server header is intentionally removed. As a security-focused
// reverse proxy, we hide upstream server identity as a defense-in-depth measure.
// The proxy does not add its own Server header either. See W8 in review #002.
```

**W10 — Body limit two-layer defense** (`src/proxy/body_limit.rs`):
Add a comment above the Content-Length check:
```rust
// Early rejection: if Content-Length is present and exceeds the limit, reject
// immediately without reading the body. For requests without Content-Length
// (chunked, HTTP/2), the Limited body wrapper below enforces the limit during
// streaming. This is a two-layer defense.
```

**W11 — Health check port conflict conservatism** (`src/config/validation.rs`):
Add a comment above the health check port conflict check:
```rust
// Health check always binds to 127.0.0.1 (hardcoded in src/health.rs), so this
// conflict check is conservative — it warns even when the health check port
// wouldn't actually conflict (e.g., health check on 127.0.0.1:80 vs listener
// on 203.0.113.10:80). This is acceptable for Phase 1.
```

**S9 — Rate limit bypass when no IP** (`src/rate_limit/mod.rs`):
Add a comment above the `return next.run(req).await` in the `None` branch:
```rust
// If no client IP can be identified, the request passes through without rate
// limiting. In practice, ConnectInfo is always set by the server's
// ConnectInfoService, so this branch is unreachable. If the proxy were ever
// deployed without ConnectInfo propagation, rate limiting would silently become
// a no-op. Consider adding a warning log or returning 429 in a future phase.
```

## Acceptance Criteria

- [ ] All five comments are added at the specified locations
- [ ] Comments reference the relevant ADRs or review findings where applicable
- [ ] Code behavior is unchanged (comments only)
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/002-implementation-review.md — C3, W8, W10, W11, S9 findings
- docs/architecture/decisions/021-x-forwarded-for-edge-proxy.md — ADR-021
- src/proxy/headers.rs — X-Forwarded-For insert
- src/proxy/handler.rs — Server header removal
- src/proxy/body_limit.rs — Content-Length check
- src/config/validation.rs — Health check port conflict
- src/rate_limit/mod.rs — No IP fallback

## Notes

> This task depends on fix/remove-health-and-hardcode-https because the X-Forwarded-Proto comment (W14) is being addressed in that task. The remaining comments here are independent.

## Summary

> To be filled on completion