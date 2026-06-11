---
id: ops/admin-socket
name: Implement Unix domain socket admin API for config reload with feedback and status
status: complete
depends_on: [config/dynamic-config]
scope: moderate
risk: medium
impact: component
level: implementation
---

## Description

Implement the Unix domain socket admin API for programmatic config reload with success/failure feedback. This is an alternative to SIGHUP that provides structured responses.

### Protocol

- **Connection lifecycle**: One command per connection. Client connects, sends one newline-terminated command, receives one newline-terminated JSON response, then the server closes the connection.
- **Message framing**: Newline-delimited (`\n`). Responses end with `\n`.

### Commands

- `reload` — Re-read config file, validate, and swap DynamicConfig. Returns:
  - Success: `{"status": "ok"}`
  - Failure: `{"status": "error", "message": "..."}`
- `status` — Return basic process info. Returns:
  - `{"status": "ok", "uptime_secs": 1234, "sites": 2}`

### Error Responses

- Unrecognized commands: `{"status": "error", "message": "unknown command: <cmd>"}`
- Invalid or empty input: `{"status": "error", "message": "invalid input"}`

### Socket Lifecycle

- Socket path from `admin_socket_path` config (default: `/run/reverse-proxy/admin.sock`)
- Empty string disables the admin socket
- Remove any existing socket file at startup before binding
- If the socket file exists and another process is listening, log a warning and disable the admin socket (but continue starting)

### Concurrency

- Multiple clients can connect simultaneously
- Reload operations are serialized via the same `tokio::sync::Mutex` used by SIGHUP reload
- If a reload is in progress, subsequent reload requests wait, then re-read the config file (getting the latest version)

## Acceptance Criteria

- [ ] Unix domain socket bound at `admin_socket_path`
- [ ] `reload` command triggers config reload and returns structured JSON response
- [ ] `status` command returns process uptime and site count
- [ ] Unknown commands return `{"status": "error", "message": "unknown command: ..."}`
- [ ] Empty/invalid input returns `{"status": "error", "message": "invalid input"}`
- [ ] One command per connection, server closes connection after response
- [ ] Stale socket file removed at startup
- [ ] If socket file exists and is active (another process), log warning and continue
- [ ] `admin_socket_path = ""` disables admin socket
- [ ] Reload operations serialized with same Mutex as SIGHUP reload
- [ ] Integration test: connect to socket, send `reload`, receive JSON response
- [ ] Integration test: connect to socket, send `status`, receive JSON response

## References

- docs/architecture/operations.md — admin socket section
- docs/architecture/decisions/014-unix-socket-reload.md — admin socket rationale
- docs/architecture/config.md — reload serialization

## Notes

> The admin socket and SIGHUP converge on the same reload code path. The only difference is that the admin socket returns a structured response while SIGHUP provides no feedback.

## Summary

> To be filled on completion