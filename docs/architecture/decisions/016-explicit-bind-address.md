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
address. `0.0.0.0` is rejected during config validation. The proxy will not
start if any listener's `bind_addr` is `0.0.0.0`.

## Rationale

- A reverse proxy terminating TLS is a security boundary — it should not be
  accidentally exposed on unintended interfaces
- Single-server deployments have a specific public IP; binding to it
  deliberately is the correct operational practice
- `0.0.0.0` is often a default in example configurations and can be deployed
  without the operator realizing the service is accessible on all interfaces
- Rejecting it at validation time prevents this common mistake
- If a deployment genuinely needs to bind all interfaces, `0.0.0.0` can be
  overridden with an explicit flag, but this should be a deliberate choice
- This matches the principle of explicit over implicit for security-sensitive
  configuration

## Consequences

**Positive:**
- Prevents accidental exposure on unintended network interfaces
- Forces operators to be deliberate about which interface the proxy serves
- Config validation catches the mistake before deployment

**Negative:**
- Not suitable for deployments that genuinely need to bind all interfaces
  (mitigated by explicit override if needed in the future)
- Slightly more configuration required (operator must know their public IP)

## References

- [config.md](../config.md)
- [overview.md](../overview.md)