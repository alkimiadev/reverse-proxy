---
id: setup/test-infrastructure
name: Set up test infrastructure with integration test helpers and fixtures
status: completed
depends_on: [setup/project-init]
scope: narrow
risk: low
impact: component
level: implementation
---

## Description

Set up the testing infrastructure that subsequent implementation tasks will use. This includes integration test directory structure, test helpers for creating mock configs, and HTTP test utilities.

Create:

1. **Test module structure**: `tests/` directory for integration tests, `src/config/test_fixtures.rs` for config test helpers
2. **Test config fixtures**: Helper functions to create valid `StaticConfig` and `DynamicConfig` instances for tests (minimal valid config that passes validation)
3. **HTTP test helpers**: Utilities for spinning up test HTTP servers (for upstream mocking) using `hyper`'s test server or `tokio::net::TcpListener`
4. **Test TLS helpers**: Self-signed certificate generation for TLS tests (using `rcgen` dev-dependency)

## Acceptance Criteria

- [ ] `tests/` directory exists with a sample integration test that compiles
- [ ] Test helper module with `test_static_config()` and `test_dynamic_config()` fixture functions
- [ ] `rcgen` added as a dev-dependency for self-signed cert generation
- [ ] `tokio-test` or equivalent test utilities available
- [ ] `cargo test` succeeds with the skeleton test
- [ ] Test config fixtures produce configs that would pass validation (once validation is implemented)

## References

- docs/architecture/config.md — config structures to create fixtures for
- docs/architecture/proxy.md — proxy handler that will need upstream mocking

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion