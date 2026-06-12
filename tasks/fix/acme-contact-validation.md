---
id: fix/acme-contact-validation
name: Validate ACME contact email has non-empty address with @ sign (W2)
status: completed
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [W2]
---

## Description

The ACME contact validation only checks that `contact.starts_with("mailto:")`
but doesn't verify there's an actual email address after the prefix. Values
like `mailto:` (empty email) pass validation but will fail at the Let's Encrypt
API at certificate provisioning time — after the proxy has already started.

The spec (config.md validation rule 19) now requires: "a valid `mailto:` URI
with a non-empty email address containing an `@` sign."

### Changes Required

**`src/config/validation.rs`** — ACME contact validation (lines 148-153):
- After checking `starts_with("mailto:")`, validate the email part:
  ```rust
  let contact = &listener.tls.acme_contact;
  if contact.is_empty() || !contact.starts_with("mailto:") {
      errors.push(ValidationError::AcmeContactInvalid {
          bind_addr: listener.bind_addr.clone(),
      });
  } else {
      let email = &contact[7..]; // after "mailto:"
      if email.is_empty() || !email.contains('@') {
          errors.push(ValidationError::AcmeContactInvalid {
              bind_addr: listener.bind_addr.clone(),
          });
      }
  }
  ```
- Add tests for:
  - Valid: `mailto:admin@example.com`
  - Invalid: `mailto:` (empty), `mailto:user` (no @), empty string, non-mailto

## Acceptance Criteria

- [ ] `mailto:` (empty email) is rejected by validation
- [ ] `mailto:user` (no @ sign) is rejected by validation
- [ ] `mailto:admin@example.com` still passes validation
- [ ] Non-mailto values still rejected
- [ ] New unit tests for the tightened validation
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/config.md — validation rule 19
- docs/reviews/003-security-and-bug-review.md — W2 finding
- src/config/validation.rs — ACME contact validation, existing tests

## Notes

> To be filled on completion

## Summary

> To be filled on completion