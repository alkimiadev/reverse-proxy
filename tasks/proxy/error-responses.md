---
id: proxy/error-responses
name: Implement proxy error responses with plain text bodies and correct status codes
status: completed
depends_on: [proxy/host-routing]
scope: single
risk: trivial
impact: isolated
level: implementation
---

## Description

Implement the error response types for the proxy handler. All error responses use plain text bodies with no proxy version or identity information. No upstream error details are included.

### Error Response Table

| Upstream Condition | Response | Body |
|-------------------|----------|------|
| Upstream reachable | Stream response as-is | (upstream body) |
| Upstream unreachable | 502 Bad Gateway | `Bad Gateway` |
| Upstream timeout | 504 Gateway Timeout | `Gateway Timeout` |
| Request body too large | 413 Payload Too Large | `Payload Too Large` |
| Rate limit exceeded | 429 Too Many Requests | `Too Many Requests` |
| Unknown Host header | 404 Not Found | `Not Found` |
| Missing Host header | 400 Bad Request | `Bad Request` |

### Response Format

- Content-Type: `text/plain; charset=utf-8`
- Body: Brief status text matching the HTTP status
- No proxy version or identity information
- No upstream error details leaked

### Logging

- 502 and 504 responses logged at `warn` level with structured fields
- 429 responses logged at `info` level with RATE_LIMIT prefix
- 404 and 400 responses not specially logged (normal routing)

## Acceptance Criteria

- [ ] Error response type/enum covering all cases in the table
- [ ] All error responses use `text/plain; charset=utf-8` Content-Type
- [ ] Error bodies are brief status text with no version or identity info
- [ ] 502 logged at `warn` level with host and upstream
- [ ] 504 logged at `warn` level with host and upstream
- [ ] 429 logged at `info` level with RATE_LIMIT prefix
- [ ] Unit tests for each error response type

## References

- docs/architecture/proxy.md — error handling section

## Notes

> This is a small but important task — correct error responses without information leakage are a security concern. Implementation agents should not add extra detail to error bodies.

## Summary

> To be filled on completion