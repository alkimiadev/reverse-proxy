# ADR-010: Multi-Site Support in Phase 1

## Status

Accepted

## Context

The original architecture phased multi-site support into Phase 2, treating
Phase 1 as a single-domain replacement for nginx serving only `git.alk.dev`.
This was based on the assumption that only one domain needed proxying initially.

However, `alk.dev` (the bare domain) will need proxying in the near future.
While `alk.dev` is a simple case — proxying to a Deno/Fresh container with no
special requirements — the proxy must support multiple sites from day one. The
config format, routing logic, and TLS certificate provisioning all need
multi-site awareness.

Additionally, `api.alk.dev` is explicitly out of scope (it runs its own
HTTP/2+ server natively), but the proxy must not prevent future sites from
being added.

The cost of deferring multi-site is high: we'd need a config format migration,
routing logic rewrite, and TLS cert management changes later. Supporting
multi-site from the start costs very little — the config format just uses an
array of sites (which it already does), host-based routing is trivial in axum,
and `rustls-acme` supports multi-domain certificates natively.

## Decision

Move multi-site support from Phase 2 into Phase 1. The proxy supports multiple
sites from the initial release:

- `[[listeners.sites]]` array in each listener config (after ADR-019; was
  `[[sites]]` at top level)
- Host-based routing via axum's `Host` extractor (already the planned approach)
- Multi-domain ACME certificate provisioning via `rustls-acme`
- Each site maps a hostname to an upstream address

Phase 1 scope becomes:

1. Multi-site reverse proxy with TLS termination
2. ACME certificate management (multi-domain)
3. HTTP → HTTPS redirect
4. Rate limiting, logging, health check, graceful shutdown
5. Systemd integration

Phase 2 scope shifts to operational hardening:

1. Per-site rate limits and body limits
2. Metrics endpoint (Prometheus-compatible)
3. Connection limits and timeouts
4. Log rotation

Note: "Per-site upstream timeouts" was originally listed here but was moved to
Phase 1 via ADR-015.

Phase 3 remains future enhancements.

## Rationale

- The config format already uses `[[sites]]` — no format change needed
- Host-based routing is the natural axum pattern and was already planned
- `rustls-acme` accepts `Vec<domain>` — multi-domain is its default usage
- The cost of adding multi-site later (config migration, routing rewrite,
  cert management changes) far exceeds the cost of supporting it now (zero
  additional complexity)
- `alk.dev` is confirmed as a near-term need, not a hypothetical
- The proxy's value proposition is being a memory-safe reverse proxy for *our
  infrastructure*, which has multiple domains

## Consequences

**Positive:**
- No config format migration needed later
- `alk.dev` can be added to the config without code changes
- TLS cert management handles multiple domains from the start
- Eliminates an entire phase of work

**Negative:**
- Slightly more testing surface (must verify correct routing with multiple
  sites)
- Must test multi-domain ACME provisioning (not just single-domain)
- Wildcard or fallback site behavior is defined by the listener's site routing

## References

- [overview.md](../overview.md)
- [config.md](../config.md)
- [tls.md](../tls.md)
- [proxy.md](../proxy.md)
- ADR-002 (custom proxy handler — rationale updated for multi-site)