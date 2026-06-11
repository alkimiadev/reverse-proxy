---
status: draft
last_updated: 2026-06-11
---

# Overview

## Vision

A memory-safe, minimal reverse proxy that replaces our vulnerable nginx instance
for forwarding requests to backend services. The proxy terminates TLS, injects
standard proxy headers, enforces rate limits, and forwards requests to upstream
services — supporting multiple domains from initial release.

This project is open source under dual licensing: MIT OR Apache-2.0, consistent
with standard Rust project licensing.

## Why This Exists

Our nginx 1.24.0 installation is vulnerable to multiple actively-exploited
CVEs, including CVE-2026-42945 (unauthenticated RCE via `rewrite`/`set`
directives). The broader threat landscape is worsening: LLM-assisted fuzzing
is accelerating bug discovery in nginx's C codebase, and security researchers
report additional undisclosed vulnerabilities. Upgrading nginx patches known
CVEs but does not address the structural problem — memory corruption bugs are
endemic to C, and the discovery rate is accelerating.

Rust's memory safety eliminates the entire class of buffer overflow,
use-after-free, and double-free bugs that constitute 6 of 7 recent nginx CVEs.
Combined with rustls (pure Rust TLS, no OpenSSL dependency), this provides a
fundamentally safer baseline.

See [threat-landscape.md](../research/threat-landscape.md) for full vulnerability
details.

## Scope

### In Scope

- **Phase 1**: Multi-site reverse proxy with TLS termination
  - TLS termination with ACME (Let's Encrypt) multi-domain certificate management
  - Manual certificate paths as fallback mode
  - HTTP → HTTPS redirect
  - Host-based routing to multiple upstream services
  - Reverse proxy to Gitea at `127.0.0.1:3000` (git.alk.dev)
  - Reverse proxy to Deno/Fresh container for alk.dev (simple pass-through)
  - Proxy header injection (Host, X-Real-IP, X-Forwarded-For, X-Forwarded-Proto)
  - Request rate limiting with fail2ban-compatible logging (global per-IP)
  - 100 MB body size limit (global)
  - Configurable bind address (no `0.0.0.0` default)
  - Health check endpoint
  - Graceful shutdown (SIGTERM handling)
  - Systemd unit file
  - Dual licensing: MIT OR Apache-2.0

- **Phase 2**: Operational hardening
  - Per-site rate limits and body limits
  - Per-site upstream timeouts
  - Metrics endpoint (Prometheus-compatible)
  - Connection limits and timeouts
  - Log rotation

- **Phase 3**: Future enhancements
  - Wildcard subdomain support
  - Per-site TLS overrides (manual certs for specific domains)
  - Unix domain socket config reload API

### Out of Scope

- HTTP/2 or HTTP/3 proxying (services that need these run their own native
  Rust servers — e.g., `api.alk.dev` runs its own HTTP/2+ server)
- Load balancing or round-robin upstream selection
- WebSocket proxying (can be added later if needed)
- Static file serving
- Access control beyond rate limiting (no auth, no IP allowlists in Phase 1)
- CGI, SCGI, uWSGI, FastCGI
- Per-site TLS configuration (all domains share one ACME config in Phase 1)

## Architecture

```
                     ┌────────────────────────────────────┐
                     │     reverse-proxy (Rust/axum)       │
config.toml ──────► │  StaticConfig + DynamicConfig       │
                     │  (ArcSwap for hot-reload)            │
                     │                                      │
bind_addr:80   ──►  │  HTTP listener → 301 redirect        │
                     │     to HTTPS                         │
                     │                                      │
bind_addr:443  ──►  │  TLS listener (tokio-rustls)         │
                     │  ├─ ACME mode: rustls-acme resolver  │
                     │  │  (multi-domain SAN cert,           │
                     │  │   auto-provision & renew)          │
                     │  └─ Manual mode: cert/key file paths  │
                     │                                      │
                     │  axum router                         │
                     │  ├─ Host-based routing                │
                     │  │  ├─ git.alk.dev → :3000            │
                     │  │  └─ alk.dev     → :8080            │
                     │  ├─ Rate limiting middleware          │
                     │  ├─ Proxy header injection            │
                     │  ├─ Body size limit (100MB)           │
                     │  └─ Reverse proxy handler             │
                     │     └─ hyper Client → upstream        │
                     │                                      │
                     │  /health → 200 OK                    │
                     └────────────────────────────────────┘
```

## Crate Dependencies

### Core

| Crate | Version | Purpose | Notes |
|-------|---------|---------|-------|
| `axum` | 0.8 | HTTP framework | Routing, middleware, extractors |
| `tokio` | 1 (full) | Async runtime | Multi-threaded runtime |
| `hyper` | 1 | HTTP protocol | Used via axum, and directly for proxy `Client` |
| `tower` | 0.5 | Middleware ecosystem | Service trait, layers |
| `rustls` | 0.23 | TLS implementation | `aws_lc_rs` crypto provider |
| `tokio-rustls` | 0.26 | Async TLS I/O | Wraps TCP with TLS |
| `rustls-acme` | 0.12 | ACME client | Let's Encrypt auto-provisioning and renewal |

### Supporting

| Crate | Version | Purpose | Notes |
|-------|---------|---------|-------|
| `serde` | 1 | Serialization | TOML config deserialization |
| `toml` | 0.8 | Config format | Declarative site definitions |
| `arc-swap` | 1 | Atomic config swap | Lock-free DynamicConfig reload |
| `tracing` | 0.1 | Structured logging | fail2ban-compatible output |
| `tracing-subscriber` | 0.3 | Log output | File + journald support |
| `rustls-pemfile` | 2 | PEM parsing | Manual cert loading |
| `rustls-pki-types` | 1 | TLS types | CertificateDer, PrivateKeyDer |
| `clap` | 4 | CLI arguments | Server startup options |
| `signal-hook` | 0.3 | Signal handling | SIGTERM/SIGINT for shutdown, SIGHUP for config reload |

Versions listed are minimum major versions. Implementation should pin exact
versions in `Cargo.toml` per standard Rust practice.

## Exports

This is a single-binary deployment. There are no library exports. The product
is the `reverse-proxy` binary plus a systemd unit file and a config file.

## Dependencies on Other Projects

- **alknet**: The `ArcSwap<DynamicConfig>` pattern, `tokio-rustls` TLS acceptor
  construction, `rustls-acme` integration, and `ServerConfig` builder patterns
  are adapted from alknet's transport and config layers. These patterns are
  referenced as validation that the approaches work in production; all code
  in this project is written from scratch.

## Design Decisions

All design decisions are documented as ADRs in [decisions/](decisions/).

| ADR | Decision | Summary |
|-----|----------|---------|
| [001](decisions/001-rust-axum.md) | Rust with axum | Memory safety eliminates the bug class causing nginx CVEs; axum provides ergonomic tower integration |
| [002](decisions/002-custom-proxy-handler.md) | Custom proxy handler | Single upstream per domain — simpler than a general proxy library |
| [003](decisions/003-toml-config.md) | TOML configuration format | Rust-native, unambiguous, excellent serde support |
| [004](decisions/004-rustls-acme.md) | ACME-primary certificate management | Eliminates certbot dependency; automatic provisioning and renewal |
| [005](decisions/005-tokio-rustls-direct.md) | tokio-rustls directly, not axum-server | Full control over TLS config, ACME resolver integration, cipher suite configuration |
| [006](decisions/006-rate-limiting-approach.md) | Token bucket rate limiting | In-memory per-IP token bucket matching nginx burst semantics |
| [007](decisions/007-custom-log-format.md) | Custom structured log format | key=value pairs with RATE_LIMIT prefix for fail2ban |
| [008](decisions/008-static-dynamic-config-split.md) | Static/dynamic config with ArcSwap | Immutable StaticConfig, hot-reloadable DynamicConfig via ArcSwap |
| [009](decisions/009-signal-handling.md) | Signal handling strategy | signal-hook for SIGTERM/SIGINT/SIGHUP |
| [010](decisions/010-multi-site-phase1.md) | Multi-site in Phase 1 | Multiple domains from initial release; avoids config migration later |
| [011](decisions/011-multi-domain-tls.md) | Multi-domain TLS config | Single SAN certificate covering all domains via rustls-acme |

## Open Questions

Open questions are tracked in [open-questions.md](open-questions.md). Key
questions affecting this document:

- **OQ-01**: Should cipher suites be restricted beyond rustls defaults? (open)
- **OQ-03**: Should the health check endpoint be on a separate port? (open)
- **OQ-07**: Should per-site TLS overrides be supported for mixed ACME/manual domains? (open)