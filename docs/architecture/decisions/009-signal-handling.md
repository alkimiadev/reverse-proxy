# ADR-009: Signal Handling Strategy

## Status

Accepted

## Context

The proxy needs to handle Unix signals for:
- **Graceful shutdown**: SIGTERM and SIGINT should stop accepting new
  connections, drain in-flight requests, then exit.
- **Config reload**: SIGHUP should trigger a DynamicConfig reload from disk.
- **Log reopen**: SIGUSR1 should close and reopen the log file, enabling
  `postrotate` logrotate configs without `copytruncate` (see review #007 W1).

Two approaches for signal handling:
- **`tokio::signal`**: Built into tokio. Handles SIGTERM and SIGINT via
  `ctrl_c()`. Does not directly handle SIGHUP.
- **`signal-hook`**: External crate. Handles all Unix signals including SIGHUP.
  More flexible but adds a dependency.

## Decision

Use `signal-hook` for all signal handling. Specifically:
- `signal-hook::flag` to set termination flags on SIGTERM/SIGINT
- `signal-hook` to register a SIGHUP handler that triggers config reload
- `signal-hook` to register a SIGUSR1 handler that reopens the log file

`tokio::signal::ctrl_c()` is registered as a secondary shutdown trigger; both
mechanisms converge on the same shutdown path. This is a belt-and-suspenders
approach: `signal-hook` handles all signals including SIGHUP, while
`ctrl_c()` provides a fallback for environments where signal handling may not
be fully wired (e.g., container runtimes).

The shutdown sequence:
1. On SIGTERM or SIGINT: stop accepting new connections, wait up to 30 seconds
   for in-flight requests to complete, then exit with code 0.
2. On SIGHUP: re-read config file, validate, and swap DynamicConfig if valid.
   Log the result.
3. On SIGUSR1: close the current log file handle and open a new one at the
   same path. Enables standard `postrotate` logrotate configs (rename + signal)
   without `copytruncate`, which creates sparse files when the FD offset is
   high. See review #007 W1.

## Rationale

- SIGHUP handling is required for config reload — `tokio::signal` doesn't
  support SIGHUP.
- `signal-hook` is well-maintained, widely used, and handles all Unix signals.
- Using one signal handling mechanism (rather than mixing `tokio::signal` and
  `signal-hook`) is simpler and avoids edge cases.
- `signal-hook::flag` is a minimal, safe API for signal-triggered flags.

## Consequences

**Positive:**
- SIGHUP for config reload is simple and well-understood
- Single signal handling mechanism for all signals
- Compatible with systemd (SIGTERM for shutdown) and standard Unix conventions

**Negative:**
- `signal-hook` is an additional dependency (but a well-established one)
- Signal handling requires careful coordination with the tokio runtime (async
  signal receivers must be properly integrated)

## References

- [operations.md](../operations.md)
- [config.md](../config.md)