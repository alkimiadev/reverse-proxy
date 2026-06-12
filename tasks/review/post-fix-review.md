---
id: review/post-fix-review
name: Review all post-fix implementations for correctness and spec conformance
status: pending
depends_on: [fix/acme-contact-and-challenge, fix/remove-health-and-hardcode-https, fix/config-reload-static-drift, fix/access-logging, fix/graceful-shutdown, fix/connect-timeout, fix/token-bucket-nanosecond, fix/normalize-host-ipv6, fix/http-port-validation, fix/integration-test-toml, fix/logging-test-global-subscriber, fix/fragile-error-detection, fix/add-code-comments, fix/clean-dead-code, fix/request-timeout-scope]
scope: broad
risk: low
impact: project
level: review
---

## Description

Post-implementation review of all fix tasks from review #002. Verify that each fix correctly addresses the original finding, matches the architecture spec, and doesn't introduce regressions.

### Review Checklist

1. **ACME contact and challenge (C1, C2)**:
   - `acme_contact` field exists on `TlsConfig` and deserializes from TOML
   - Validation rule 19 enforces `mailto:` format for `acme_contact` in ACME mode
   - `AcmeTlsConfig.contact` receives configured contact (not empty vec)
   - `build_acme_challenge_config` removed, `TlsMode::Acme` no longer has `challenge_config`
   - TLS-ALPN-01 challenge still works via main listener ALPN protocol

2. **Health route removal / HTTPS hardcode (W5, W14, ADR-022)**:
   - No `/health` route on main listener
   - `is_https` removed from `ProxyState`
   - `X-Forwarded-Proto` always `"https"` with explanatory comment
   - Port 9900 health check still works independently

3. **Config reload drift (C4)**:
   - `ConfigReloadHandle.static_config` uses `ArcSwap<StaticConfig>`
   - Second reload only reports new changes, not repeated old changes

4. **Access logging (W13)**:
   - `log_request!` called for every proxied request
   - `log_upstream_error!` called for upstream failures
   - Log format matches spec: `REQUEST client_ip=... host=... method=... path=... status=... upstream=... duration_ms=...`
   - `duration_ms` measures wall time accurately

5. **Graceful shutdown (W1, W7)**:
   - HTTPS tasks joined with timeout, not immediately aborted
   - Admin socket and eviction task respond to shutdown signal
   - Socket file cleaned up on exit

6. **Connect timeout (W4)**:
   - `upstream_connect_timeout_secs` wired up in proxy handler
   - Separate connect timeout enforced

7. **All other fixes**:
   - Token bucket nanosecond precision
   - IPv6 normalize_host
   - http_port validation
   - Integration test TOML
   - Logging test global subscriber
   - Fragile error detection
   - Code comments added
   - Dead code cleaned, `#[non_exhaustive]` added

8. **Quality gates**:
   - `cargo clippy` passes with no warnings
   - `cargo fmt --check` passes
   - `cargo test` passes (all unit and integration tests)
   - No regressions in existing functionality

## Acceptance Criteria

- [ ] All fix tasks completed and verified
- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo fmt --check` passes
- [ ] `cargo test` passes
- [ ] Architecture spec conformance verified for all changes
- [ ] No regressions in existing tests

## References

- docs/reviews/002-implementation-review.md — all findings
- docs/architecture/ — updated architecture docs
- docs/architecture/decisions/022-health-check-scope.md — ADR-022

## Notes

> This review should run after all fix tasks are complete. Focus on correctness, spec conformance, and regression testing.

## Summary

> To be filled on completion