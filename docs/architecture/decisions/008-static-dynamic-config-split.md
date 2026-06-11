# ADR-008: Static/Dynamic Configuration Split with ArcSwap

## Status

Accepted

## Context

The proxy needs configuration that can be partially reloaded at runtime (site
definitions, rate limits) without restarting the process and dropping active
connections. However, some configuration (bind addresses, TLS mode) fundamentally
requires creating new listeners and cannot be changed at runtime.

Two approaches:
- **Full restart for all config changes**: Simple, but requires dropping
  active connections for every change, including trivial rate limit adjustments.
- **Static/dynamic split**: Immutable parameters (bind address, TLS mode) in a
  `StaticConfig` that requires restart; runtime-adjustable parameters (sites,
  rate limits) in a `DynamicConfig` that can be atomically swapped via
  `Arc<ArcSwap<DynamicConfig>>` without dropping connections.

This pattern is proven in the alknet project, which uses the same
`ArcSwap<DynamicConfig>` approach for auth policy, forwarding rules, and rate
limits.

## Decision

Split configuration into `StaticConfig` (immutable after startup) and
`DynamicConfig` (hot-reloadable via `ArcSwap`). The split is:

**StaticConfig** (restart required):
- Bind address, HTTP port, HTTPS port
- TLS mode (ACME vs. manual), cert paths, ACME settings
- Log level and format

**DynamicConfig** (hot-reloadable via SIGHUP):
- Site definitions (hostname → upstream mappings)
- Rate limits (requests per second, burst)
- Body size limits

`ConfigReloadHandle` provides a `reload(DynamicConfig)` method that atomically
swaps the entire config. All request handlers read `DynamicConfig` via
`ArcSwap::load()` — a lock-free operation.

## Rationale

- Rate limits and site definitions change more frequently than bind addresses
  and TLS settings. Hot-reload avoids unnecessary downtime.
- `ArcSwap` provides lock-free reads and atomic writes — no partial updates,
  no lock contention on the hot path.
- Proven pattern from alknet, where it's used for auth policy, forwarding
  rules, and rate limits.
- SIGHUP trigger is simple, well-understood, and compatible with systemd and
  process supervisors.
- The entire config is swapped at once, preventing inconsistent states where
  some sites use the old config and others use the new one.

## Consequences

**Positive:**
- Zero-downtime config reload for sites and rate limits
- Lock-free reads on the request hot path
- Atomic config updates — no partial states
- Proven pattern from alknet

**Negative:**
- Two config types add conceptual complexity
- SIGHUP reload requires reading the config file from disk (need to handle
  file read errors gracefully)
- Must validate DynamicConfig before swapping (invalid config must not replace
  valid config)

## References

- [config.md](../config.md)
- alknet ADR-030 (static/dynamic config split)