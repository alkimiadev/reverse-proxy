---
id: fix/upstream-uri-error-handling
name: Return 502 on upstream URI parse failure instead of dropping query string (W3)
status: completed
depends_on: []
scope: narrow
risk: low
impact: component
level: implementation
review_findings: [W3]
---

## Description

`build_upstream_uri` silently drops the query string on parse failure and
`.unwrap()`s the fallback. This corrupts requests — the upstream receives the
wrong URL with no query parameters, and neither the client nor operator is
notified. The `.unwrap()` on the fallback could also panic.

The spec (proxy.md request forwarding step 1) states: "If URI construction
fails (e.g., the resulting URI is malformed), the proxy must return 502 Bad
Gateway and log the error at warn level. The proxy must never silently drop
parts of the URI."

### Changes Required

**`src/proxy/handler.rs`**:
- Change `build_upstream_uri` to return `Result<Uri, ()>`:
  ```rust
  fn build_upstream_uri(scheme: &str, upstream: &str, original_uri: &Uri) -> Result<Uri, ()> {
      let path = original_uri.path();
      let query = original_uri
          .query()
          .map(|q| format!("?{}", q))
          .unwrap_or_default();
      let uri_string = format!("{}://{}{}{}", scheme, upstream, path, query);
      uri_string.parse::<Uri>().map_err(|e| {
          warn!(error = %e, uri = %uri_string, "failed to parse upstream URI");
      })
  }
  ```
- Update `proxy_handler` to handle the `Err` case:
  ```rust
  let upstream_uri = match build_upstream_uri(&upstream_scheme, &upstream, req.uri()) {
      Ok(uri) => uri,
      Err(()) => {
          log_upstream_error!(&host_owned, &upstream_addr, "malformed upstream URI");
          let duration_ms = start.elapsed().as_millis() as u64;
          log_request!(&client_ip, &host_owned, &method, &path, 502u16, &upstream, duration_ms);
          return StatusCode::BAD_GATEWAY.into_response();
      }
  };
  ```
- Update the existing unit tests for `build_upstream_uri` to handle the
  `Result` return type.

## Acceptance Criteria

- [ ] `build_upstream_uri` returns `Result<Uri, ()>` instead of `Uri`
- [ ] URI parse failure logs a warning with the malformed URI string
- [ ] URI parse failure returns 502 Bad Gateway to the client
- [ ] No `.unwrap()` calls in `build_upstream_uri`
- [ ] Query strings are never silently dropped
- [ ] Existing unit tests updated for `Result` return type
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/proxy.md — request forwarding step 1, URI error handling
- docs/reviews/003-security-and-bug-review.md — W3 finding
- src/proxy/handler.rs — `build_upstream_uri`, `proxy_handler`

## Notes

> To be filled on completion

## Summary

> To be filled on completion