---
status: open
last_updated: 2026-08-10
reviewed_code:
  - src/server.rs
  - src/proxy/handler.rs
reviewer: code-reviewer
based_on: docs/reviews/007-connection-lifecycle-and-deployment-drift.md
trigger: Post-deployment observation — 930+ idle connections persisting after 6 days uptime despite C1 fix
---

# Follow-Up Review #008 — HTTP/2 Keep-Alive Defeats Idle Timeout

## Purpose

Review #007 C1 identified that the reverse-proxy had no server-side idle
timeout on TLS connections, causing FD exhaustion. The fix (commit `0885486`)
added `keep_alive_interval(15s)` + `keep_alive_timeout(60s)` on the HTTP/2
builders and `header_read_timeout(60s)` on the HTTP/1.1 builder. After
deploying and running for 6 days, the proxy has **930+ established
connections** — some over 145 hours old — that are never closed by the idle
timeout. This review examines why the C1 fix is not working as intended and
what needs to change.

---

## Finding: HTTP/2 keep-alive pings defeat the idle timeout [new code required]

**Location**: `src/server.rs:119-141`

### What was implemented (C1 fix)

```rust
// HTTP/2 path (line 119-129)
let mut builder = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
builder
    .timer(hyper_util::rt::TokioTimer::new())
    .keep_alive_interval(Some(Duration::from_secs(15)))      // sends PING every 15s
    .keep_alive_timeout(connection_idle_timeout)              // 60s
    .enable_connect_protocol();

// HTTP/1.1 + auto path (line 130-141)
let mut builder = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
builder
    .http1()
    .timer(hyper_util::rt::TokioTimer::new())
    .header_read_timeout(Some(connection_idle_timeout));     // 60s, first-request only
builder
    .http2()
    .timer(hyper_util::rt::TokioTimer::new())
    .keep_alive_interval(Some(Duration::from_secs(15)))
    .keep_alive_timeout(connection_idle_timeout)
    .enable_connect_protocol();
```

### Why it doesn't work

**HTTP/2 path**: `keep_alive_interval(15s)` sends a PING frame every 15
seconds. `keep_alive_timeout(60s)` is the timeout for receiving a PONG
*response* to a PING. If the client responds to the PING (which any
well-behaved HTTP/2 client does), the timeout is reset. The PING/PONG cycle
keeps the connection "alive" indefinitely — the timeout only fires if the
client *stops responding* to pings (e.g. network failure, crashed client).
A crawler that opens an HTTP/2 connection, makes one request, then sits idle
but still responds to PINGs will keep the connection open forever.

This is confirmed by production data: 930+ established connections, some 145
hours old (since proxy start 6 days ago), all with `timer: 0` in `ss` output
(no active timer). The `keep_alive_timeout` is never firing because clients
respond to pings.

**HTTP/1.1 path**: `header_read_timeout(60s)` only applies to the time
waiting for the *first* request headers after the TCP/TLS handshake. Once
the first request is received, there is no idle timeout between subsequent
requests on the same keep-alive connection. A client that sends one request
every 55 seconds keeps the connection alive indefinitely.

### What hyper actually needs

hyper does not have a built-in "idle connection timeout" that closes
connections after N seconds of no *real* (non-ping) activity. The available
options are:

| Setting | What it does | What we need |
|---------|-------------|--------------|
| `keep_alive_interval` | Sends PINGs to detect dead peers | Useful, but not an idle timeout |
| `keep_alive_timeout` | Timeout for PONG response to a PING | Only fires if client stops responding |
| `header_read_timeout` | Timeout for first request headers | Only covers first request, not idle between requests |

None of these provide: "close this connection if no real HTTP request has
been received in the last N seconds."

### Recommendation

**Option A (preferred): Wrap `serve_connection` with a tokio timeout
that resets on each request.** Implement a custom idle timeout that tracks
the last real request time and closes the connection if no request arrives
within `connection_idle_timeout`. This can be done by wrapping the service
to record a timestamp on each request, and racing `serve_connection` against
a sleep timer that resets on each request:

```rust
// Pseudocode for the connection handler:
let last_activity = Arc::new(AtomicU64::new(now()));
let svc = IdleTrackingService::new(svc, last_activity.clone());

tokio::select! {
    result = builder.serve_connection(io, svc) => {
        // connection completed normally
    }
    _ = idle_timeout_watcher(last_activity.clone(), connection_idle_timeout) => {
        // no real request for connection_idle_timeout — close the connection
        // (dropping the io handle closes the TLS stream)
    }
}

async fn idle_timeout_watcher(
    last_activity: Arc<AtomicU64>,
    timeout: Duration,
) {
    loop {
        let elapsed = now() - last_activity.load();
        let remaining = timeout.saturating_sub(elapsed);
        if remaining.is_zero() {
            return; // idle timeout fired
        }
        tokio::time::sleep(remaining).await;
    }
}
```

The `IdleTrackingService` wraps the inner service and updates
`last_activity` on each `call()`. This gives a true idle timeout that fires
regardless of PING/PONG activity.

**Option B: Remove `keep_alive_interval` and rely solely on the
`select!`-based timeout.** The keep-alive PING mechanism is useful for
detecting dead peers, but it's not a substitute for an idle timeout. If
both are needed, keep the PING for liveness detection but add the
`select!`-based idle timeout on top.

**Option C: For HTTP/1.1, also set `keep_alive_timeout` on the
`http1()` builder.** hyper's `http1::Builder` has a `keep_alive_timeout`
method (distinct from `header_read_timeout`) that sets the timeout for
reading the next request on a keep-alive connection. This would close idle
HTTP/1.1 connections after the timeout. However, this doesn't help with
HTTP/2, which is where most of the idle connections are (crawlers default to
HTTP/2 via ALPN).

### HTTP/1.1 fix

For the HTTP/1.1 path (`auto::Builder`), add
`.http1().keep_alive_timeout(Some(connection_idle_timeout))` in addition to
`header_read_timeout`. This covers idle time between requests on keep-alive
HTTP/1.1 connections.

### Verification after fix

After implementing the true idle timeout, verify with:
1. Open an HTTP/2 connection to the proxy, send one request, then idle
2. Confirm the connection closes after `connection_idle_timeout` (60s)
3. Check FD count drops to baseline (~25) when traffic stops
4. Confirm active connections (making requests every <60s) are not closed

### Production impact

Without this fix, the proxy accumulates idle HTTP/2 connections from every
crawler/search-engine that connects. After 6 days of uptime with the C1 fix
deployed, the proxy has 930+ idle connections (some 145 hours old) from
~625 unique IPs. The `max_connections = 1024` semaphore prevents FD
exhaustion, but the proxy is near the cap, and legitimate new connections
will start being blocked if the count reaches 1024.

The connections don't consume CPU or I/O when truly idle, but they hold FDs
and TLS state (~60KB each in memory). At 930 connections, that's ~56MB of
TLS state for connections that should have been closed hours or days ago.

---

## Summary

| Issue | Status | Action |
|-------|--------|--------|
| C1 fix (keep_alive_timeout) | Deployed but ineffective | Replace with true idle timeout (Option A) |
| HTTP/1.1 header_read_timeout | Deployed but only covers first request | Add `keep_alive_timeout` for between-request idle |
| HTTP/2 keep_alive_interval | Deployed, keeps connections alive | Remove or keep alongside true idle timeout |

## References

- [Review #007](007-connection-lifecycle-and-deployment-drift.md) — original C1 finding and fix
- [hyper http2::Builder docs](https://docs.rs/hyper/latest/hyper/server/conn/http2/struct.Builder.html) — `keep_alive_interval`, `keep_alive_timeout`
- [hyper-util auto::Builder docs](https://docs.rs/hyper-util/latest/hyper_util/server/conn/auto/struct.Builder.html) — `http1().keep_alive_timeout()`
- Production observation: 930+ idle connections, 145h old, dev1 2026-08-10