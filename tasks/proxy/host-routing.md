---
id: proxy/host-routing
name: Implement Host-based routing with global routing table from DynamicConfig
status: pending
depends_on: [config/dynamic-config]
scope: narrow
risk: low
impact: component
level: implementation
---

## Description

Implement the host-based routing that matches incoming requests to site definitions. Sites are defined per-listener in TOML but collected into a single global routing table in `DynamicConfig`.

### Routing Logic

1. Check for `/health` path — if matched, return 200 OK with empty body (regardless of Host)
2. Extract `Host` header from request
3. If no `Host` header, return `400 Bad Request`
4. Normalize `Host` to lowercase, strip port component (e.g., `git.alk.dev:443` → `git.alk.dev`)
5. Look up normalized host in the global routing table
6. If found, forward to the matching `SiteConfig`'s upstream
7. If not found, return `404 Not Found`

### Global Routing Table

The routing table is a `HashMap<String, SiteConfig>` (or similar) in `DynamicConfig`, built by collecting all sites from all listeners. Hostnames must be unique — validation enforces this.

The routing table is part of `DynamicConfig` and is swapped atomically on config reload. This means a config reload can add, remove, or change site routing without restarting.

## Acceptance Criteria

- [ ] Host-based routing extracts `Host` header and normalizes to lowercase
- [ ] Port component stripped from `Host` header before matching
- [ ] `/health` path matches regardless of `Host` header, returns 200 OK
- [ ] Missing `Host` header returns `400 Bad Request`
- [ ] Unknown host returns `404 Not Found`
- [ ] Global routing table built from all listeners' site definitions
- [ ] Routing table updates atomically on config reload via ArcSwap
- [ ] Case-insensitive host matching per RFC 7230 §2.7.3
- [ ] Unit tests for host normalization (case, port stripping)
- [ ] Unit tests for routing table lookup (match, no match)

## References

- docs/architecture/proxy.md — Host-based routing section
- docs/architecture/config.md — DynamicConfig, global routing table

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion