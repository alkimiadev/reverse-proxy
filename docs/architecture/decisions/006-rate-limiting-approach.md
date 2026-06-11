# ADR-006: Token Bucket Rate Limiting with In-Memory State

## Status

Accepted

## Context

The proxy must enforce request rate limits per client IP address, replacing
nginx's `limit_req_zone` directive. Rate limiting is critical for preventing
abuse and for fail2ban integration (rate-limited requests trigger fail2ban
actions).

Several rate limiting approaches exist:
- **Token bucket**: Tokens accumulate at a fixed rate; each request consumes a
  token. Allows short bursts up to the bucket capacity.
- **Leaky bucket**: Requests are processed at a fixed rate; excess requests
  queue or are rejected. No burst allowance.
- **Fixed window**: Count requests in fixed time windows (e.g., per minute).
  Allows burst at window boundaries.
- **Sliding window**: Count requests in a rolling time window. More accurate
  than fixed window but more complex.

The current nginx config uses `limit_req zone=gitea_limit burst=20 nodelay`,
which is a token bucket with burst allowance.

For state storage:
- **In-memory HashMap**: Fast, no external dependencies, lost on restart.
- **External store (Redis, etc.)**: Shared across instances, persists across
  restarts. Adds operational complexity.
- **tower-governor crate**: Pre-built rate limiting middleware. Uses
  generalized cell algorithm. Adds dependency.

## Decision

Use a token bucket algorithm with in-memory `HashMap<IpAddr, TokenBucket>`
state, protected by `tokio::sync::Mutex`. Rate limiting runs as axum middleware
before the proxy handler.

Rate limits are global per-IP (not per-site) in Phase 1. Per-site rate limits
may be added in Phase 2 as the config model evolves.

Stale entries in the HashMap are cleaned up periodically. A background task
scans the HashMap at a configurable interval (default: 60 seconds) and removes
entries that haven't been accessed within the cleanup interval.

## Rationale

- Token bucket matches nginx's `limit_req burst` semantics, ensuring
  behavioral compatibility during migration.
- In-memory state is sufficient for a single-instance proxy (no shared state
  needed).
- `tokio::sync::Mutex` (not `std::sync::Mutex`) avoids holding the lock across
  await points and integrates with the async runtime.
- Custom implementation gives full control over logging output for fail2ban
  integration (ADR-007).
- State loss on restart is acceptable — rate limit state is inherently
  ephemeral.

## Consequences

**Positive:**
- Behavioral compatibility with nginx rate limiting
- Full control over fail2ban log format
- No external dependencies (Redis, etc.)
- Simple implementation (~100 lines)

**Negative:**
- Rate limit state is lost on restart (acceptable for single-instance deploy)
- Not suitable for multi-instance deployments without external state store
  (Phase 1 is single-instance)
- HashMap grows over time without eviction (mitigated by periodic cleanup)

## References

- [operations.md](../operations.md)
- nginx `limit_req` documentation