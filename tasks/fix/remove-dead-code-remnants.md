---
id: fix/remove-dead-code-remnants
name: Remove dead code items identified in security review #003 (S1)
status: completed
depends_on: []
scope: narrow
risk: trivial
impact: component
level: implementation
review_findings: [S1]
---

## Description

Security review #003 identifies the following dead code items that survived the
previous `fix/clean-dead-code` task. These items are defined but never called in
production code:

| Item | File | Note |
|------|------|------|
| `log_rate_limit!` macro | `src/logging/format.rs:72-83` | Rate limiter uses `warn!` directly |
| `log_config_reload!` macro | `src/logging/format.rs:97-106` | Reload uses `info!`/`warn!` directly |
| `format_event_fields()` | `src/logging/format.rs:50-54` | Never called |
| `ProxyError::PayloadTooLarge` | `src/proxy/error.rs:12` | Body limit returns tuple directly |
| `ProxyError::NotFound` | `src/proxy/error.rs:20` | Code uses `UnknownHost` instead |
| `ProxyError::BadRequest` | `src/proxy/error.rs:22` | Code uses `MissingHost` instead |
| `ProxyError::UpstreamTls` | `src/proxy/error.rs:28` | Never constructed |
| `build_multi_domain_server_config()` | `src/tls/config.rs:78-102` | Never called |
| `SniCertResolver` | `src/tls/config.rs:104-126` | Only used by above |
| `AcmeTlsConfig::directory_url()` | `src/tls/acme.rs:53-59` | Only used in tests |

### Changes Required

For each item, either wire it up (if it should be used) or remove it:

**Logging macros** (`src/logging/format.rs`):
- `log_rate_limit!`: The rate limiter middleware now uses `warn!` directly.
  Remove the macro and its test invocation in `log_macros_compile`.
- `log_config_reload!`: The admin socket reload uses `info!`/`warn!` directly.
  Remove the macro and its test invocation.
- `format_event_fields()`: Never called externally. Remove it.

**ProxyError variants** (`src/proxy/error.rs`):
- `PayloadTooLarge`: Not used by body limit middleware (returns tuple directly).
  Keep for now with a comment explaining it's reserved for future use, OR remove
  if no plan to use it. The `#[non_exhaustive]` attribute means removing a variant
  is not a breaking change for external consumers.
- `NotFound`: Superseded by `UnknownHost`. Remove and update `status_code()` and
  `body()` match arms.
- `BadRequest`: Superseded by `MissingHost`. Remove and update match arms.
- `UpstreamTls`: Never constructed. Remove and update match arms.
- Update unit tests that reference removed variants.

**TLS dead code** (`src/tls/config.rs`, `src/tls/acme.rs`):
- `build_multi_domain_server_config()`: Remove function.
- `SniCertResolver`: Remove struct and impl (only used by above function).
- `AcmeTlsConfig::directory_url()`: Check if it's used in tests. If only in
  tests, gate with `#[cfg(test)]`. If truly unused, remove.

**Test helpers** (`tests/helpers/http_test_helper.rs`):
- `TestUpstream::url()`, `TestUpstream::upstream_addr()`: Currently annotated
  `#[allow(dead_code)]`. If not used in any test, remove. If used, remove the
  `#[allow(dead_code)]` annotation.

## Acceptance Criteria

- [ ] `log_rate_limit!` and `log_config_reload!` macros removed
- [ ] `format_event_fields()` removed
- [ ] Unused `ProxyError` variants removed (`NotFound`, `BadRequest`, `UpstreamTls`)
- [ ] `PayloadTooLarge` either removed or documented with a TODO comment
- [ ] `build_multi_domain_server_config()` and `SniCertResolver` removed
- [ ] `directory_url()` either `#[cfg(test)]`-gated or removed
- [ ] Dead test helper methods removed or `#[allow(dead_code)]` removed
- [ ] All match arms in `ProxyError` methods updated for removed variants
- [ ] All unit tests updated for removed variants
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/003-security-and-bug-review.md — S1 finding
- src/logging/format.rs — dead macros and function
- src/proxy/error.rs — unused ProxyError variants
- src/tls/config.rs — dead TLS functions
- src/tls/acme.rs — directory_url method
- tests/helpers/http_test_helper.rs — dead_code-annotated helpers

## Notes

> The previous `fix/clean-dead-code` task added `#[non_exhaustive]` and removed
> some dead code. This task addresses the items that survived that round or were
> newly identified in review #003.

## Summary

> To be filled on completion