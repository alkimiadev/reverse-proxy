# ADR-019: Multi-Config Listener Support

## Status

Accepted

## Context

OQ-07 asked whether per-site TLS overrides should be supported for mixed
ACME/manual domains. The original framing assumed a single listener with one
TLS configuration, where the question was about mixing certificate sources
within that listener.

However, there are two distinct deployment models for multi-domain TLS:

1. **Shared-IP multi-domain**: One IP address, one TLS listener, one SAN
   certificate covering multiple domains via SNI. All domains share the same
   ACME configuration. This is what the current architecture documents describe.

2. **Dedicated-IP single-domain (multi-config)**: Each IP address gets its own
   SSL certificate for its own domain. In this model, 1 IP = 1 SSL = 1 domain.
   Each listener is independently configured with its own bind address, TLS
   certificate, and site mapping. No SNI multiplexing is needed because each
   domain has its own IP.

The actual deployment uses model 2 (dedicated-IP single-domain). The proxy
should support both models, and the choice between them should be a deployment
concern, not an architectural limitation.

There are two approaches to supporting model 2:

- **Multiple instances**: Run separate `reverse-proxy` processes, each with
  their own config file binding to a different IP. Simple, isolated, no
  code changes needed.
- **Multi-listener single process**: One process with a `[[listeners]]`
  config that defines multiple independent listeners, each with their own
  bind address, TLS config, and site mapping. More complex but shares
  resources (process overhead, connection pool, logging).

## Decision

Support both deployment models by extending the configuration format with
`[[listeners]]`, where each listener is an independent TLS endpoint with its
own bind address, TLS configuration, and site routing.

The config format changes from a single implicit listener to explicit
`[[listeners]]` entries:

```toml
# Multi-config: two listeners, each on a different IP
[[listeners]]
bind_addr = "203.0.113.10"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["git.alk.dev"]
acme_cache_dir = "/var/lib/reverse-proxy/acme-cache"
acme_directory = "production"

[[listeners.sites]]
host = "git.alk.dev"
upstream = "127.0.0.1:3000"

[[listeners]]
bind_addr = "203.0.113.11"
http_port = 80
https_port = 443

[listeners.tls]
mode = "manual"
cert_path = "/etc/ssl/alk.dev/fullchain.pem"
key_path = "/etc/ssl/alk.dev/privkey.pem"

[[listeners.sites]]
host = "alk.dev"
upstream = "127.0.0.1:8080"
```

Each `[[listeners]]` entry is an independent TLS endpoint with:
- Its own `bind_addr` (required, no `0.0.0.0`; see ADR-016)
- Its own `http_port` and `https_port`
- Its own `tls` configuration (ACME or manual)
- Its own `[[sites]]` array for host-based routing

The single-listener case is a natural subset of this format — a config with
one `[[listeners]]` entry behaves identically to the current single-listener
design. The multi-domain SAN case (one IP, one cert, multiple domains) is
also supported: a single listener with multiple domains in `acme_domains`
and multiple `[[sites]]`.

Global settings (logging, rate limiting, body limits, health check, admin
socket) are top-level configuration. Listener-specific settings (bind address,
ports, TLS, sites) live inside each `[[listeners]]` entry.

Example configuration:

```toml
# Global settings
health_check_port = 9900
admin_socket_path = "/run/reverse-proxy/admin.sock"

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

# Listener definitions
[[listeners]]
bind_addr = "203.0.113.10"
# ... listener config ...
```

## Rationale

- The dedicated-IP model (1 IP = 1 SSL = 1 domain) is our actual deployment.
  The architecture should natively support it.
- The shared-IP multi-domain model (SAN certificate) is also common and useful.
  Supporting both is straightforward with the `[[listeners]]` format.
- Multiple instances would work but wastes resources (separate processes,
  separate connection pools, separate logging pipelines) for what is
  fundamentally the same binary doing the same job.
- A single process with multiple listeners is more efficient: one process, one
  log pipeline, one config reload mechanism, one health check endpoint.
- The `[[listeners]]` format is a natural extension — each listener is
  independent, but global concerns (logging, rate limiting, health checks)
  are shared, which is correct since they're all the same proxy.
- The single-listener case is a degenerate case of `[[listeners]]` with one
  entry, so existing configurations translate trivially.

## Consequences

**Positive:**
- Natively supports both deployment models (dedicated-IP and shared-IP)
- Single process for all listeners: shared logging, config reload, health
  checks, rate limiting
- Each listener is fully independent — different IPs, different TLS configs,
  different domains
- The config format is regular and predictable — `[[listeners]]` is a
  natural TOML array of tables
- Mixed ACME/manual configurations are naturally supported: each listener
  chooses its own TLS mode
- The single-listener, single-domain case is trivially supported

**Negative:**
- Config format change from `[server]` with implicit single listener to
  `[[listeners]]` with explicit listener entries
- The proxy must manage multiple TCP listeners and TLS acceptors, which
  adds implementation complexity
- Each listener needs its own ACME state machine (in ACME mode), increasing
  resource usage proportional to listener count
- Global rate limiting applies across all listeners (per-IP rate limiting
  doesn't distinguish which listener received the request — this is correct
  behavior since the IP is the same regardless of listener)

## References

- [config.md](../config.md)
- [tls.md](../tls.md)
- [overview.md](../overview.md)
- ADR-008 (static/dynamic config split)
- ADR-011 (multi-domain TLS config)
- ADR-016 (explicit bind address)
- OQ-05 (multiple bind addresses — now resolved via per-listener `bind_addr`)