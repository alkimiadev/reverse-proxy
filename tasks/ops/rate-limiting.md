---
id: ops/rate-limiting
name: Implement token bucket rate limiting with IPv6 /64 normalization and background eviction
status: pending
depends_on: [config/dynamic-config]
scope: moderate
risk: medium
impact: component
level: implementation
---

## Description

Implement per-IP token bucket rate limiting as axum middleware. This runs before the proxy handler and rejects requests that exceed the rate limit with 429 Too Many Requests.

### Token Bucket Algorithm

- **Nodelay** semantics matching nginx's `limit_req burst nodelay`
- When bucket is empty, request is immediately rejected with 429 — no queuing
- Tokens added at rate of `requests_per_second` (1 token every `1000ms / requests_per_second`)
- Bucket capacity is `burst` value
- Per-IP in Phase 1 (not per-site)

### IPv6 Normalization

- **IPv4**: Rate limited per individual address (`/32`)
- **IPv6**: Rate limited per `/64` prefix. All addresses in the same `/64` share a token bucket
- Normalize IPv6 addresses to their `/64` prefix before bucket lookup

### Rate Limit State

- `Arc<Mutex<HashMap<IpAddr, TokenBucket>>>` shared via axum State
- Token bucket struct with: `tokens: f64`, `last_refill: Instant`, `rate: f64`, `max: u32`

### Background Eviction Task

- Runs every 60 seconds (configurable)
- Removes entries whose last access timestamp is older than 300 seconds (5 minutes default)
- Prevents unbounded memory growth

### Config Reload Behavior

When rate limit parameters change:
1. New `DynamicConfig` swapped in via ArcSwap
2. On next request from an existing IP, rate limiter reads current DynamicConfig
3. Token bucket refills using new rate, capacity set to new burst
4. If current token count exceeds new burst max, cap to new burst max
5. HashMap is NOT cleared — avoids rate-limiting gap

### Logging

Rate limit events logged with `RATE_LIMIT` prefix:
```
RATE_LIMIT client_ip=203.0.113.50 host=Y.Z path=/W status=429
```

### Middleware Integration

Rate limiting runs as tower middleware before the proxy handler in the axum router.

## Acceptance Criteria

- [ ] Token bucket implementation with nodelay semantics
- [ ] Per-IP rate limiting with configurable rate and burst
- [ ] IPv6 addresses normalized to `/64` prefix before bucket lookup
- [ ] IPv4 addresses used as-is (`/32`)
- [ ] Background eviction task removes stale entries every 60 seconds
- [ ] Config reload: new rate/burst parameters adopted on next request from existing IP
- [ ] Token count capped to new burst max when burst decreases
- [ ] HashMap not cleared on config reload (no rate-limiting gap)
- [ ] `429 Too Many Requests` response with `Too Many Requests` body
- [ ] `RATE_LIMIT` prefixed log event with `client_ip`, `host`, `path`, `status`
- [ ] Rate limiter state shared via `Arc<Mutex<HashMap<IpAddr, TokenBucket>>>`
- [ ] Unit tests for token bucket algorithm (fill, drain, reject)
- [ ] Unit tests for IPv6 `/64` normalization
- [ ] Integration test: requests above rate limit receive 429

## References

- docs/architecture/operations.md — rate limiting section
- docs/architecture/decisions/006-rate-limiting-approach.md — token bucket rationale

## Notes

> The rate limiter must be efficient on the hot path — no locks on reads. Consider using a `DashMap` or similar concurrent map instead of `Mutex<HashMap>` for better read performance. The spec says `Mutex<HashMap>` but an implementation agent may choose a more performant concurrent data structure.

## Summary

> To be filled on completion