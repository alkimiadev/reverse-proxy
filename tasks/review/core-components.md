---
id: review/core-components
name: Review core component implementations for spec conformance and pattern consistency
status: pending
depends_on: [config/static-config, config/dynamic-config, config/validation, config/cli-parsing, tls/manual-tls, tls/acme-tls, proxy/host-routing, proxy/headers-and-forwarding, proxy/error-responses]
scope: moderate
risk: low
impact: phase
level: review
---

## Description

Review the core component implementations (config, TLS, proxy) for spec conformance, pattern consistency, and correctness before proceeding to the integration and operations phase.

### Review Checklist

1. **Config conformance**:
   - StaticConfig fields match config.md exactly
   - DynamicConfig fields match config.md exactly
   - All 18 validation rules implemented
   - Default values match config.md defaults table
   - TOML deserialization works for both example configs

2. **TLS conformance**:
   - Manual mode: PEM loading, ServerConfig construction, cipher suite restriction
   - ACME mode: rustls-acme integration, challenge handling, certificate failure behavior
   - Cipher suites match ADR-012 (4 TLS 1.2 suites + all TLS 1.3)
   - Protocol versions restricted to TLS 1.2 and 1.3

3. **Proxy conformance**:
   - Host-based routing: case-insensitive, port-stripped, global routing table
   - Header injection: X-Real-IP, X-Forwarded-For (replaced), X-Forwarded-Proto, Host
   - Hop-by-hop header removal
   - Error responses: correct status codes, plain text, no information leakage
   - Request forwarding: streaming, no buffering, hyper Client configuration

4. **Pattern consistency**:
   - ArcSwap used consistently for DynamicConfig
   - ConnectInfo propagated correctly
   - Error handling patterns are consistent
   - Logging patterns are consistent

5. **Test coverage**:
   - Unit tests for config validation rules
   - Unit tests for host routing
   - Unit tests for header injection
   - Integration tests for proxy forwarding

## Acceptance Criteria

- [ ] All StaticConfig/DynamicConfig fields match config.md
- [ ] All validation rules implemented correctly
- [ ] TLS cipher suites and protocol versions match ADR-012
- [ ] Proxy headers match ADR-021 (X-Forwarded-For replaced, not appended)
- [ ] Error responses match proxy.md table
- [ ] ArcSwap pattern consistent across codebase
- [ ] Test coverage adequate for core functionality
- [ ] `cargo clippy` passes with no warnings
- [ ] `cargo fmt --check` passes
- [ ] All existing tests pass

## References

- docs/architecture/config.md
- docs/architecture/tls.md
- docs/architecture/proxy.md
- docs/architecture/decisions/ (relevant ADRs)

## Notes

> This review should verify that the core components are ready for integration. Focus on spec conformance and pattern consistency. If deviations are found, document them and decide whether to fix or accept.

## Summary

> To be filled on completion