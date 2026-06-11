# ADR-004: ACME-Primary Certificate Management

## Status

Accepted

## Context

The proxy needs TLS certificates for HTTPS. Two approaches are available:

1. **certbot (external ACME client)**: Run certbot as a cron job or systemd
   timer to obtain and renew certificates. The proxy loads certificates from
   files on disk. Renewal requires either SIGHUP/restart or inotify file
   watching to pick up new certs.

2. **rustls-acme (built-in ACME client)**: The proxy handles ACME
   certificate provisioning and renewal internally as a background task. No
   external certbot dependency. The `ResolvesServerCertAcme` cert resolver
   automatically serves the correct certificate and updates when renewed.

The alknet project has successfully implemented the rustls-acme approach, and
its patterns are directly reusable.

## Decision

Use `rustls-acme` as the primary certificate management mode, with manual
certificate paths as a fallback mode for testing, self-signed certs, and
corporate CA environments.

## Rationale

- **Eliminates certbot dependency**: No external cron job, no deploy hooks, no
  certbot package to install and maintain. The proxy is self-contained.
- **Automatic renewal**: `rustls-acme` runs as a background tokio task that
  handles certificate provisioning and renewal automatically (~30 days before
  expiry).
- **No restart needed**: When `rustls-acme` provisions a new certificate, the
  `ResolvesServerCertAcme` resolver updates atomically. No SIGHUP, no restart,
  no file watching.
- **Proven pattern**: alknet uses the same approach successfully.
- **Cache persistence**: `DirCache` persists ACME state between restarts,
  avoiding re-provisioning.
- **Fallback mode**: Manual cert paths are still supported for environments
  where ACME is not possible.

## Consequences

**Positive:**
- Single binary deployment (no certbot dependency)
- Zero-downtime certificate renewal
- Simpler operational model (no certbot cron, no deploy hooks)
- Proven in alknet

**Negative:**
- `rustls-acme` is an additional dependency
- ACME challenges require either port 80 (HTTP-01) or TLS-ALPN-01 on port 443,
  which our proxy already listens on
- Less control over certificate issuance compared to certbot (e.g., no DNS-01
  challenge support, though rustls-acme supports TLS-ALPN-01 which is sufficient
  for our use case)
- Manual mode requires restart for cert changes (acceptable for fallback)

## References

- [tls.md](../tls.md)
- alknet ADR-008: ACME/Let's Encrypt decision
- `rustls-acme` crate: https://github.com/FlorianUekermann/rustls-acme