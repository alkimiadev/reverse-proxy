# ADR-021: X-Forwarded-For Edge Proxy Model

## Status

Accepted

## Context

The reverse proxy terminates TLS and injects standard proxy headers (Host,
X-Real-IP, X-Forwarded-For, X-Forwarded-Proto) before forwarding requests to
upstream services. The question is how to handle the `X-Forwarded-For` header
when the request already contains one.

Two approaches exist:

1. **Append**: Add the client IP to any existing `X-Forwarded-For` value.
   This preserves the proxy chain history but trusts that existing values are
   legitimate. In a chained proxy scenario (e.g., CDN → reverse proxy →
   backend), appending is correct because the CDN's `X-Forwarded-For` value
   is trustworthy.

2. **Replace**: Set `X-Forwarded-For` to the client's IP address from
   `ConnectInfo<SocketAddr>`, discarding any existing value. This is correct
   when the proxy is the edge proxy directly facing the internet, because
   client-provided `X-Forwarded-For` headers are untrusted — any client can
   inject arbitrary values.

This proxy is deployed as the **edge proxy** — it sits directly in front of the
internet with no trusted proxies upstream. All client connections come directly
to this proxy.

## Decision

Set `X-Forwarded-For` to the client's IP address from `ConnectInfo<SocketAddr>`,
**replacing** any existing value rather than appending.

The proxy is an edge proxy. There are no trusted proxies upstream, so existing
`X-Forwarded-For` headers from clients cannot be trusted. Replacing prevents
header spoofing attacks where a malicious client injects fake IP addresses to
confuse upstream services or bypass IP-based access controls.

## Rationale

- The proxy is the edge proxy — it directly faces the internet. No CDN or
  other trusted proxy sits in front of it.
- Client-provided `X-Forwarded-For` headers are untrusted. Any client can send
  `X-Forwarded-For: 1.2.3.4` to spoof their IP.
- Appending to an untrusted header creates a security vulnerability: upstream
  services (like Gitea) may use `X-Forwarded-For` for IP-based rate limiting or
  access control. Spoofed values would bypass these protections.
- `X-Real-IP` is also set to the client's IP from `ConnectInfo`, providing a
  trustworthy header for upstream services that need the real client IP.
- If this proxy is ever placed behind a CDN or other trusted proxy, the header
  handling model should be revisited. A "trusted proxies" configuration can be
  added in Phase 2 to support chained proxy scenarios.

## Consequences

**Positive:**
- Prevents `X-Forwarded-For` spoofing attacks
- Upstream services receive the real client IP, not a spoofed one
- Simple, predictable header handling — no trust model to configure
- Consistent with the proxy's role as the edge proxy

**Negative:**
- If the proxy is placed behind a CDN or other proxy in the future, the header
  handling must be updated to support a "trusted proxies" model
- Does not preserve legitimate proxy chain history (not applicable in our
  deployment — there are no proxies upstream)

## References

- [proxy.md](../proxy.md)
- Architecture Review C2 (X-Forwarded-For security model)