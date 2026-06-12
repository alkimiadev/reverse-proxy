---
id: fix/connect-timeout
name: Wire upstream_connect_timeout_secs to enforce separate connect timeout
status: pending
depends_on: []
scope: narrow
risk: medium
impact: component
level: implementation
review_findings: [W4]
---

## Description

The proxy uses `tokio::time::timeout` with `upstream_request_timeout_secs` for the entire request, but there is no separate connect timeout. A slow DNS resolution or TCP handshake consumes the full request timeout budget, leaving no time for the actual request/response cycle.

The architecture already specifies a 5-second default connect timeout separate from the request timeout (ADR-015, ADR-017), and `SiteConfig` already includes `upstream_connect_timeout_secs` with a default of 5. The field just isn't wired up in the proxy handler.

### Changes Required

**`src/proxy/handler.rs`**:
- Read `site.upstream_connect_timeout_secs` alongside `site.upstream_request_timeout_secs`
- Implement a two-phase timeout for the upstream request:
  ```rust
  let connect_timeout = Duration::from_secs(site.upstream_connect_timeout_secs);
  let request_timeout = Duration::from_secs(site.upstream_request_timeout_secs);

  let result = tokio::time::timeout(request_timeout, async {
      // Phase 1: connect timeout
      // The hyper client handles connect internally, so we wrap the
      // entire request in the request timeout. The connect timeout
      // can be enforced by setting it on the hyper client's connect
      // configuration, or by using a separate timeout wrapper.
      client.request(upstream_req).await
  }).await;
  ```
- For Phase 1, the simplest correct approach is to set the connect timeout on the hyper client builder. However, hyper's `Client` doesn't directly expose a connect timeout per-request. The alternative is to use `tokio::time::timeout(connect_timeout, ...)` wrapping just the connection phase, but this requires restructuring to separate the connect from the request.

  **Recommended approach**: Since `SiteConfig` already has `upstream_connect_timeout_secs`, and the hyper client is shared across all sites, the cleanest Phase 1 approach is to document that the `upstream_request_timeout_secs` covers the entire exchange and note that a per-site connect timeout requires a per-site client or a two-phase timeout approach. For now, log a warning if the connect timeout differs from the request timeout, and use the request timeout as the overall timeout. A future Phase 2 task can implement true per-site connect timeouts with separate client builders.

  **Alternative approach**: Use `connect_timeout` on the `HttpConnector` or `HttpsConnector` when building clients. The `hyper_util::client::legacy::Client` builder supports `.pool_idle_timeout()` but not a direct connect timeout. However, `hyper_rustls::HttpsConnector` wraps an `HttpConnector` which does support `set_connect_timeout()`. This would require creating clients per-site or passing the timeout through at connection time.

  **Simplest correct approach for Phase 1**: Set `connect_timeout` on the `HttpConnector` used by both the HTTP and HTTPS clients. Create a helper that sets `connect_timeout` on the `HttpConnector`. This provides a global connect timeout that applies to all upstream connections. Per-site override can be a Phase 2 enhancement.

- Actually, re-reading the code: the `create_http_client` and `create_https_client` functions create shared clients. The simplest approach is to add a `connect_timeout` method to the `HttpConnector` via `set_connect_timeout()` and the `HttpsConnector` builder. This would set a default connect timeout on all upstream connections.

- For per-site connect timeout, we'd need to either:
  a. Create per-site clients (heavy approach)
  b. Use a two-phase timeout with `tokio::time::timeout` (wraps the connect phase)

- **Decision**: Wire `upstream_connect_timeout_secs` as the connect timeout on the HTTP/HTTPS connectors. This provides site-independent connect timeout enforcement. Per-site connect timeout variation requires client-per-site which is out of scope for Phase 1. The config field exists and the value is available; the implementation should at minimum use the per-site value from `SiteConfig` as the request timeout's "connect portion" when building the outer timeout.

  **Final recommended approach**: Use a two-phase timeout in `proxy_handler`:
  ```rust
  let connect_timeout = Duration::from_secs(site.upstream_connect_timeout_secs);
  let request_timeout = Duration::from_secs(site.upstream_request_timeout_secs);

  let result = tokio::time::timeout(request_timeout, async {
      let response = client.request(upstream_req).await?;
      Ok(response)
  }).await;
  ```
  
  Since we can't easily separate connect from request phases with hyper's shared client, the simplest improvement is to at minimum set `set_connect_timeout` on the `HttpConnector` used to build the clients, using the default 5s value. Then per-site `upstream_request_timeout_secs` remains the overall timeout.

## Acceptance Criteria

- [ ] `upstream_connect_timeout_secs` from `SiteConfig` is read and used in `proxy_handler`
- [ ] A connect timeout is enforced on upstream connections (default 5s)
- [ ] A request timeout remains the overall timeout (default 60s)
- [ ] Slow upstream connections time out appropriately
- [ ] All existing tests pass
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/decisions/015-per-site-timeouts.md — ADR-015
- docs/architecture/decisions/017-upstream-connection-defaults.md — ADR-017
- docs/reviews/002-implementation-review.md — W4 finding
- src/proxy/handler.rs — current timeout implementation

## Notes

> The exact implementation approach depends on what hyper's API supports for connect timeouts. If `set_connect_timeout` on `HttpConnector` works cleanly, use it. Otherwise, document the limitation and use the two-phase `tokio::time::timeout` approach.

## Summary

> To be filled on completion