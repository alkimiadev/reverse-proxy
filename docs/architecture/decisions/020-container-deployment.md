# ADR-020: Container Deployment Model

## Status

Accepted

## Context

The reverse proxy replaces an nginx instance running directly on the host, which
is vulnerable to multiple actively-exploited CVEs (including CVE-2026-42945,
unauthenticated RCE). Rust's memory safety eliminates the bug class responsible
for most nginx CVEs, but containerization adds a second independent barrier.

Running the proxy in a minimal Docker container means that even if an attacker
finds a logic-level vulnerability in the proxy, they must also escape the
container boundary. The probability of chaining a Rust proxy exploit with a
container escape is materially lower than exploiting any single layer alone.

Additionally, the proxy must support flexible upstream addressing. While the
initial deployment runs all services on the same host (Gitea, Postgres, the
proxy itself in containers sharing a Docker network), upstreams may also be on
different machines — reachable via LAN IP, VPN, or SSH-based tunnel (alknet).
The configuration must not assume same-host deployment.

Finally, fail2ban integration requires reliable log access. Docker log drivers
(journald, json-file) introduce complexity and fragility for log parsing. A
log file on a volume mount is simpler, more reliable, and easier for fail2ban
to consume directly from the host filesystem.

## Decision

1. **The proxy runs in a minimal Docker container** (multi-stage build:
   compile in `rust:alpine`, run in `alpine` or `scratch`). The container
   image contains only the static binary and necessary runtime files.

2. **Bind address `0.0.0.0` is allowed inside containers** via an explicit
   opt-in flag (`allow_wildcard_bind = true` in config, or
   `--allow-wildcard-bind` CLI flag). The default remains: reject `0.0.0.0`
   as a bind address. In a container, binding `0.0.0.0` is correct and safe
   because the container's network namespace is isolated — it only exposes
   interfaces Docker publishes via `-p` flags. See ADR-016 for the original
   rationale and this override.

3. **Upstream addressing is unopinionated**. The `upstream` field in
   `SiteConfig` accepts any `host:port` combination:
   - Docker DNS names (`gitea:3000`) for containers on the same Docker network
   - Loopback addresses (`127.0.0.1:3000`) for host-network or sidecar
     deployments
   - LAN IPs (`10.0.0.5:3000`) for same-network services
   - VPN or tunnel endpoints as needed
   The proxy makes no assumptions about upstream locality.

4. **Logging is file-primary for fail2ban integration.** The proxy writes
   structured logs to a file (default: `/var/log/reverse-proxy/access.log`)
   on a volume mount. stdout/stderr logging is always-on (for `docker logs`
   and `journalctl`). File logging is the authoritative source for fail2ban
   because it avoids the fragility of Docker log driver parsing.

 5. **ACME state and admin key are volume-mounted.** The ACME cache directory
    (`/var/lib/reverse-proxy/acme-cache/`) and admin key file
    (`/etc/reverse-proxy/admin-key`) are mounted as volumes so state persists
    across container restarts and the host can send authenticated reload commands
    (ADR-028).

6. **Health checks use Docker's native mechanism.** The health check endpoint
   on port 9900 (localhost only) is used directly by Docker's `HEALTHCHECK`
   directive. No port publishing needed.

7. **SSH passthrough for Gitea** remains on the host. The proxy does not handle
   SSH traffic. Port 22 traffic is routed directly to the Gitea container by
   Docker, not through the reverse proxy. This matches the current nginx
   deployment where SSH is independent.

## Consequences

**Positive:**
- Defense-in-depth: memory-safe language + container isolation + minimal image
- Flexible upstream addressing supports same-host, LAN, VPN, and tunnel
  deployments without config changes
- File-based logging is simple and reliable for fail2ban — no Docker log
  driver parsing required
- ACME state persists across container restarts via volume mounts
- Health check integrates natively with Docker's orchestration

**Negative:**
- Slightly more complex deployment (Docker Compose, volume mounts, network
  configuration) compared to a bare binary
- `0.0.0.0` override must be documented clearly to prevent misuse in
  non-container deployments
- File logging requires a volume mount, adding a deployment step
- Container rebuilds are needed for binary updates (mitigated by CI/CD)

## References

- [ADR-016: Explicit Bind Address Requirement](016-explicit-bind-address.md)
- [overview.md](../overview.md)
- [operations.md](../operations.md)
- [config.md](../config.md)