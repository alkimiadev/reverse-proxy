---
id: fix/admin-socket-resource-limits
name: Add read timeout and line length limit to admin socket (ADR-027)
status: completed
depends_on: []
scope: narrow
risk: low
impact: component
level: implementation
review_findings: [W4, S4]
---

## Description

The admin socket's `handle_connection` reads one newline-terminated line with
`reader.read_line(&mut line)` but sets no timeout and no length limit. This
allows:
1. A client to connect and send no data, holding a connection indefinitely
2. A client to send unbounded data without a newline, causing OOM

ADR-027 specifies: 5-second read timeout, 4096 byte line length limit.

### Changes Required

**`src/admin/socket.rs`** — `handle_connection` function (lines 166-210):
- Wrap the `BufReader` with `tokio::io::take` to limit read size to 4096 bytes:
  ```rust
  let (reader, mut writer) = stream.into_split();
  let mut reader = BufReader::new(tokio::io::take(reader, 4096));
  let mut line = String::new();
  ```
- Wrap the `read_line` call in a `tokio::time::timeout`:
  ```rust
  use std::time::Duration;
  let read_result = tokio::time::timeout(
      Duration::from_secs(5),
      reader.read_line(&mut line),
  ).await;
  ```
- Handle timeout and line-too-long cases:
  ```rust
  match read_result {
      Ok(Ok(0)) | Ok(Err(_)) => {
          // existing "invalid input" handling
      }
      Err(_) => {
          // timeout
          tracing::debug!("admin socket connection timed out");
          let _ = writer.write_all(
              format!("{}\n", serde_json::to_string(&ErrorResponse {
                  status: "error",
                  message: "read timeout".to_string(),
              }).unwrap()).as_bytes()
          ).await;
          return;
      }
      Ok(Ok(n)) => {
          // Check if line was truncated (no newline found within limit)
          if !line.ends_with('\n') && n > 0 {
              tracing::warn!("admin socket command exceeded 4096 byte limit");
              let _ = writer.write_all(
                  format!("{}\n", serde_json::to_string(&ErrorResponse {
                      status: "error",
                      message: "command too long".to_string(),
                  }).unwrap()).as_bytes()
              ).await;
              return;
          }
          // ... existing command handling
      }
  }
  ```
- Update existing tests and add new tests for timeout and line length limit.

## Acceptance Criteria

- [ ] Read timeout of 5 seconds applied to admin socket connections
- [ ] Line length limit of 4096 bytes applied (via `tokio::io::take`)
- [ ] Timeout logged at `debug` level per ADR-027
- [ ] Line-too-long logged at `warn` level per ADR-027
- [ ] Both conditions return appropriate error JSON to the client
- [ ] Legitimate commands (`reload`, `status`) still work
- [ ] New tests for timeout and line length limit behavior
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/decisions/027-admin-socket-resource-limits.md — ADR-027
- docs/architecture/operations.md — admin socket resource limits
- docs/reviews/003-security-and-bug-review.md — W4, S4 findings
- src/admin/socket.rs — `handle_connection`

## Notes

> To be filled on completion

## Summary

> To be filled on completion