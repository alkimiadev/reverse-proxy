# ADR-007: Custom Structured Log Format for Fail2ban

## Status

Accepted

## Context

The proxy needs to produce log output that fail2ban can parse to detect and ban
abusive IP addresses. The current nginx setup uses nginx's default log format
with standard fail2ban filters.

Options for fail2ban integration:
- **nginx-compatible format**: Replicate nginx's log format so existing
  fail2ban filters work unchanged. Couples us to nginx's format.
- **Custom structured format**: Design a clean, parseable format with a
  corresponding custom fail2ban filter. Gives us control and clarity.
- **JSON format**: Machine-readable but harder for fail2ban regex matching.

## Decision

Use a custom structured log format with a corresponding custom fail2ban filter.

The format for rate-limited requests:

```
RATE_LIMIT client_ip=<IP> host=<host> path=<path> status=429
```

The format for general access logs:

```
REQUEST client_ip=<IP> host=<host> method=<METHOD> path=<path> status=<code> upstream=<addr> duration_ms=<ms>
```

A corresponding fail2ban filter (`/etc/fail2ban/filter.d/reverse-proxy.conf`)
uses regex matching on the `RATE_LIMIT` prefix and `client_ip=<HOST>` field.

## Rationale

- Custom format is clear, unambiguous, and self-documenting
- No coupling to nginx's format, which may change or include fields we don't
  produce
- `key=value` pairs are easy to parse with regex and easy to extend
- The `RATE_LIMIT` prefix makes it trivial to distinguish rate-limit events
  from other logs
- Writing a custom fail2ban filter is straightforward (5 lines of config)
- We control both sides (the proxy and the filter), so compatibility is
  guaranteed

## Consequences

**Positive:**
- Clean, purpose-built format
- Easy to extend with new fields
- No dependency on nginx log format
- Custom fail2ban filter is simple to maintain

**Negative:**
- Cannot reuse existing nginx fail2ban filters (trivial to write our own)
- Existing fail2ban configurations need updating (acceptable since we're
  replacing nginx entirely)

## References

- [operations.md](../operations.md)
- [open-questions.md](../open-questions.md) OQ-02 (now resolved)