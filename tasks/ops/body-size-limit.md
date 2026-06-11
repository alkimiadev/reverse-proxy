---
id: ops/body-size-limit
name: Implement global request body size limit with axum DefaultBodyLimit middleware
status: pending
depends_on: [config/dynamic-config]
scope: single
risk: trivial
impact: isolated
level: implementation
---

## Description

Implement the global request body size limit using axum's `DefaultBodyLimit` middleware. The default limit is 100 MB (104,857,600 bytes), matching the current nginx configuration and accommodating Gitea's push operations with large pack files (ADR-018).

### Implementation

- Set `DefaultBodyLimit::max(body_limit_bytes)` as axum middleware
- `body_limit_bytes` comes from `DynamicConfig`, so it can be changed at runtime via config reload
- When the limit is exceeded, axum returns `413 Payload Too Large` with `Payload Too Large` body
- In Phase 1, the limit is global (not per-site)

### Config Reload

Since `body_limit_bytes` is in `DynamicConfig`, it updates on config reload. However, axum's `DefaultBodyLimit` is typically set as a layer at router construction time. The implementation needs to ensure the current limit is read from `DynamicConfig` on each request, not cached at router construction time.

This may require a custom middleware that reads `DynamicConfig` via `ArcSwap` on each request, rather than relying solely on axum's `DefaultBodyLimit`.

## Acceptance Criteria

- [ ] Body size limit enforced on all proxied requests
- [ ] Default: 100 MB (104,857,600 bytes)
- [ ] 413 Payload Too Large response when limit exceeded
- [ ] Limit is configurable via `DynamicConfig`
- [ ] Limit can be changed at runtime via config reload
- [ ] Config value is read from ArcSwap on each request (not cached)
- [ ] Integration test: request with body > limit receives 413
- [ ] Integration test: request with body < limit succeeds

## References

- docs/architecture/proxy.md — body size limit section
- docs/architecture/config.md — DynamicConfig, body_limit_bytes
- docs/architecture/decisions/018-body-size-limit.md — 100 MB default rationale

## Notes

> The implementation agent should investigate whether axum's `DefaultBodyLimit` can be dynamically updated, or if a custom middleware reading from ArcSwap is needed. The important thing is that config reload changes the limit without restarting.

## Summary

> To be filled on completion