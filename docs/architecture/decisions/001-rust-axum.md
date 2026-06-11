# ADR-001: Rust with Axum

## Status

Accepted

## Context

Our current nginx 1.24.0 installation is vulnerable to multiple actively-exploited
CVEs, most critically CVE-2026-42945 (CVSS 9.2, unauthenticated RCE via
`ngx_http_rewrite_module`). Six of seven recent nginx CVEs are memory corruption
bugs (buffer overflow, use-after-free, buffer overread) — the exact class of
vulnerabilities that Rust eliminates by construction.

The threat landscape is worsening: LLM-assisted fuzzing is accelerating bug
discovery in nginx's C codebase, and security researchers report additional
undisclosed vulnerabilities.

We need to replace nginx with a memory-safe alternative that can handle:
- TLS termination
- HTTP reverse proxying to backend services
- Rate limiting with fail2ban-compatible logging
- Operational simplicity (single binary, systemd integration)

## Decision

Use Rust with the axum web framework for the reverse proxy implementation.

**Rust** provides:
- Memory safety by construction (no buffer overflows, use-after-free, or
  double-free at runtime)
- rustls (pure Rust TLS) avoids OpenSSL dependency and its CVE history
- Single static binary deployment with no runtime dependencies
- Excellent async I/O support via tokio

**axum** provides:
- Ergonomic handler definitions with extractors
- Tower middleware ecosystem (Service trait, layers)
- Type-safe routing and state management
- Well-maintained, widely used, good documentation

## Consequences

**Positive:**
- Eliminates the entire class of memory corruption vulnerabilities affecting
  nginx
- Single binary deployment simplifies operations
- Rust's type system catches many errors at compile time
- axum + tower provides composable middleware

**Negative:**
- Smaller ecosystem than nginx for HTTP proxy features (but our use case is
  simple)
- We maintain the code (vs. using a battle-tested C project)
- Less granular control over HTTP/2 and connection pooling compared to nginx
- Team needs Rust expertise (already available)

## References

- [threat-landscape.md](../../research/threat-landscape.md)
- [overview.md](../overview.md)