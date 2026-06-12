# ADR-027: Admin Socket Resource Limits

## Status

Accepted

## Context

The admin Unix domain socket (ADR-014) accepts connections from local
processes and reads one newline-terminated command per connection. The current
implementation has no read timeout and no line length limit.

Two attack vectors exist for processes with access to the admin socket
(controlled by Unix file permissions):

1. **Connection hold**: A client connects and sends no data, keeping a
   connection and tokio task open indefinitely. An attacker with socket access
   can open many such connections to exhaust file descriptors or task slots.

2. **Unbounded memory allocation**: A client sends data without a newline,
   causing `read_line` to buffer indefinitely. This allows unbounded memory
   consumption from a single connection.

While the admin socket is protected by Unix file permissions (only processes
with access to the socket file can connect), defense-in-depth warrants basic
resource limits. The socket is a local diagnostic interface, not a
high-performance endpoint — strict limits are appropriate.

## Decision

Apply two resource limits to admin socket connections:

1. **Read timeout**: 5 seconds per connection. If no complete command is
   received within 5 seconds, the connection is closed and a timeout error
   is logged at `debug` level.

2. **Line length limit**: 4096 bytes per command. If a client sends more
   than 4096 bytes without a newline, the connection is closed and the event
   is logged at `warn` level. The longest valid command (`reload`) is 6 bytes —
   4096 bytes provides ample room for future commands while preventing abuse.

Both limits apply per connection. The admin socket creates one tokio task per
connection, so the limits also bound per-connection resource usage.

## Rationale

- Defense-in-depth: even with Unix permission protection, basic resource
  limits prevent accidental or deliberate resource exhaustion.
- 5 seconds is generous for a local socket — a local process should be able
  to send a command within milliseconds. The timeout handles stuck clients and
  network issues on tunnel-mounted sockets.
- 4096 bytes is 600x the longest current command. It accommodates future
  multi-field commands without allowing abuse.
- These limits have no impact on legitimate admin socket usage — the current
  commands (`reload`, `status`) are tiny and complete instantly.

## Consequences

**Positive:**
- No unbounded memory allocation from admin socket connections
- No indefinite connection holding
- Simple implementation: `tokio::time::timeout` + `tokio::io::take` (or
  `read_until` with a byte limit)
- No impact on legitimate usage

**Negative:**
- If a future admin command requires more than 4096 bytes (unlikely), the
  limit must be raised.
- A 5-second timeout could cause issues if the admin socket is accessed via
  a slow network tunnel. In practice, this is unlikely — admin socket access
  is typically local.

## References

- ADR-014 — Unix domain socket config reload API
- [operations.md](../operations.md) — Admin socket protocol
- Security Review W4 — Admin socket has no read timeout or line length limit