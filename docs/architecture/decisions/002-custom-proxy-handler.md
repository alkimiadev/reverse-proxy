# ADR-002: Custom Proxy Handler

## Status

Accepted

## Context

We need to implement HTTP reverse proxying — receiving requests and forwarding
them to an upstream service (Gitea on localhost:3000). Two approaches are
available:

1. **`axum-reverse-proxy` crate**: Provides path-based routing, header
   forwarding, round-robin load balancing, TLS support, retry mechanisms, and
   RFC 9110 compliance.
2. **Custom handler** (Felix Knorr pattern): Build a handler using hyper's
   `Client` to forward requests. ~50-100 lines of Rust for our needs.

Our use case is minimal: single upstream per domain, no load balancing, no
retry, no HTTP/2 proxying. While the proxy supports multiple domains
(ADR-010), each domain routes to exactly one upstream.

## Decision

Implement a custom proxy handler using hyper's `Client` for request forwarding,
following the pattern demonstrated by Felix Knorr and used in the alknet
project's channel proxy.

## Rationale

- `axum-reverse-proxy` adds complexity we don't need (load balancing, retry,
  path-based routing to multiple backends)
- Our proxy case is the simplest possible: match a Host header, forward the
  entire request to a single upstream, stream the response back
- Multi-domain support (ADR-010) doesn't change this — each domain still maps
  to one upstream
- The Felix Knorr pattern is proven, idiomatic, and ~50-100 lines
- We maintain full control over header injection, error handling, and upstream
  connection behavior
- If requirements grow, we can adopt `axum-reverse-proxy` later

## Consequences

**Positive:**
- Minimal dependencies
- Full control over proxy behavior
- Easy to understand and audit (~100 lines of proxy code)
- No unnecessary abstraction layers

**Negative:**
- We implement and maintain proxy logic ourselves (but it's trivial for our
  use case — each domain maps to one upstream)
- If requirements grow to load balancing or retry, we'd need to add that
  ourselves or switch to `axum-reverse-proxy`

## References

- [proxy.md](../proxy.md)
- [ADR-010](010-multi-site-phase1.md) (multi-site in Phase 1)
- Felix Knorr, "Replacing nginx with axum" (felix-knorr.net/posts/2024-10-13-replacing-nginx-with-axum.html)