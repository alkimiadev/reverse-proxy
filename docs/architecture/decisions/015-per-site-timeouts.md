# ADR-015: Per-Site Upstream Timeouts with Defaults

## Status

Accepted

## Context

The proxy forwards requests to upstream services. Connection and request
timeouts affect reliability — too short and legitimate slow responses fail,
too long and the proxy is vulnerable to resource exhaustion from stalled
connections.

Phase 1 initially specified global timeout defaults (5s connect, 60s request)
for all upstreams. However, different upstream services have different latency
profiles:

- Gitea (git.alk.dev): Git push operations can be slow for large repos; 60s
  may be insufficient for clone/push operations with large pack files
- Deno/Fresh (alk.dev): Fast responses expected; 60s is generous

Global timeouts don't accommodate these differences. Per-site timeout
configuration allows tuning for each upstream without affecting others.

## Decision

Add optional per-site upstream timeout configuration to `SiteConfig`. When not
specified, sensible defaults are used.

**SiteConfig additions:**

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `upstream_connect_timeout_secs` | `u64` | `5` | TCP connection timeout in seconds |
| `upstream_request_timeout_secs` | `u64` | `60` | Full request timeout in seconds |

These are part of `DynamicConfig` (hot-reloadable via ArcSwap) since they
affect per-request behavior and should not require a restart to change.

## Rationale

- Different upstreams genuinely have different latency profiles — Gitea pushes
  with large pack files need more time than a fast static site
- Defaults of 5s connect and 60s request match common reverse proxy conventions
  (nginx defaults: 60s, haproxy defaults: 30s connect, 60s server)
- Making these per-site rather than global allows tuning without side effects
- Per-site overrides in DynamicConfig means timeout changes don't require
  restarts
- The defaults are reasonable for most services; explicit configuration is only
  needed for outliers

## Consequences

**Positive:**
- Each upstream can be tuned independently
- Defaults work for most cases — explicit configuration is optional
- Hot-reloadable (part of DynamicConfig)
- Consistent with how other reverse proxies handle timeouts

**Negative:**
- Two more fields per site in config (mitigated by sensible defaults)
- Per-site timeout means the proxy must look up per-request config for each
  upstream connection (already required for routing, so no additional overhead)

## References

- [proxy.md](../proxy.md)
- [config.md](../config.md)
- OQ-06 (now resolved)