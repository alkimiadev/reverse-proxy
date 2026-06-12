# ADR-026: Connector Timeout Ceiling for Per-Site Timeouts

## Status

Accepted

## Context

ADR-015 specifies per-site upstream connect timeout configuration with a default
of 5 seconds. The proxy enforces connect timeouts using two mechanisms:

1. **`tokio::time::timeout`**: Wraps the entire `client.request()` call with the
   per-site `upstream_connect_timeout_secs` value.
2. **`HttpConnector::set_connect_timeout()`**: Sets the TCP-level connect timeout
   on the hyper `HttpConnector` inside the shared client.

The problem: the HTTP connector's `set_connect_timeout()` is set once at client
creation time and applies to all requests through that client. The current
implementation hardcodes this to 5 seconds. Since the connector's internal
timeout fires before `tokio::time::timeout`, any per-site connect timeout
greater than 5 seconds is silently capped — the connector times out at 5s
regardless of the configured value.

Three approaches exist:

1. **Raise the connector timeout to a high ceiling**: Set the connector's
   `set_connect_timeout` to a value higher than any reasonable per-site timeout
   (e.g., 30s). Let `tokio::time::timeout` enforce the actual per-site limit.
   The connector timeout becomes a safety ceiling, not the primary enforcement
   mechanism.

2. **Remove the connector timeout entirely**: Set `set_connect_timeout` to
   `None` and rely solely on `tokio::time::timeout`. This removes one layer
   of timeout enforcement but simplifies the model.

3. **Create per-site client instances**: Each site gets its own hyper Client
   with its own connector configured with the site's connect timeout. This is
   the most precise approach but creates many client instances and connection
   pools, increasing resource usage.

## Decision

Use approach 1: set the connector timeout to a high ceiling value (30 seconds)
and let `tokio::time::timeout` enforce the actual per-site connect timeout.

The connector timeout serves as a safety ceiling — it ensures that even if the
`tokio::time::timeout` wrapper fails or is misconfigured, TCP connections
cannot hang indefinitely. The ceiling of 30s is well above the default 5s and
any reasonable per-site override.

The `tokio::time::timeout` wrapper with the per-site value is the primary
enforcement mechanism. It fires at the correct per-site threshold.

## Rationale

- The shared client architecture (ADR-017) means one connector timeout for all
  sites. Creating per-site clients would undermine connection pooling and
  increase resource usage.
- A ceiling approach preserves the defense-in-depth benefit of two timeout
  layers while allowing per-site values to actually work.
- 30s is a reasonable ceiling — no legitimate upstream connect should take
  longer than 30s. Sites that need a higher connect timeout can set the ceiling
  even higher if needed, but 30s covers all practical cases.
- Removing the connector timeout (approach 2) removes the safety ceiling
  entirely. If `tokio::time::timeout` has a bug or is misapplied, TCP connects
  could hang indefinitely. The ceiling provides a backstop.
- The HTTPS client uses `HttpsConnector<HttpConnector>`, which wraps the
  `HttpConnector`. The same ceiling applies to both HTTP and HTTPS clients.

## Consequences

**Positive:**
- Per-site connect timeouts work as documented (ADR-015)
- Maintains defense-in-depth with two timeout layers
- No change to the shared client / connection pooling architecture
- Simple implementation: change one constant

**Negative:**
- The connector timeout no longer matches the default connect timeout (5s
  default vs. 30s ceiling). Operators who read the connector timeout might
  be confused — documentation must make the ceiling role clear.
- If a site needs a connect timeout > 30s, the ceiling must be raised. This
  is unlikely in practice but creates a hidden upper bound.

## References

- [proxy.md](../proxy.md) — Upstream connection, per-site timeouts
- ADR-015 — Per-site upstream timeouts with defaults
- ADR-017 — Upstream connection defaults
- Security Review C3 — Connect timeout silently capped at 5s