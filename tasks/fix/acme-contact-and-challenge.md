---
id: fix/acme-contact-and-challenge
name: Fix ACME contact email wiring and remove unused challenge config
status: pending
depends_on: []
scope: moderate
risk: high
impact: component
level: implementation
review_findings: [C1, C2]
---

## Description

Fix two related ACME bugs that prevent Let's Encrypt certificate provisioning from working:

1. **C2**: The `acme_contact` field is defined in the architecture (`TlsConfig.acme_contact` in config.md) but missing from the `TlsConfig` struct in `static_config.rs`. The ACME setup in `acceptor.rs` always passes `contact: vec![]`, which will cause production Let's Encrypt registration to fail with a 400-level error.

2. **C1**: When `TlsMode::Acme` is matched in `main.rs`, the `challenge_config` and `resolver` fields are destructured with `_` (discarded). The separate `challenge_config` is unnecessary — the `rustls-acme` crate's `ResolvesServerCertAcme` resolver handles TLS-ALPN-01 challenges automatically on the main listener when `acme-tls/1` is included in the ALPN list (which `build_acme_server_config` already does at line 28). The separate `challenge_config` and `build_acme_challenge_config` function should be removed.

### Changes Required

**`src/config/static_config.rs`**:
- Add `acme_contact: String` field to `TlsConfig` struct (with `#[serde(default)]`)
- Update TOML deserialization tests to include `acme_contact`

**`src/config/validation.rs`**:
- Add validation rule 19: In ACME mode, `tls.acme_contact` must be a non-empty string starting with `mailto:`
- Add `ValidationError::AcmeContactInvalid` variant

**`src/tls/acceptor.rs`**:
- Pass `tls_config.acme_contact` to `AcmeTlsConfig.contact` instead of `vec![]`
- Remove `build_acme_challenge_config` function (dead code after C1 fix)
- Remove `challenge_config` field from `TlsMode::Acme` variant
- Update `setup_tls` to only return `default_config` and `resolver` (no `challenge_config`)

**`src/main.rs`**:
- Update `TlsMode::Acme` destructuring to remove `challenge_config: _`
- Remove any references to the discarded fields

**`src/tls/acme.rs`**:
- Ensure `AcmeTlsConfig.contact` is used properly (it already has `contact: Vec<String>` field and `self.contact.iter().map(|c| c.as_str())` in `setup()` — the struct is fine, it just needs to receive the actual contact from config)

**Tests**:
- Update `static_config.rs` tests for new `acme_contact` field
- Update `validation.rs` tests for new ACME contact validation
- Update `acceptor.rs` tests for removed `challenge_config`
- Verify ACME setup still creates resolver correctly

## Acceptance Criteria

- [ ] `acme_contact` field exists on `TlsConfig` and deserializes from TOML
- [ ] Validation rejects ACME mode without `acme_contact` (or with non-`mailto:` value)
- [ ] `AcmeTlsConfig.contact` receives the configured `acme_contact` value (not empty vec)
- [ ] `build_acme_challenge_config` function is removed
- [ ] `TlsMode::Acme` variant no longer has `challenge_config` field
- [ ] `main.rs` no longer discards ACME resolver or challenge config
- [ ] ACME TLS-ALPN-01 challenge handling still works via main listener's ALPN protocol list
- [ ] All existing tests pass
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/config.md — `acme_contact` field definition, validation rule 19
- docs/architecture/tls.md — ACME mode, contact email
- docs/reviews/002-implementation-review.md — C1, C2 findings
- src/tls/acceptor.rs — current ACME setup code
- src/tls/acme.rs — AcmeTlsConfig struct
- src/config/static_config.rs — TlsConfig struct
- src/config/validation.rs — validation rules

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion