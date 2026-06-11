---
id: config/validation
name: Implement config validation with all 18 validation rules and error reporting
status: pending
depends_on: [config/static-config]
scope: moderate
risk: medium
impact: component
level: implementation
---

## Description

Implement comprehensive config validation per the 18 rules defined in config.md. Validation runs on startup (fail-fast, exit with non-zero code) and on reload (reject reload, log error).

### Validation Rules (from config.md)

1. At least one `[[listeners]]` entry must exist
2. Each listener's `bind_addr` is not `0.0.0.0` unless `allow_wildcard_bind` is enabled (config OR CLI flag — OR relationship)
3. Each listener's `bind_addr` and `https_port` combination must be unique
4. In ACME mode, `acme_domains` must be non-empty
5. In manual mode, `cert_path` and `key_path` must both be set and files must be readable
6. Each site must have a `host` and `upstream`
7. Site `host` values must be unique across all listeners (no duplicate hostnames)
8. `rate_limit.requests_per_second` must be > 0
9. `body.limit_bytes` must be > 0
10. Each listener's `bind_addr` and `http_port` combination must be unique (if http_port > 0)
11. Within a listener, `http_port` and `https_port` must differ
12. `https_port` must be 1–65535 (required — TLS needs a port)
13. `http_port` must be 0 (disabled) or 1–65535
14. `health_check_port` must not conflict with any listener's `http_port` or `https_port` on the same bind address
15. Site `host` values must not include a port number (e.g., `git.alk.dev`, not `git.alk.dev:443`)
16. Site `host` values must be valid hostnames (not IP addresses, not including ports). Hostnames normalized to lowercase
17. `upstream` must be in `host:port` format where `port` is 1–65535
18. `upstream_scheme` values must be `"http"` or `"https"` (lowercase)

### Error Reporting

On validation failure, collect ALL errors (don't stop at first) and report them together. This helps operators fix multiple issues in one pass. Use a `Vec<ValidationError>` that is logged or printed on startup failure.

### Startup vs Reload Behavior

- **Startup**: If validation fails, exit with non-zero code and log all validation errors
- **Reload**: If validation fails, reject the reload, log all errors, keep old DynamicConfig active

## Acceptance Criteria

- [ ] All 18 validation rules implemented
- [ ] Validation collects all errors (doesn't stop at first)
- [ ] `ValidationError` enum with descriptive messages for each rule
- [ ] `validate(config: &StaticConfig, dynamic: &DynamicConfig) -> Result<(), Vec<ValidationError>>` function
- [ ] Startup validation: exits with code 1 on failure, logs all errors
- [ ] Reload validation: rejects reload on failure, logs all errors, keeps old config
- [ ] `allow_wildcard_bind` OR logic: config flag OR CLI flag enables it
- [ ] Hostname normalization to lowercase during validation
- [ ] File existence check for manual mode `cert_path` and `key_path`
- [ ] Unit tests covering each validation rule with valid and invalid inputs
- [ ] Integration test: valid config from config.md examples passes all validation

## References

- docs/architecture/config.md — full validation rules, default values, TOML format
- docs/architecture/decisions/016-explicit-bind-address.md — `0.0.0.0` rejection rationale
- docs/architecture/decisions/020-container-deployment.md — `allow_wildcard_bind` for containers

## Notes

> Rule 5 (file readability check for manual certs) should check that the files exist and are readable at validation time, not just that the paths are set. This provides early feedback on misconfiguration.

## Summary

> To be filled on completion