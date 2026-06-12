---
status: draft
last_updated: 2026-06-12
reviewed_code:
  - src/main.rs
  - src/server.rs
  - src/cli.rs
  - src/lib.rs
  - src/shutdown.rs
  - src/health.rs
  - src/utils.rs
  - src/config/static_config.rs
  - src/config/dynamic_config.rs
  - src/config/validation.rs
  - src/config/test_fixtures.rs
  - src/config/mod.rs
  - src/proxy/handler.rs
  - src/proxy/headers.rs
  - src/proxy/body_limit.rs
  - src/proxy/error.rs
  - src/proxy/mod.rs
  - src/rate_limit/mod.rs
  - src/rate_limit/bucket.rs
  - src/admin/socket.rs
  - src/admin/mod.rs
  - src/logging/mod.rs
  - src/logging/format.rs
  - src/tls/acceptor.rs
  - src/tls/acme.rs
  - src/tls/config.rs
  - src/tls/redirect.rs
  - src/tls/mod.rs
  - tests/integration_test.rs
  - tests/helpers/http_test_helper.rs
  - tests/helpers/tls_test_helper.rs
  - tests/helpers/mod.rs
reviewer: code-reviewer
---

# Security & Bug Review #003

## Purpose

Third review of the codebase, focused on security vulnerabilities, logic bugs,
and correctness issues that survived the first two reviews. Also flags dead code,
code smells, and test gaps.

## Severity Definitions

| Severity | Meaning |
|----------|---------|
| **Critical** | Will cause incorrect behavior or security issues in production |
| **Warning** | Could cause issues under specific conditions or represents a missed edge case |
| **Suggestion** | Code quality, style, or minor improvement opportunity |

---

## Critical Findings

### C1. Rate Limiter Uses Client-Supplied X-Forwarded-For for IP Identification

**File**: `src/rate_limit/mod.rs:66-76`

**Problem**: The `rate_limit_middleware` extracts the client IP by checking the
`X-Forwarded-For` header **first**, then falling back to `ConnectInfo`:

```rust
let client_ip = req
    .headers()
    .get("x-forwarded-for")   // <-- checked FIRST
    .and_then(|v| v.to_str().ok())
    .and_then(|v| v.split(',').next())
    .and_then(|v| v.trim().parse::<IpAddr>().ok())
    .or_else(|| {
        req.extensions()
            .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|ci| ci.ip())
    });
```

The middleware runs **before** the proxy handler. At that point,
`inject_proxy_headers` has not yet run, so `X-Forwarded-For` is whatever the
**client** sent — completely untrusted. This creates two attack vectors:

1. **Rate limit bypass**: Attacker sends each request with
   `X-Forwarded-For: <random IP>`. Every request appears to come from a
   different IP, evading the per-IP token bucket entirely.
2. **Denial-of-service via IP spoofing**: Attacker sends requests with
   `X-Forwarded-For: <victim IP>`. The victim's IP gets rate-limited and their
   legitimate requests receive 429s.

The proxy is the **edge** — it terminates TLS directly from the internet. The
only trustworthy source of client IP is `ConnectInfo<SocketAddr>`, which
`ConnectInfoService` sets from the TCP peer address before TLS handshake
(`src/server.rs:171-173`).

**Solution**: Swap the priority: check `ConnectInfo` first, and only fall back
to `X-Forwarded-For` if `ConnectInfo` is absent (which shouldn't happen in the
current deployment). The `X-Forwarded-For` header from the client is untrusted
at the edge.

```rust
let client_ip = req
    .extensions()
    .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
    .map(|ci| ci.ip())
    .or_else(|| {
        req.headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse::<IpAddr>().ok())
    });
```

---

### C2. InFlightCounter Never Increments — Drain Logic Is Completely Broken

**File**: `src/server.rs:17-46,73-74`

**Problem**: `InFlightCounter` has an `increment()` method that is **never
called** anywhere in the codebase. The `InFlightGuard` only calls `decrement()`
on drop:

```rust
struct InFlightGuard(Arc<InFlightCounter>);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.decrement();  // only decrements!
    }
}
```

The guard is created in `serve_https_listener`:

```rust
tokio::spawn(async move {
    let _guard = InFlightGuard(in_flight.clone());
    // ...
});
```

Since `increment()` is never called, `count` stays at 0. When the first guard
drops, `fetch_sub(1)` on an `AtomicUsize` with value 0 wraps to `usize::MAX`.
`is_zero()` checks `count == 0`, which will never be true again. The drain loop
in `main.rs:250` will always time out and report `usize::MAX` remaining
connections.

**Solution**: Call `in_flight.increment()` before spawning the connection task:

```rust
in_flight.increment();
tokio::spawn(async move {
    let _guard = InFlightGuard(in_flight.clone());
    // ...
});
```

Or fold the increment into `InFlightGuard::new()`:

```rust
impl InFlightGuard {
    fn new(counter: Arc<InFlightCounter>) -> Self {
        counter.increment();
        Self(counter)
    }
}
```

---

### C3. Per-Site `upstream_connect_timeout_secs` Silently Capped at 5 Seconds

**File**: `src/proxy/handler.rs:86-98,218-224,226-229`

**Problem**: The proxy handler applies a per-site connect timeout via
`tokio::time::timeout`:

```rust
let connect_timeout = Duration::from_secs(site.upstream_connect_timeout_secs);
// ...
tokio::time::timeout(connect_timeout, state.http_client.request(upstream_req)).await
```

But the HTTP connector inside the client has its own hardcoded connect timeout:

```rust
pub fn create_http_client() -> Client<HttpConnector, Body> {
    let mut connector = HttpConnector::new();
    connector.set_connect_timeout(Some(Duration::from_secs(5)));  // hardcoded
    // ...
}
```

The connector's internal timeout takes precedence — if
`upstream_connect_timeout_secs` is set to 10s, the connector still times out at
5s. The `tokio::time::timeout` wrapper can only make the effective timeout
**shorter**, not longer. Per-site connect timeout values > 5s are silently
ignored.

**Solution**: Set the connector's connect timeout to match (or exceed) the
per-site value, or remove the hardcoded connector timeout and rely solely on
the `tokio::time::timeout` wrapper. Since the client is shared across all sites,
the simplest fix is to set the connector timeout to a high value (e.g., 30s)
and let the per-site `tokio::time::timeout` enforce the actual limit:

```rust
connector.set_connect_timeout(Some(Duration::from_secs(30)));
```

Or make the connector timeout configurable per-site by creating clients lazily.

---

### C4. `init_json` Without Log File Doesn't Enable JSON Format

**File**: `src/logging/mod.rs:54-59`

**Problem**: When `format = "json"` is configured but no `log_file_path` is set,
the `None` branch of `init_json` creates a layer **without** calling `.json()`:

```rust
None => {
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_filter(env_filter);
    tracing_subscriber::registry().with(layer).try_init()?;
}
```

The output is plain text, not JSON. The `format = "json"` config value is
silently ignored. The `Some(path)` branch correctly calls `.json()` on both
layers.

**Solution**: Add `.json()` to the `None` branch:

```rust
None => {
    let layer = tracing_subscriber::fmt::layer()
        .json()
        .with_ansi(false)
        .with_filter(env_filter);
    tracing_subscriber::registry().with(layer).try_init()?;
}
```

---

## Warning Findings

### W1. `is_valid_upstream` Doesn't Validate the Host Part

**File**: `src/config/validation.rs:309-327`

**Problem**: `is_valid_upstream` checks that the upstream has a `host:port`
format with a valid port number, but performs **no validation on the host
part** beyond checking it's non-empty and doesn't start with `http://` or
`https://`. Values like `!!!bad!!!:3000` or `@#$%:8080` pass validation.

**Solution**: Validate the host part is a valid DNS name or IP address. Reuse
`is_valid_hostname` for DNS names and add an IP address parse check for the
host portion (handling IPv6 bracket notation):

```rust
fn is_valid_upstream(upstream: &str) -> bool {
    if let Some(idx) = upstream.rfind(':') {
        let host_part = &upstream[..idx];
        let port_str = &upstream[idx + 1..];
        if host_part.is_empty() { return false; }
        if upstream.starts_with("http://") || upstream.starts_with("https://") { return false; }
        let port: u16 = match port_str.parse() { Ok(p) => p, Err(_) => return false };
        if port == 0 { return false; }
        // Validate host part
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

---

### W2. ACME Contact Validation Only Checks `mailto:` Prefix

**File**: `src/config/validation.rs:149`

**Problem**: The validation checks `contact.starts_with("mailto:")` but doesn't
verify there's an actual email address after the prefix. `acme_contact =
"mailto:"` (empty email) passes validation but will fail at the Let's Encrypt
API with a 400-level error at certificate provisioning time — after the proxy
has already started.

**Solution**: Validate that the string after `mailto:` is non-empty and contains
an `@` sign:

```rust
if contact.is_empty() || !contact.starts_with("mailto:") {
    errors.push(ValidationError::AcmeContactInvalid { ... });
} else {
    let email = &contact[7..]; // after "mailto:"
    if email.is_empty() || !email.contains('@') {
        errors.push(ValidationError::AcmeContactInvalid { ... });
    }
}
```

---

### W3. `build_upstream_uri` Silently Drops Query String on Parse Failure

**File**: `src/proxy/handler.rs:190-202`

**Problem**: If the full upstream URI (with query string) fails to parse, the
fallback drops the query string entirely and `.unwrap()`s the result:

```rust
uri_string.parse::<Uri>().unwrap_or_else(|_| {
    format!("{}://{}{}", scheme, upstream, path)  // query dropped!
        .parse::<Uri>()
        .unwrap()  // panics if this also fails
})
```

This silently corrupts requests — the upstream receives the wrong URL with no
query parameters, and neither the client nor operator is notified. The
`.unwrap()` on the fallback parse could also panic (though unlikely since
`scheme://host:port/path` should always parse).

**Solution**: Log a warning and return a 502 Bad Gateway instead of silently
dropping data:

```rust
fn build_upstream_uri(scheme: &str, upstream: &str, original_uri: &Uri) -> Result<Uri, ()> {
    let path = original_uri.path();
    let query = original_uri.query().map(|q| format!("?{}", q)).unwrap_or_default();
    let uri_string = format!("{}://{}{}{}", scheme, upstream, path, query);
    uri_string.parse::<Uri>().map_err(|e| {
        warn!(error = %e, uri = %uri_string, "failed to parse upstream URI");
    })
}
```

---

### W4. Admin Socket Has No Read Timeout or Line Length Limit

**File**: `src/admin/socket.rs:166-210`

**Problem**: `handle_connection` reads one newline-terminated line with
`reader.read_line(&mut line)` but sets no timeout and no length limit. A
malicious client with access to the admin socket (Unix permissions permitting)
can:

- Connect and send no data, holding a connection and task open indefinitely
- Send gigabytes of data without a newline, causing unbounded memory allocation

**Solution**: Wrap the read in `tokio::time::timeout` and use
`read_until` with a reasonable limit, or use `take` to cap the stream:

```rust
let mut reader = BufReader::new(tokio::io::take(reader, 4096)); // 4KB limit
let read_result = tokio::time::timeout(
    Duration::from_secs(5),
    reader.read_line(&mut line),
).await;
```

---

### W5. `TlsMode` Wildcard Match Could Cause Listener/Acceptor Mismatch

**File**: `src/main.rs:170-194,209-210`

**Problem**: The `match tls_mode` has a wildcard `_` arm that logs a warning
and pushes **no** acceptor. Then `bound_listeners.into_iter().zip(tls_acceptors.into_iter())`
uses `zip`, which silently stops at the shorter iterator. If the wildcard arm
were ever reached, some listeners would have no TLS acceptor and would be
silently dropped with no error.

`setup_tls` already rejects unknown modes with `bail!`, so the wildcard is
unreachable in practice. But it's a latent bug waiting for a future refactor.

**Solution**: Either remove the wildcard arm (since `TlsMode` only has two
variants and `setup_tls` already validates), or make the mismatch explicit:

```rust
if bound_listeners.len() != tls_acceptors.len() {
    anyhow::bail!("listener/acceptor count mismatch");
}
```

---

### W6. `RawConfig` and `FullConfig` Are Duplicated

**Files**: `src/cli.rs:49-65`, `src/config/mod.rs:15-31`

**Problem**: `RawConfig` (used at startup in `cli.rs`) and `FullConfig` (used
at reload in `config/mod.rs`) have identical fields and identical serde
attributes. They exist because the initial load path manually constructs
`StaticConfig` + `SerializableDynamicConfig`, while the reload path uses
`FullConfig::into_static_and_dynamic()`. This duplication means any new config
field must be added in two places.

**Solution**: Delete `RawConfig` and use `FullConfig` in `load_config`:

```rust
let full: FullConfig = toml::from_str(&config_content)?;
let (static_config, dynamic_config) = full.into_static_and_dynamic();
```

Then `collect_sites` can also be removed since `into_static_and_dynamic`
already collects sites from all listeners.

---

### W7. `cleanup_stale_socket` Connects to Verify Liveness — Side Effect on Other Process

**File**: `src/admin/socket.rs:142-160,162-164`

**Problem**: `is_socket_active` checks whether another process owns the socket
by **connecting to it**:

```rust
async fn is_socket_active(path: &str) -> bool {
    tokio::net::UnixStream::connect(path).await.is_ok()
}
```

If another process is listening, this creates a real connection that the other
process will `accept()`, read nothing, and close. This is a harmless but
unnecessary side effect on the other process.

**Solution**: Check for liveness by inspecting `/proc/net/unix` or by attempting
a zero-byte `sendmsg` with `MSG_PEEK`. For Phase 1, the current approach is
acceptable since the admin socket is a local diagnostic interface.

---

### W8. `test_health_check_disabled_when_port_zero` Test Name Is Misleading

**File**: `tests/integration_test.rs:82-88`

**Problem**: The test is named `test_health_check_disabled_when_port_zero` but
port 0 means "OS picks a random port" — the health check listener still starts
and binds successfully. The actual "disabled" logic is in `main.rs:94` where
`health_check_port > 0` gates the call to `start_health_check_listener`. The
test verifies that `start_health_check_listener(0)` works (binds to a random
port), which is correct but the name implies it tests the disable path.

**Solution**: Rename to `test_health_check_binds_random_port_when_zero` or add
a separate test that verifies the `main.rs` gating logic.

---

### W9. `test_dynamic_config_with_limit` Has Empty `routing_table`

**File**: `tests/integration_test.rs:618-635`

**Problem**: The helper constructs `DynamicConfig` directly with
`routing_table: Default::default()` (empty HashMap), bypassing
`DynamicConfig::from_sites()` which would normally build the routing table.
The body limit tests only read `body.limit_bytes` so the empty table doesn't
matter, but it's a latent trap for anyone who copies this pattern for tests
that need routing.

**Solution**: Use `DynamicConfig::from_sites()` to construct the config, or at
minimum add a comment warning that the routing table is intentionally empty.

---

### W10. `TokenBucket` Fields Are `pub` Despite Internal-Only Use

**File**: `src/rate_limit/bucket.rs:4-9`

**Problem**: All `TokenBucket` fields are `pub`:

```rust
pub struct TokenBucket {
    pub tokens: f64,
    pub last_refill: Instant,
    pub rate: f64,
    pub max: u32,
    pub last_access: Instant,
}
```

Only `last_access` is read externally (by `evict_stale`). The other fields
should be private to prevent accidental direct mutation that bypasses
`try_consume`/`refill` logic.

**Solution**: Make `tokens`, `last_refill`, `rate`, and `max` private. Keep
`last_access` pub(crate) for `evict_stale`.

---

### W11. Admin Socket Exposes `reload_mutex` for Testing

**File**: `src/admin/socket.rs:71-73`

**Problem**: `AdminSocket::reload_mutex()` is a public method that exists solely
for the `test_reload_serialized_with_mutex` test. It exposes an internal
synchronization primitive, and the test acquires the mutex before sending a
reload command to verify serialization — coupling the test to implementation
details.

**Solution**: Remove the method and test serialization through observable
behavior (e.g., send two rapid reload requests and verify the final config
state reflects the last one), or make the method `#[cfg(test)]`-gated.

---

### W12. `http_port` Type Is `u32` While `https_port` Is `u16`

**File**: `src/config/static_config.rs:34,36`

**Problem**: `http_port` is declared as `u32` but `https_port` is `u16`. Both
represent TCP port numbers (valid range 1–65535). The type inconsistency means
comparisons require casting (`listener.http_port == listener.https_port as u32`
at `validation.rs:133`) and `http_port` could theoretically hold values >
65535 that are caught by validation rather than the type system.

**Solution**: Change `http_port` to `u16` for consistency. Port 0 (disabled)
fits in `u16`. Update all `as u32` casts in validation and `main.rs`.

---

## Suggestions

### S1. Remove Dead Code Before Release

**Files**: Multiple

The following items are defined but never called in production code:

| Item | File | Note |
|------|------|------|
| `log_rate_limit!` macro | `src/logging/format.rs:72-83` | Rate limiter uses `warn!` directly |
| `log_config_reload!` macro | `src/logging/format.rs:97-106` | Reload uses `info!`/`warn!` directly |
| `format_event_fields()` | `src/logging/format.rs:50-54` | Never called |
| `ProxyError::BadGateway` (struct variant) | `src/proxy/error.rs:8` | Code uses `UpstreamConnection` instead |
| `ProxyError::GatewayTimeout` (struct variant) | `src/proxy/error.rs:10` | Code uses `UpstreamTimeout` instead |
| `ProxyError::PayloadTooLarge` | `src/proxy/error.rs:12` | Body limit returns tuple directly |
| `ProxyError::NotFound` | `src/proxy/error.rs:20` | Code uses `UnknownHost` instead |
| `ProxyError::BadRequest` | `src/proxy/error.rs:22` | Code uses `MissingHost` instead |
| `ProxyError::UpstreamTls` | `src/proxy/error.rs:28` | Never constructed |
| `build_multi_domain_server_config()` | `src/tls/config.rs:78-102` | Never called |
| `SniCertResolver` | `src/tls/config.rs:104-126` | Only used by above |
| `AcmeTlsConfig::directory_url()` | `src/tls/acme.rs:53-59` | Only used in tests |
| `TestUpstream::url()` | `tests/helpers/http_test_helper.rs:39-41` | `#[allow(dead_code)]` |
| `TestUpstream::upstream_addr()` | `tests/helpers/http_test_helper.rs:43-46` | `#[allow(dead_code)]` |

Either wire these up or remove them. Dead code increases audit surface and
creates confusion about what's actually active.

---

### S2. Make Eviction Interval and Max Age Configurable

**File**: `src/main.rs:196-201`

**Problem**: The eviction task interval (60s) and max age (300s) are hardcoded.
In high-traffic deployments, a shorter interval or longer max age might be
desirable.

**Suggestion**: Add `rate_limit.eviction_interval_secs` and
`rate_limit.bucket_max_age_secs` to `RateLimitConfig` with defaults of 60 and
300. These are dynamic config fields so they can be hot-reloaded.

---

### S3. Log Root Certificate Count at Startup

**File**: `src/proxy/handler.rs:246-258`

**Problem**: `root_certs()` loads native certificates silently (only logs
errors). If the system has zero root certificates (misconfigured CA bundle),
all HTTPS upstream connections will fail with opaque TLS errors.

**Suggestion**: Log the number of loaded certificates at info level:

```rust
let cert_count = result.certs.len();
info!(certs_loaded = cert_count, errors = result.errors.len(), "loaded system root certificates");
if cert_count == 0 {
    warn!("no system root certificates loaded — HTTPS upstream connections will fail");
}
```

---

### S4. Add Read Timeout and Line Length Limit to Admin Socket

**File**: `src/admin/socket.rs:166-210`

See W4 for the problem. Even if the admin socket is only accessible via Unix
permissions, defense-in-depth warrants basic resource limits.

---

### S5. Consolidate `RawConfig` and `FullConfig`

**Files**: `src/cli.rs:49-65`, `src/config/mod.rs:15-31`

See W6. Using a single `FullConfig` type for both startup and reload eliminates
duplication and ensures both paths stay in sync.

---

### S6. Add `#[non_exhaustive]` to `TokenBucket` (or Make Fields Private)

**File**: `src/rate_limit/bucket.rs:4-9`

See W10. Making fields private prevents accidental direct mutation. If external
crates need to construct `TokenBucket`, add a `new()` constructor (which
already exists).

---

### S7. Add More Status Info to Admin Socket `status` Command

**File**: `src/admin/socket.rs:265-275`

**Suggestion**: The `status` response currently returns `uptime_secs` and
`sites`. Consider adding:
- `rate_limit` (requests_per_second, burst)
- `body_limit_bytes`
- `listeners` count
- `in_flight_requests` (if InFlightCounter is fixed per C2)

This gives operators a quick health snapshot without reading logs.

---

### S8. Consider Making `upstream_connect_timeout_secs` Actually Work

**File**: `src/proxy/handler.rs:216-229`

See C3. The hardcoded 5s connector timeout caps the per-site connect timeout.
Either remove the connector timeout or set it to a high ceiling value.

---

### S9. `acme_contact` Config Field Should Be `Vec<String>` for Multiple Contacts

**File**: `src/config/static_config.rs:60`

**Problem**: `acme_contact` is a single `String` but ACME supports multiple
contact emails. The `AcmeTlsConfig.contact` field is already `Vec<String>` and
the config value is wrapped in `vec![...]` at `src/tls/acceptor.rs:70`.

**Suggestion**: Change `acme_contact` to `Vec<String>` in `TlsConfig` for
consistency with the ACME protocol. This is a config format change — document
the migration.

---

### S10. Add Integration Test for Rate Limiter Using `ConnectInfo`

**File**: `tests/integration_test.rs:90-163`

**Problem**: All rate limit integration tests pass the client IP via
`X-Forwarded-For` header. No test verifies the `ConnectInfo` extraction path,
which is the primary path after C1 is fixed.

**Suggestion**: Add a test that sets `ConnectInfo` on the request extensions
(like `make_request_with_connect_info` in `src/proxy/handler.rs` tests) and
verifies rate limiting works without the `X-Forwarded-For` header.

---

## Summary Statistics

| Severity | Count | Status |
|----------|-------|--------|
| Critical | 4 | Must fix before production |
| Warning | 12 | Should fix — correctness and robustness |
| Suggestion | 10 | Consider for code quality |

## Recommended Fix Priority

1. **C1 (rate limiter X-Forwarded-For spoofing)** — Active security vulnerability.
   Any external attacker can bypass rate limiting entirely.
2. **C2 (InFlightCounter never increments)** — Graceful shutdown drain logic is
   completely non-functional. The drain always times out.
3. **C3 (connect timeout capped at 5s)** — Per-site timeout configuration is
   silently ignored for values > 5s.
4. **C4 (JSON format not applied without log file)** — Format config is silently
   ignored in a common deployment mode (stdout-only JSON logging).
5. **W1 (upstream host validation gap)** — Invalid upstream addresses pass config
   validation.
6. **W2 (ACME contact validation gap)** — Empty email after `mailto:` passes
   validation, fails at runtime.
7. **W3 (query string silently dropped)** — Data corruption on URI parse failure.
8. **W4 (admin socket resource limits)** — DoS vector for local socket access.
9. **W5 (TlsMode wildcard mismatch)** — Latent bug for future refactors.
10. **W6 (RawConfig/FullConfig duplication)** — Maintenance burden.
11. **Remaining W and S findings** — Fix opportunistically.
