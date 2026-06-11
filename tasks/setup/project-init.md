---
id: setup/project-init
name: Initialize Rust project with Cargo, dependencies, and module skeleton
status: pending
depends_on: []
scope: moderate
risk: low
impact: project
level: implementation
---

## Description

Initialize the Rust project from scratch. The repo currently has only `docs/` and `.git/`. Set up a single-binary Rust project with all core dependencies per the architecture spec (overview.md), and create the module skeleton that subsequent tasks will fill in.

This is a single-binary project — there are no library exports. The product is the `reverse-proxy` binary.

### Core Dependencies

| Crate | Purpose |
|-------|---------|
| `axum` 0.8 | HTTP framework, routing, middleware, extractors |
| `tokio` 1 (full) | Async runtime |
| `hyper` 1 | HTTP protocol, proxy `Client` |
| `tower` 0.5 | Middleware ecosystem, Service trait |
| `rustls` 0.23 | TLS implementation, `aws_lc_rs` crypto provider |
| `tokio-rustls` 0.26 | Async TLS I/O |
| `rustls-acme` 0.12 | ACME client for Let's Encrypt |
| `serde` 1 | Serialization |
| `toml` 0.8 | Config format |
| `arc-swap` 1 | Atomic config swap for DynamicConfig |
| `tracing` 0.1 | Structured logging |
| `tracing-subscriber` 0.3 | Log output (file + stdout) |
| `rustls-pemfile` 2 | PEM parsing for manual cert loading |
| `rustls-pki-types` 1 | TLS types (CertificateDer, PrivateKeyDer) |
| `clap` 4 | CLI arguments |
| `signal-hook` 0.3 | SIGTERM/SIGINT/SIGHUP handling |

Pin exact versions in `Cargo.toml` per standard Rust practice.

### Module Skeleton

```
src/
├── main.rs           — entry point, CLI parsing, startup orchestration
├── config/
│   ├── mod.rs         — config module, re-exports
│   ├── static_config.rs — StaticConfig, ListenerConfig, TlsConfig, LoggingConfig
│   ├── dynamic_config.rs — DynamicConfig, SiteConfig, RateLimitConfig
│   └── validation.rs  — config validation logic
├── proxy/
│   ├── mod.rs         — proxy module, re-exports
│   ├── handler.rs     — reverse proxy handler
│   ├── headers.rs     — proxy header injection
│   └── error.rs       — error response types
├── tls/
│   ├── mod.rs         — TLS module, re-exports
│   ├── acceptor.rs    — TLS acceptor construction (manual + ACME)
│   └── redirect.rs    — HTTP → HTTPS redirect handler
├── rate_limit/
│   ├── mod.rs         — rate limit module
│   └── bucket.rs      — token bucket implementation
├── logging/
│   ├── mod.rs         — logging module
│   └── format.rs      — custom structured log format
├── admin/
│   ├── mod.rs         — admin socket module
│   └── socket.rs      — Unix domain socket handler
├── health.rs          — health check endpoint
└── shutdown.rs        — graceful shutdown logic
```

## Acceptance Criteria

- [ ] `Cargo.toml` with all dependencies listed in overview.md, exact versions pinned
- [ ] `src/main.rs` with minimal `fn main()` that compiles
- [ ] All module files exist with `mod.rs` re-exports and skeleton content
- [ ] `cargo check` succeeds with no errors
- [ ] `cargo clippy` succeeds with no warnings
- [ ] Binary name is `reverse-proxy` in `Cargo.toml`
- [ ] `.gitignore` covers `target/`
- [ ] Dual licensing: `MIT OR Apache-2.0` in `Cargo.toml`

## References

- docs/architecture/overview.md — crate dependencies, exports
- docs/architecture/config.md — config structure
- docs/architecture/proxy.md — proxy handler architecture
- docs/architecture/tls.md — TLS architecture
- docs/architecture/operations.md — rate limiting, logging, health check, shutdown

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion