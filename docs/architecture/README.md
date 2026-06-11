---
status: draft
last_updated: 2026-06-11
---

# Reverse Proxy — Architecture

## Current State

**Phase 0 (Exploration) — Complete.** Phase 1 (Architecture) — In progress.

This project replaces our vulnerable nginx 1.24.0 installation with a
memory-safe Rust/axum reverse proxy. The primary motivation is CVE-2026-42945
(unauthenticated RCE in nginx's rewrite module) and the broader pattern of
memory corruption bugs in nginx's C codebase.

## Architecture Documents

| Document | Status | Description |
|----------|--------|-------------|
| [overview.md](overview.md) | Draft | Vision, scope, crate dependencies, exports |
| [proxy.md](proxy.md) | Draft | Reverse proxy handler, request flow, header injection |
| [tls.md](tls.md) | Draft | TLS termination, ACME, manual certs, SNI |
| [config.md](config.md) | Draft | TOML config format, static/dynamic split, ArcSwap reload |
| [operations.md](operations.md) | Draft | Rate limiting, logging, health check, systemd, shutdown |

## ADR Table

| ADR | Title | Status |
|-----|-------|--------|
| [001](decisions/001-rust-axum.md) | Rust with Axum | Accepted |
| [002](decisions/002-custom-proxy-handler.md) | Custom Proxy Handler | Accepted |
| [003](decisions/003-toml-config.md) | TOML Configuration Format | Accepted |
| [004](decisions/004-rustls-acme.md) | ACME-Primary Certificate Management | Accepted |
| [005](decisions/005-tokio-rustls-direct.md) | tokio-rustls Directly, Not axum-server | Accepted |
| [006](decisions/006-rate-limiting-approach.md) | Token Bucket Rate Limiting | Accepted |
| [007](decisions/007-custom-log-format.md) | Custom Structured Log Format | Accepted |
| [008](decisions/008-static-dynamic-config-split.md) | Static/Dynamic Config Split with ArcSwap | Accepted |
| [009](decisions/009-signal-handling.md) | Signal Handling Strategy | Accepted |

## Open Questions

See [open-questions.md](open-questions.md) for the full tracker.

| OQ | Question | Priority | Status |
|----|----------|----------|--------|
| OQ-01 | Should cipher suites be restricted beyond rustls defaults? | medium | open |
| ~~OQ-02~~ | ~~What log format should fail2ban consume?~~ | ~~high~~ | **resolved** (ADR-007) |
| OQ-03 | Should the health check endpoint be on a separate port? | low | open |
| OQ-04 | Config reload: SIGHUP only or also Unix socket API? | low | open |
| OQ-05 | Should the proxy bind to multiple addresses? | low | open |
| OQ-06 | Should upstream timeouts be configurable per-site? | low | open |

## Document Lifecycle

| Status | Meaning | Transitions |
|--------|---------|-------------|
| `draft` | Under active development. May change significantly. | → `reviewed` when open questions are resolved |
| `reviewed` | Architecture is final. Implementation may begin. | → `stable` when implementation is complete |
| `stable` | Locked. Changes require review and may warrant an ADR. | → `deprecated` when superseded |
| `deprecated` | Superseded. Kept for reference. | Removed when no longer referenced |