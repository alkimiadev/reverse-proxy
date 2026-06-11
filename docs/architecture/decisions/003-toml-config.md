# ADR-003: TOML Configuration Format

## Status

Accepted

## Context

The proxy needs a configuration file format for defining sites, TLS settings,
bind addresses, and rate limits. Options include TOML, YAML, JSON, and custom
binary formats.

## Decision

Use TOML as the configuration file format.

## Rationale

- **Rust-native**: TOML is the configuration format for Cargo (Rust's package
  manager). The Rust ecosystem has first-class TOML support via `serde` +
  `toml` crate.
- **Unambiguous**: TOML has a single canonical representation for any given
  data structure, unlike YAML which has multiple equivalent representations and
  surprising type coercion rules (e.g., `no` → boolean, `1.0` → float).
- **Human-friendly**: TOML is easy to read and write for simple configurations
  like ours. It supports sections (tables), arrays, and inline tables.
- **Good error messages**: The `toml` crate provides clear deserialization
  error messages pointing to the exact field that failed.

## Consequences

**Positive:**
- Familiar to Rust developers (Cargo.toml)
- Clear, unambiguous syntax
- Excellent serde integration with detailed error reporting
- No type coercion surprises

**Negative:**
- Not as widely used for config outside Rust (but our audience is ourselves)
- No `#include` or file composition (each config file is self-contained)

## References

- [config.md](../config.md)