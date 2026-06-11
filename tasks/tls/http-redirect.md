---
id: tls/http-redirect
name: Implement HTTP to HTTPS redirect listener with Host-based URL construction
status: complete
depends_on: [config/static-config, config/dynamic-config]
scope: narrow
risk: low
impact: component
level: implementation
---

## Description

Implement the HTTP → HTTPS redirect listener. Each listener that has `http_port > 0` runs a plain HTTP listener that redirects all requests to the HTTPS equivalent URL.

### Redirect Behavior

1. Read the `Host` header from the incoming request
2. If no `Host` header, return `400 Bad Request`
3. Construct redirect URL: `https://{host}:{https_port}/{path}?{query}`
   - `{host}` is the hostname portion of the `Host` header (port stripped)
   - `{https_port}` is the listener's `https_port`, omitted if 443
   - `{path}` and `{query}` preserved from original request
4. Return `301 Permanent Redirect` with `Location` header

### Per-Listener

Each listener has its own HTTP redirect on its own bind address and `http_port`. Multiple listeners on different IPs can each have their own redirect.

### ACME HTTP-01 Challenge Support

When a listener is in ACME mode and uses HTTP-01 challenges, the redirect listener must also serve `/.well-known/acme-challenge/{token}` paths. This is a fallback for environments where TLS-ALPN-01 is not suitable.

Note: TLS-ALPN-01 is the default and primary challenge type. HTTP-01 support should be implemented but is not the primary path.

## Acceptance Criteria

- [ ] HTTP listener binds to `bind_addr:http_port` for each enabled listener
- [ ] Redirect to `https://{host}:{https_port}/{path}?{query}` with 301 status
- [ ] Port 443 is omitted from redirect URL (standard HTTPS port)
- [ ] Non-443 HTTPS ports are included in redirect URL
- [ ] Missing `Host` header returns `400 Bad Request`
- [ ] Per-listener redirect: each listener has its own HTTP redirect
- [ ] `http_port = 0` disables HTTP redirect for that listener
- [ ] ACME HTTP-01 challenge path handling (placeholder for future integration)
- [ ] Unit tests for redirect URL construction
- [ ] Integration test: HTTP request redirects to correct HTTPS URL

## References

- docs/architecture/proxy.md — HTTP → HTTPS redirect section
- docs/architecture/tls.md — ACME challenge types, HTTP listener

## Notes

> To be filled by implementation agent

## Summary

> To be filled on completion