---
id: fix/log-root-cert-count
name: Log system root certificate count at startup (S3)
status: completed
depends_on: []
scope: single
risk: trivial
impact: isolated
level: implementation
review_findings: [S3]
---

## Description

`root_certs()` loads native certificates silently — only logs errors. If the
system has zero root certificates (misconfigured CA bundle), all HTTPS upstream
connections will fail with opaque TLS errors and no diagnostic message.

### Changes Required

**`src/proxy/handler.rs`** — `root_certs()` function (lines 246-258):
- Add an info-level log with cert count and warn if zero:
  ```rust
  fn root_certs() -> rustls::RootCertStore {
      let mut roots = rustls::RootCertStore::empty();
      let result = rustls_native_certs::load_native_certs();
      for cert in result.certs {
          roots.add(cert).ok();
      }
      let cert_count = roots.len();
      let error_count = result.errors.len();
      if cert_count == 0 {
          warn!(certs_loaded = cert_count, errors = error_count,
              "no system root certificates loaded — HTTPS upstream connections will fail");
      } else {
          info!(certs_loaded = cert_count, errors = error_count,
              "loaded system root certificates");
      }
      for err in &result.errors {
          warn!(error = %err, "failed to load native certificate");
      }
      roots
  }
  ```

## Acceptance Criteria

- [ ] Info-level log with cert count when certs > 0
- [ ] Warn-level log when cert count is 0
- [ ] Error count included in log output
- [ ] Individual cert load errors still logged at warn level
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/reviews/003-security-and-bug-review.md — S3 finding
- src/proxy/handler.rs — `root_certs()` function

## Notes

> To be filled on completion

## Summary

> To be filled on completion