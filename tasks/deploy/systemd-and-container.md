---
id: deploy/systemd-and-container
name: Create systemd unit file, Dockerfile, and docker-compose.yml for production deployment
status: pending
depends_on: [ops/signals-and-shutdown]
scope: moderate
risk: low
impact: component
level: implementation
---

## Description

Create the deployment artifacts: systemd unit file, Dockerfile, and docker-compose.yml template.

### Systemd Unit File

```ini
[Unit]
Description=Reverse Proxy
After=network.target
Wants=network-online.target

[Service]
Type=notify
NotifyAccess=all
ExecStart=/usr/local/bin/reverse-proxy --config /etc/reverse-proxy/config.toml
Restart=on-failure
RestartSec=5

# Security hardening
NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=yes
PrivateTmp=yes
ReadWritePaths=/var/lib/reverse-proxy /var/log/reverse-proxy

# ACME challenge cache directory
StateDirectory=reverse-proxy

[Install]
WantedBy=multi-user.target
```

The proxy signals readiness to systemd via `sd_notify("READY=1")` after binding listeners and completing initial configuration load.

### Dockerfile

Multi-stage build:
- Build stage: `rust:alpine` with `x86_64-unknown-linux-musl` target for static linking
- Run stage: `alpine` (or `scratch` for absolute minimum)
- The `aws_lc_rs` crypto provider is statically linked — no OpenSSL dependency
- Binary is self-contained, no runtime dependencies beyond libc for DNS resolution

### Docker Compose Template

Example template showing:
- Reverse proxy with volume mounts for config, ACME cache, logs, and admin socket
- `allow_wildcard_bind = true` for container deployments
- Health check using `wget` against local health endpoint
- Network configuration for upstream services

### Fail2ban Configuration

- Filter definition matching the `RATE_LIMIT` log prefix
- Jail configuration for rate-limiting offenders

## Acceptance Criteria

- [ ] Systemd unit file at `deploy/reverse-proxy.service`
- [ ] `Type=notify` with `sd_notify("READY=1")` integration in binary
- [ ] Security hardening directives in unit file
- [ ] `ReadWritePaths` for ACME cache and log directory
- [ ] Dockerfile with multi-stage build (`rust:alpine` → `alpine`/`scratch`)
- [ ] Static linking with `x86_64-unknown-linux-musl` target
- [ ] Docker Compose template at `deploy/docker-compose.yml`
- [ ] Volume mounts for config (ro), ACME cache (rw), logs (rw), admin socket (rw)
- [ ] Health check in Docker Compose using `wget` against `http://127.0.0.1:9900/health`
- [ ] Fail2ban filter definition at `deploy/fail2ban/filter.d/reverse-proxy.conf`
- [ ] Fail2ban jail configuration at `deploy/fail2ban/jail.d/reverse-proxy.conf`
- [ ] `docker build` succeeds
- [ ] Container starts and responds to health check

## References

- docs/architecture/operations.md — systemd, container deployment, fail2ban, health check
- docs/architecture/decisions/020-container-deployment.md — container model rationale

## Notes

> The Dockerfile should use `musl` for static linking. The `aws_lc_rs` crypto provider is statically linked. The resulting binary has no runtime dependencies beyond libc for DNS resolution (which `musl` provides).

## Summary

> To be filled on completion