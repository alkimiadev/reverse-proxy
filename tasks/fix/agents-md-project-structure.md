---
id: fix/agents-md-project-structure
name: Update AGENTS.md project structure and common modifications after admin refactor
status: completed
depends_on: [fix/admin-http-api]
scope: narrow
risk: low
impact: isolated
level: review
review_findings: []
adr: [028]
---

## Description

After the admin socket → HTTP API migration, `AGENTS.md` needs updates to
reflect the new project structure, config format, and operational procedures.

### Changes Required

**Project Structure section** — Update to reflect new admin module layout:
```
src/
├── admin/
│   ├── auth.rs         # Bearer token auth middleware (subtle, SHA-256)
│   ├── handler.rs      # HTTP handlers for /admin/reload, /status, /rotate-key
│   └── mod.rs          # Re-exports
```
Remove:
```
│   ├── socket.rs       # REMOVED — was Unix domain socket admin API
```

**Key Architecture Concepts section** — Update the admin socket description:
- Replace "Unix domain socket (`admin_socket_path`)" with "Authenticated HTTP
  admin API (`admin_key_path`) on health check port"
- Note that admin endpoints require Bearer token auth
- Note that `admin_key_path` empty string = disabled (returns 404)

**Config Format section** — Update:
- Replace `admin_socket_path` references with `admin_key_path`
- Note that `admin_key_path` default is `/etc/reverse-proxy/admin-key`
- Add key file format info (plaintext, one line, read once at startup)

**Common Modifications section** — Replace:
```bash
# Before (Unix socket)
echo "reload" | socat - UNIX-CONNECT:/run/reverse-proxy/admin.sock

# After (HTTP with Bearer token)
curl -H "Authorization: Bearer $ADMIN_KEY" http://127.0.0.1:9900/admin/reload
curl -H "Authorization: Bearer $ADMIN_KEY" http://127.0.0.1:9900/admin/status
```

**Build & Run section** — No changes needed (build commands unchanged).

**Testing section** — Note that admin tests now use HTTP (reqwest) instead of
Unix socket (tokio::net::UnixStream).

## Acceptance Criteria

- [ ] Project structure shows `auth.rs` and `handler.rs`, not `socket.rs`
- [ ] Key architecture concepts mention `admin_key_path` and Bearer token auth
- [ ] Config format section mentions `admin_key_path`
- [ ] Common modifications section uses `curl` examples, not `socat`
- [ ] No references to `admin_socket_path` remain in AGENTS.md

## References

- AGENTS.md — current project structure and common modifications
- docs/architecture/decisions/028-admin-http-api.md — ADR-028

## Notes

> Depends on `fix/admin-http-api` being complete so the new file names are
> accurate.

## Summary

Updated project structure (admin/auth.rs, handler.rs, mod.rs; config/mod.rs),
architecture concepts (admin HTTP API, TOCTOU check, wildcard bind consistency),
config format (admin_key_path), testing (HTTP-based admin tests), and common
modifications (curl commands replacing socat). Updated README.md with same
changes: admin HTTP API section, project structure, architecture diagram, config
table, and Docker compose volumes.