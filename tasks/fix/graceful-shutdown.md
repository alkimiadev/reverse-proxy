---
id: fix/graceful-shutdown
name: Fix shutdown to drain listeners and stop background tasks cleanly
status: completed
depends_on: []
scope: moderate
risk: medium
impact: component
level: implementation
review_findings: [W1, W7]
---

## Description

Two related shutdown issues:

1. **W1**: On shutdown, `handle.abort()` is called on each HTTPS server task in `main.rs`. This immediately kills the tokio task, interrupting in-flight request processing. The `InFlightGuard` RAII type ensures `decrement` is called on drop, but `abort()` prevents normal drops. The architecture spec says tasks should be joined with a timeout, not aborted — only aborting after the shutdown timeout expires.

   The good news: `serve_https_listener` already has a `shutdown_rx` that breaks the accept loop on shutdown signal. So tasks will stop accepting new connections. We just need to wait for them to drain in-flight requests instead of aborting them.

2. **W7**: `start_admin_socket` runs an infinite `loop` accepting connections with no way to break out. It doesn't accept a shutdown signal, so it can't be gracefully stopped. Similarly, the rate limiter eviction task runs an infinite loop with no cancellation mechanism.

### Changes Required

**`src/main.rs`**:
- Replace `handle.abort()` loop with timeout-based join:
  ```rust
  let shutdown_timeout = shutdown.shutdown_timeout();
  for handle in https_server_handles {
      match tokio::time::timeout(shutdown_timeout, handle).await {
          Ok(_) => {}
          Err(_) => {
              warn!("shutdown timeout expired, aborting listener task");
              handle.abort();
          }
      }
  }
  ```
- After draining, signal cancellation to admin socket and eviction task

**`src/admin/socket.rs`**:
- Add a `shutdown_rx: tokio::sync::watch::Receiver<bool>` parameter to `start_admin_socket`
- Replace the infinite `loop { listener.accept().await }` with `tokio::select!`:
  ```rust
  tokio::select! {
      result = listener.accept() => { /* handle connection */ },
      _ = shutdown_rx.changed() => {
          info!("admin socket shutting down");
          break;
      }
  }
  ```
- Clean up the socket file on exit (remove the Unix domain socket file)
- Update callers in `main.rs` to pass the shutdown channel

**`src/rate_limit/mod.rs`**:
- Add a `shutdown_rx: tokio::sync::watch::Receiver<bool>` parameter to `start_eviction_task`
- Replace infinite loop with `tokio::select!`:
  ```rust
  tokio::select! {
      _ = interval_timer.tick() => { limiter.evict_stale(max_age); },
      _ = shutdown_rx.changed() => {
          info!("rate limiter eviction task shutting down");
          break;
      }
  }
  ```
- Update caller in `main.rs`

## Acceptance Criteria

- [ ] HTTPS server tasks are joined with a timeout, not immediately aborted
- [ ] Tasks are only aborted if the shutdown timeout expires before they finish
- [ ] Admin socket listener breaks its accept loop on shutdown signal
- [ ] Admin socket file is cleaned up on shutdown
- [ ] Rate limiter eviction task breaks its loop on shutdown signal
- [ ] ACME state machine task is cancellable (it already exits on `None` from stream, but should also respond to cancellation)
- [ ] In-flight requests are allowed to drain before forceful shutdown
- [ ] All existing tests pass
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/operations.md — shutdown sequence
- docs/reviews/002-implementation-review.md — W1, W7 findings
- src/main.rs — current shutdown sequence
- src/admin/socket.rs — current infinite loop
- src/rate_limit/mod.rs — current infinite eviction loop
- src/server.rs — InFlightCounter and drain_in_flight

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion