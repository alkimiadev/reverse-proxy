# ADR-024: ANSI-Disabled Logging for Container Deployments

## Status

Accepted

## Context

During deployment, the proxy's log output contained ANSI escape codes (color
codes) because `tracing-subscriber`'s default `fmt::layer()` enables ANSI
output when connected to a terminal. In a Docker container, `docker logs`
captures stdout/stderr, and the log file written to
`/var/log/reverse-proxy/access.log` is also a plain text file.

ANSI escape codes in logs cause two problems:
1. **fail2ban regex failure**: The fail2ban filter regex expects plain text with
   a `RATE_LIMIT` prefix. ANSI codes embedded in the log line before the prefix
   break pattern matching, causing fail2ban to miss rate limit events entirely.
2. **Docker log readability**: `docker logs` output is cluttered with escape
   sequences when not running in a terminal that supports them.

## Decision

All `tracing-subscriber` fmt layers now use `with_ansi(false)`:

- **File layer**: Always plain text, no ANSI codes
- **Stdout layer**: Always plain text, no ANSI codes
- **JSON layer**: Always plain text (JSON format doesn't benefit from colors)

This applies to both text and JSON log formats, in both file and stdout
destinations.

Additionally, the fail2ban regex was corrected: the `^` anchor was removed from
the `failregex` pattern because log lines have a timestamp/level prefix before
the `RATE_LIMIT` keyword. The corrected pattern matches `RATE_LIMIT` anywhere
in the line rather than only at the start.

## Consequences

**Positive:**
- fail2ban regex matching works reliably in all environments
- Log output is clean and parseable regardless of environment
- No behavioral difference between Docker, systemd, and terminal environments

**Negative:**
- Loss of color-coding in terminal output during development (acceptable
  trade-off for reliability; developers can use `RUST_LOG` filtering instead)

## References

- [operations.md](../operations.md) — logging and fail2ban integration
- [ADR-007](007-custom-log-format.md) — custom structured log format
- [ADR-020](020-container-deployment.md) — container deployment model