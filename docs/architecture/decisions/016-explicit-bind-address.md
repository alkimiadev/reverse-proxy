# ADR-016: Explicit Bind Address Requirement

## Status

Accepted

## Context

The proxy's `bind_addr` configuration field specifies the IP address to bind
to. The default for most network services is `0.0.0.0` (bind all interfaces),
which means the service is accessible on every network interface, including
public-facing ones.

For a reverse proxy that terminates TLS and handles security-sensitive traffic,
binding to all interfaces is risky. The proxy should only listen on the
intended interface(s) — typically a specific public IP for a single-server
deployment.

## Decision

The `bind_addr` field on each `[[listeners]]` entry must be an explicit IP
address. `0.0.0.0` is rejected during config validation unless explicitly
overridden. The proxy will not start if any listener's `bind_addr` is
`0.0.0.0` and the override is not enabled.

**Override mechanism**: `allow_wildcard_bind = true` in the config file, or
`--allow-wildcard-bind` on the command line. This flag permits `0.0.0.0` as a
valid bind address. It is intended for container deployments where the
container's network namespace is isolated and `0.0.0.0` is the correct address
(Docker publishes only the explicitly mapped ports).

## Rationale

- A reverse proxy terminating TLS is a security boundary — it should not be
  accidentally exposed on unintended interfaces
- Single-server deployments have a specific public IP; binding to it
  deliberately is the correct operational practice
- `0.0.0.0` is often a default in example configurations and can be deployed
  without the operator realizing the service is accessible on all interfaces
- Rejecting it at validation time prevents this common mistake
- In a container, `0.0.0.0` is correct and safe because the container's
  network namespace isolates it — only Docker-published ports are reachable
  from outside the container
- The override is opt-in so it must be a deliberate choice, not an accident
- This matches the principle of explicit over implicit for security-sensitive
  configuration

## Consequences

**Positive:**
- Prevents accidental exposure on unintended network interfaces in bare-metal
  deployments
- Forces operators to be deliberate about which interface the proxy serves
- Config validation catches the mistake before deployment
- Container deployments work correctly with `0.0.0.0` when the override is
  explicitly enabled

**Negative:**
- Container deployments require one extra config flag (`allow_wildcard_bind =
  true`) — a minor operational step
- Slightly more configuration required for bare-metal (operator must know
  their public IP)

## References

- [ADR-020: Container Deployment Model](020-container-deployment.md)
- [config.md](../config.md)
- [overview.md](../overview.md)