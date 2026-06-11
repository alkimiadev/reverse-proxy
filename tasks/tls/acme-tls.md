---
id: tls/acme-tls
name: Implement ACME certificate provisioning with rustls-acme for automatic Let's Encrypt
status: complete
depends_on: [setup/project-init]
scope: moderate
risk: high
impact: component
level: implementation
---

## Description

Implement ACME mode TLS certificate provisioning using `rustls-acme`. Each listener in ACME mode creates its own `AcmeCertProvider` with the listener's domain list, cache directory, and Let's Encrypt directory.

### ACME Mode

For each listener in ACME mode:
1. Create `AcmeConfig::new(domains)` with the domains from `acme_domains`
2. Configure the ACME state machine as a background tokio task per listener
3. `ResolvesServerCertAcme` serves the ACME-provisioned certificate
4. Certificate renewal is automatic (~30 days before expiry)
5. Cache directory persists ACME state between restarts via `DirCache`

### Certificate Failure Behavior

| Scenario | Behavior |
|----------|----------|
| First start, no cached cert, ACME unreachable | **Fail to start** with clear error |
| First start, no cached cert, ACME succeeds | Normal startup |
| Start with cached cert, ACME unreachable for renewal | **Start normally** with cached cert, log `warn` |
| Renewal failure after startup | **Continue serving existing cert**, log `warn` |
| Cached cert expired, renewal fails at startup | **Fail to start** |
| Cached cert expires during runtime | **Continue serving expired cert**, log `error` |

Key principle: **never start without a valid TLS certificate, but always continue serving if a valid cert exists**.

### ACME Challenge Type

Default is TLS-ALPN-01 since the proxy already listens on port 443. HTTP-01 is available as a fallback via the port 80 redirect listener serving `/.well-known/acme-challenge/{token}`.

### ServerConfig for ACME Mode

Build `ServerConfig` with `with_cert_resolver()`, passing the `ResolvesServerCertAcme` resolver. Register `acme-tls/1` in `alpn_protocols` for TLS-ALPN-01 challenge handling.

## Acceptance Criteria

- [ ] ACME state machine runs as background tokio task per listener
- [ ] `AcmeConfig` created per listener with correct domains, cache dir, and directory
- [ ] `ResolvesServerCertAcme` integrated into `ServerConfig`
- [ ] `acme-tls/1` ALPN protocol registered for TLS-ALPN-01 challenges
- [ ] Cipher suite and protocol version restrictions applied (same as manual mode)
- [ ] Certificate failure behavior matches the table above
- [ ] Cache directory (`DirCache`) persists ACME state between restarts
- [ ] Each listener uses its own cache directory to avoid conflicts
- [ ] ACME renewal is automatic, no manual intervention
- [ ] `staging` vs `production` ACME directory selection works
- [ ] Unit tests for ACME config construction (mocked, not real Let's Encrypt calls)

## References

- docs/architecture/tls.md — ACME mode, certificate failure behavior, challenge types
- docs/architecture/decisions/004-rustls-acme.md — ACME-primary rationale
- docs/architecture/decisions/005-tokio-rustls-direct.md — direct tokio-rustls for ACME integration

## Notes

> Real ACME integration tests require a network connection to Let's Encrypt staging. For CI, consider mock tests that verify the config and state machine setup without making real ACME requests. Manual testing against staging should be done before deployment.

## Summary

> To be filled on completion