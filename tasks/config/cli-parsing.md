---
id: config/cli-parsing
name: Implement CLI argument parsing with clap and config file loading
status: completed
depends_on: [config/static-config, config/validation]
scope: narrow
risk: low
impact: component
level: implementation
---

## Description

Implement the CLI entry point using `clap` with derive macros. The CLI reads the config file, deserializes both static and dynamic portions, validates, and returns the parsed config.

### CLI Interface

```
reverse-proxy [OPTIONS]

Options:
  --config <PATH>              Path to config file [default: /etc/reverse-proxy/config.toml]
  --validate                   Validate config and exit
  --allow-wildcard-bind        Permit 0.0.0.0 as a bind address (for container deployments)
  --help                       Show help
  --version                    Show version
```

The `--allow-wildcard-bind` flag is OR'd with the config `allow_wildcard_bind` field — if either is set, wildcard binding is allowed.

### Behavior

- `--validate`: Load and validate the config, print success or errors, exit 0 or 1
- Normal run: Load, validate, return config for the startup sequence
- Config file not found: exit with error
- Config validation fails: exit with code 1 and log all errors

## Acceptance Criteria

- [ ] `clap` with derive macros for CLI parsing
- [ ] `--config` flag with default `/etc/reverse-proxy/config.toml`
- [ ] `--validate` flag: loads, validates, reports, exits
- [ ] `--allow-wildcard-bind` flag: OR'd with config value
- [ ] `--version` prints version from `Cargo.toml`
- [ ] Config file loading and TOML deserialization
- [ ] Validation runs on every load (startup and `--validate`)
- [ ] Error messages are clear and actionable
- [ ] Unit tests for CLI argument parsing
- [ ] Integration test: `--validate` with valid config exits 0
- [ ] Integration test: `--validate` with invalid config exits 1 and reports errors

## References

- docs/architecture/operations.md — CLI interface
- docs/architecture/config.md — config loading, validation
- docs/architecture/decisions/016-explicit-bind-address.md — `allow_wildcard_bind`

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion