---
id: fix/upstream-host-validation
name: Validate host part of upstream address in config (W1)
status: completed
depends_on: []
scope: narrow
risk: low
impact: component
level: implementation
review_findings: [W1]
---

## Description

`is_valid_upstream` checks that the upstream has a `host:port` format with a
valid port number, but performs **no validation on the host part** beyond
checking it's non-empty and doesn't start with `http://` or `https://`. Values
like `!!!bad!!!:3000` or `@#$%:8080` pass validation.

The spec (config.md validation rule 17) now requires: "the host part must parse
as a valid IpAddr or pass is_valid_hostname validation." Bracket-enclosed values
must be parsed as IPv6 addresses.

### Changes Required

**`src/config/validation.rs`** — `is_valid_upstream` function (lines 309-327):
- After validating the port, validate the host part:
  ```rust
  fn is_valid_upstream(upstream: &str) -> bool {
      if let Some(idx) = upstream.rfind(':') {
          let host_part = &upstream[..idx];
          let port_str = &upstream[idx + 1..];
          if host_part.is_empty() { return false; }
          if upstream.starts_with("http://") || upstream.starts_with("https://") { return false; }
          let port: u16 = match port_str.parse() { Ok(p) => p, Err(_) => return false };
          if port == 0 { return false; }
          // Validate host part per config.md rule 17
          if host_part.starts_with('[') && host_part.ends_with(']') {
              let inner = &host_part[1..host_part.len()-1];
              inner.parse::<std::net::Ipv6Addr>().is_ok()
          } else {
              host_part.parse::<std::net::IpAddr>().is_ok() || is_valid_hostname(host_part)
          }
      } else {
          false
      }
  }
  ```
- Note: `is_valid_hostname` already exists in the same file and is used for site
  `host` validation. It rejects IP addresses, which is correct for site hosts
  but wrong for upstream hosts — upstream hosts CAN be IPs. The upstream
  validation must check `IpAddr::parse` first, then fall back to
  `is_valid_hostname` for DNS names.

- Add tests for:
  - Valid: `gitea:3000`, `127.0.0.1:3000`, `[::1]:3000`
  - Invalid: `!!!bad!!!:3000`, `@#$%:8080`, `:3000` (empty host)

## Acceptance Criteria

- [ ] `is_valid_upstream` validates the host part as IP address or valid hostname
- [ ] IPv6 bracket notation is handled (e.g., `[::1]:3000`)
- [ ] Invalid host characters like `!!!bad!!!:3000` are rejected
- [ ] Valid upstream formats still pass: `gitea:3000`, `127.0.0.1:3000`
- [ ] New unit tests for valid and invalid upstream host parts
- [ ] `cargo test` passes
- [ ] `cargo clippy` passes with no warnings

## References

- docs/architecture/config.md — validation rule 17
- docs/reviews/003-security-and-bug-review.md — W1 finding
- src/config/validation.rs — `is_valid_upstream`, `is_valid_hostname`

## Notes

> `is_valid_hostname` currently rejects IP addresses (intentional for site
> hosts). The upstream validation must handle IPs separately before falling
> back to `is_valid_hostname`.

## Summary

> To be filled on completion