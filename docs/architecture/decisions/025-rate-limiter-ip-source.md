# ADR-025: Rate Limiter IP Source Must Be ConnectInfo Only

## Status

Accepted

## Context

The rate limiter identifies clients by IP address to enforce per-IP token bucket
limits. The question is: what is the authoritative source of the client IP for
rate limiting?

Two potential sources exist:

1. **`ConnectInfo<SocketAddr>`**: The TCP peer address, extracted from
   `TcpStream::peer_addr()` before TLS handshake and propagated to axum via
   `ConnectInfoService`. This is the real client IP at the TCP level.

2. **`X-Forwarded-For` header**: A client-supplied HTTP header that may contain
   an IP address. This header is set by the proxy handler *after* rate limiting
   (the rate limiter runs as middleware before the handler), so the value
   present during rate limit checks is whatever the client sent — completely
   untrusted.

ADR-021 already establishes that the proxy is an edge proxy and client-supplied
`X-Forwarded-For` headers are untrusted. The proxy handler replaces
`X-Forwarded-For` with the `ConnectInfo` IP before forwarding upstream
specifically to prevent spoofing. However, ADR-021 only addresses the proxy
handler's header injection — it does not specify the rate limiter's IP source.

The current implementation checks `X-Forwarded-For` *first* and falls back to
`ConnectInfo`, which creates two attack vectors:

- **Rate limit bypass**: A client sends each request with a different random
  `X-Forwarded-For` value. Every request appears to come from a different IP,
  evading the per-IP token bucket entirely.
- **Denial-of-service via IP spoofing**: A client sends requests with
  `X-Forwarded-For: <victim IP>`. The victim's bucket is depleted and their
  legitimate requests receive 429 responses.

## Decision

The rate limiter must use `ConnectInfo<SocketAddr>` as the **sole** source of
client IP addresses. `X-Forwarded-For` must not be consulted for rate limiting
purposes.

The rate limiter runs as middleware before the proxy handler. At that point in
the request lifecycle, no proxy headers have been injected — any `X-Forwarded-For`
header present is from the client and is untrusted (ADR-021).

`ConnectInfo<SocketAddr>` is always present because each listener populates it
via `into_make_service_with_connect_info::<SocketAddr>()`. If `ConnectInfo` is
absent (which should not happen in normal operation), the request should be
rejected rather than falling back to an untrusted header.

## Rationale

- The proxy is the edge proxy (ADR-021). Client-supplied headers are
  untrusted at the edge.
- Rate limiting is a security mechanism — it must use the most trustworthy
  IP source available. `ConnectInfo` is set by the kernel's TCP stack, not by
  the client.
- The rate limiter's position in the middleware stack (before the handler)
  means it sees raw client headers, not the replaced `X-Forwarded-For` that the
  proxy handler injects.
- Falling back to `X-Forwarded-For` when `ConnectInfo` is absent creates a
  downgrade attack — an attacker could potentially strip `ConnectInfo` from
  extensions to force the fallback path. Better to reject than to accept an
  untrusted IP.

## Consequences

**Positive:**
- Rate limiting cannot be bypassed via header spoofing
- Rate limiting cannot be weaponized to DoS legitimate clients
- Consistent with ADR-021's edge proxy trust model
- No ambiguity about the IP source — one source, one code path

**Negative:**
- If the proxy is ever placed behind a trusted CDN or load balancer,
  `ConnectInfo` will reflect the CDN's IP, not the end client's. This would
  require a "trusted proxies" configuration (already noted as a Phase 2
  consideration in ADR-021).
- Requests without `ConnectInfo` are rejected. This should not happen in
  normal operation but adds a hard failure mode.

## References

- [proxy.md](../proxy.md) — Proxy header injection, request flow
- [operations.md](../operations.md) — Rate limiting design
- ADR-006 — Token bucket rate limiting
- ADR-021 — X-Forwarded-For edge proxy model
- Security Review C1 — Rate limiter X-Forwarded-For spoofing vulnerability