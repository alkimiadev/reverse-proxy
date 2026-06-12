---
id: fix/clean-dead-code
name: Remove dead_code annotations and add #[non_exhaustive] to public enums
status: pending
depends_on: [fix/acme-contact-and-challenge]
scope: narrow
risk: low
impact: component
level: implementation
review_findings: [S4, S2]
---

## Description

Many public items are annotated with `#[allow(dead_code)]` because they're defined but not yet used by the binary crate. After the ACME contact and challenge fix (which wires up previously unused ACME code), most of these annotations should be removable.

Additionally, public enums (`TlsMode`, `ProxyError`, `AdminSocketError`, `ValidationError`) should have `#[non_exhaustive]` to allow future expansion without breaking changes.

### Changes Required

**Remove `#[allow(dead_code)]` annotations**:
- After `fix/acme-contact-and-challenge` is complete, `build_acme_challenge_config` and `TlsMode::Acme.challenge_config` will be removed entirely (not just un-annotated)
- Run `cargo check` and remove `#[allow(dead_code)]` annotations from items that are now used
- Items that are still genuinely unused after the ACME fix should keep the annotation but with a TODO comment explaining why

**Add `#[non_exhaustive]` to public enums**:
- `src/tls/acceptor.rs:49` — `TlsMode` enum (may gain modes like `"letsencrypt"` or `"auto"`)
- `src/proxy/error.rs:5` — `ProxyError` enum (may gain `UpstreamTls` error handling)
- `src/admin/socket.rs:15` — `AdminSocketError` enum (may gain new error variants)
- `src/config/validation.rs:10` — `ValidationError` enum (new validation rules may be added)

### Files to Check

- `src/tls/acceptor.rs` — lines 14, 33, 48, 58
- `src/tls/acme.rs` — lines 9, 11, 15, 23, 55
- `src/config/static_config.rs` — lines 4, 31, 44, 49, 54, 56, 70, 76, 86, 91

## Acceptance Criteria

- [ ] `#[allow(dead_code)]` annotations removed from items that are now used
- [ ] Remaining `#[allow(dead_code)]` annotations have TODO comments explaining why
- [ ] `#[non_exhaustive]` added to `TlsMode`, `ProxyError`, `AdminSocketError`, `ValidationError`
- [ ] `cargo check` passes (no unused code warnings for annotated items)
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/002-implementation-review.md — S4, S2 findings
- src/tls/acceptor.rs — TlsMode enum
- src/proxy/error.rs — ProxyError enum
- src/admin/socket.rs — AdminSocketError enum
- src/config/validation.rs — ValidationError enum

## Notes

> This task depends on fix/acme-contact-and-challenge because the ACME fix removes some dead code (challenge_config) and wires up other code that was previously unused.

## Summary

> To be filled on completion