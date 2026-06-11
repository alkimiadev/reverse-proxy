---
id: config/static-config
name: Implement StaticConfig, ListenerConfig, TlsConfig, and LoggingConfig structs with TOML deserialization
status: completed
depends_on: [setup/project-init]
scope: moderate
risk: low
impact: component
level: implementation
---

## Description

Implement the static configuration structs that are immutable after startup. These are deserialized from the TOML config file and validated at startup. Changes to static config require a process restart.

### Structs to Implement

**StaticConfig** (top-level, immutable after startup):
- `listeners: Vec<ListenerConfig>` — at least one required
- `allow_wildcard_bind: bool` — default `false`
- `health_check_port: u16` — default `9900`, `0` to disable
- `admin_socket_path: String` — default `/run/reverse-proxy/admin.sock`, empty string to disable
- `shutdown_timeout_secs: u64` — default `30`
- `logging: LoggingConfig`

**LoggingConfig** (nested in `[logging]`):
- `level: String` — default `"info"`
- `format: String` — default `"text"`
- `log_file_path: Option<String>` — optional, enables file logging when set

**ListenerConfig** (per `[[listeners]]`):
- `bind_addr: String` — required
- `http_port: u16` — default `80`, `0` to disable
- `https_port: u16` — default `443`
- `tls: TlsConfig`
- `sites: Vec<SiteConfig>` — sites defined per listener (moved to global routing in DynamicConfig)

**TlsConfig** (nested in `[listeners.tls]`):
- `mode: String` — `"acme"` or `"manual"`
- ACME fields: `acme_domains`, `acme_cache_dir`, `acme_directory`
- Manual fields: `cert_path`, `key_path`

**SiteConfig** (per `[[listeners.sites]]`):
- `host: String` — hostname to match
- `upstream: String` — `host:port` format
- `upstream_scheme: String` — default `"http"`
- `upstream_connect_timeout_secs: u64` — default `5`
- `upstream_request_timeout_secs: u64` — default `60`

All structs derive `Debug`, `Clone`, `serde::Deserialize`. Use serde defaults for optional fields.

## Acceptance Criteria

- [ ] `StaticConfig`, `LoggingConfig`, `ListenerConfig`, `TlsConfig`, and `SiteConfig` structs defined with correct fields and types
- [ ] All structs derive `Debug`, `Clone`, `serde::Deserialize`
- [ ] Default values implemented per config.md defaults table
- [ ] TOML deserialization works for both multi-config (dedicated-IP) and shared-IP (SAN certificate) config formats
- [ ] Unit tests verify deserialization of both example configs from config.md
- [ ] `cargo check` and `cargo test` succeed

## References

- docs/architecture/config.md — full config structure, defaults, TOML format
- docs/architecture/decisions/003-toml-config.md — TOML format decision
- docs/architecture/decisions/008-static-dynamic-config-split.md — static/dynamic split rationale
- docs/architecture/decisions/019-multi-config-listeners.md — `[[listeners]]` format

## Notes

> SiteConfig is defined per-listener in TOML but collected into a global routing table in DynamicConfig. The per-listener definition is just for config organization; at runtime, hostnames must be unique across all listeners.

## Summary

> To be filled on completion