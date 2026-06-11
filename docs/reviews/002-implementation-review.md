---
status: draft
last_updated: 2026-06-11
reviewed_code:
  - src/main.rs
  - src/server.rs
  - src/tls/acceptor.rs
  - src/tls/acme.rs
  - src/tls/config.rs
  - src/tls/redirect.rs
  - src/config/validation.rs
  - src/config/dynamic_config.rs
  - src/config/static_config.rs
  - src/config/mod.rs
  - src/proxy/handler.rs
  - src/proxy/headers.rs
  - src/proxy/error.rs
  - src/proxy/body_limit.rs
  - src/proxy/mod.rs
  - src/rate_limit/mod.rs
  - src/rate_limit/bucket.rs
  - src/admin/socket.rs
  - src/shutdown.rs
  - src/health.rs
  - src/logging/mod.rs
  - src/logging/format.rs
  - src/cli.rs
reviewer: code-reviewer
---

# Implementation Review #002

## Purpose

Post-implementation review of all modules. Each finding is structured as
**Problem** → **Solution** (or **Open Question** where no solution is yet known).

## Severity Definitions

| Severity | Meaning |
|----------|---------|
| **Critical** | Will cause incorrect behavior or security issues in production |
| **Warning** | Could cause issues under specific conditions or represents a missed edge case |
| **Suggestion** | Code quality, style, or minor improvement opportunity |

---

## Critical Findings

### C1. ACME Challenge Listener Not Started

**File**: `src/main.rs:168-181`

**Problem**: When `TlsMode::Acme` is matched, the `challenge_config` and `resolver`
fields are destructured with `_` (discarded). The ACME TLS-ALPN-01 challenge
listener — required for Let's Encrypt certificate provisioning — is never started.
Without it, ACME certificate issuance will fail: Let's Encrypt cannot verify domain
ownership, and no certificates will ever be obtained.

**Solution**: Wire the challenge config into a separate TLS listener or configure
the `rustls-acme` resolver to serve TLS-ALPN-01 challenges on the same port.
The `rustls-acme` crate's `ResolvesServerCertAcme` handles ALPN protocol
negotiation automatically — if the ACME challenge ALPN protocol (`acme-tls/1`)
is included in the server's ALPN list, the resolver will serve challenge
certificates on the main HTTPS port. Verify that the `build_acme_server_config`
function includes `ACME_TLS_ALPN_01` in its ALPN list (it already does at line 28).
This means the main listener should be sufficient for TLS-ALPN-01 challenges
if the resolver is correctly installed. The separate `challenge_config` in
`TlsMode::Acme` may be unnecessary — confirm whether `rustls-acme` requires a
dedicated listener or if the resolver on the default config handles challenges
automatically.

---

### C2. ACME Contact Email Always Empty

**File**: `src/tls/acme.rs:86`

**Problem**: `AcmeTlsConfig` always sets `contact: vec![]`. Let's Encrypt requires
a contact email for production certificate requests. The `acme_directory` config
field supports `"production"` but an empty contact list will cause ACME
registration to fail with a 400-level error from the Let's Encrypt API.

**Solution**: Add an `acme_contact` field (e.g., `acme_contact = "mailto:admin@example.com"`)
to the `TlsConfig` struct and wire it through to `AcmeTlsConfig.contact`. This
requires changes to:
1. `src/config/static_config.rs` — add `acme_contact: Vec<String>` to `TlsConfig`
2. `src/tls/acme.rs` — use `self.contact` in `AcmeTlsConfig::setup`
3. `src/tls/acceptor.rs` — pass the contact list from `TlsConfig`

---

### C3. X-Forwarded-For Replaces Instead of Appending

**File**: `src/proxy/headers.rs:28`

**Problem**: `inject_proxy_headers` uses `headers.insert()` for `X-Forwarded-For`,
which **replaces** any existing value. The spec (review #001, finding C2) decided
that as an edge proxy the X-Forwarded-For should be **set** (not appended),
because there are no trusted proxies in front. However, the implementation
doesn't match either behavior — it silently discards any existing header rather
than explicitly setting it to just the client IP, which is correct for an edge
proxy but was implemented without a clear comment explaining the rationale.

**Solution**: The current behavior is actually **correct** for an edge proxy
(per review #001/C2). The fix is documentation, not code change. Add a comment
to `inject_proxy_headers` explaining:

```rust
// X-Forwarded-For is SET (not appended) because this proxy is the outermost
// edge proxy. Any existing X-Forwarded-For from the client is untrusted and
// must be replaced with the actual client IP from ConnectInfo.
```

This ensures future maintainers don't "fix" this by changing `insert` to `append`.

---

### C4. ConfigReloadHandle Never Updates Stored StaticConfig

**File**: `src/config/dynamic_config.rs:124-148`

**Problem**: The `reload()` method computes `diff_static_config(&self.static_config, &new_static)`
and returns the changed fields, but **never updates `self.static_config`** with
the new static config. This means:
- The first reload correctly reports changed fields
- The second reload compares against the **original** static config, not the
  last-reloaded one, and reports the same changes again
- Operators get repeated "static config fields changed" warnings for the same
  fields on every reload

**Solution**: After validation succeeds and the diff is computed, update
`self.static_config` with the new value. Since `ConfigReloadHandle` is accessed
concurrently, the static config field needs interior mutability. Use
`ArcSwap<StaticConfig>` or `std::sync::RwLock<StaticConfig>`:

```rust
pub struct ConfigReloadHandle {
    config: Arc<ArcSwap<DynamicConfig>>,
    static_config: ArcSwap<StaticConfig>,  // Changed from StaticConfig
    reload_mutex: Mutex<()>,
}
```

Then in `reload()`:
```rust
let changed_fields = diff_static_config(&self.static_config.load(), &new_static);
self.static_config.store(Arc::new(new_static));
self.config.store(Arc::new(new_dynamic));
```

---

## Warning Findings

### W1. Shutdown Aborts Listeners Without Draining

**File**: `src/main.rs:238-240`

**Problem**: On shutdown, `handle.abort()` is called on each HTTPS server task.
This immediately kills the tokio task, which can interrupt in-flight request
processing. The `drain_in_flight` counter is only decremented in
`InFlightGuard::drop`, but `abort()` prevents the guard from being dropped
normally for connections still being processed by the task.

**Solution**: Instead of `abort()`, use `tokio::time::timeout` with the shutdown
timeout to `join` each handle:

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

Alternatively, keep the `shutdown_rx` pattern already in `serve_https_listener`
(which breaks the accept loop on shutdown signal) and remove the `abort()` calls
— the tasks will naturally exit when they stop accepting new connections and
existing connections drain.

---

### W2. Shutdown Doesn't Stop Admin Socket or Background Tasks

**File**: `src/main.rs:238-250`

**Problem**: The shutdown sequence aborts HTTPS listeners but doesn't stop:
- The admin socket listener (runs in infinite loop at `src/admin/socket.rs:99-111`)
- The rate limiter eviction task (runs in infinite loop at `src/rate_limit/mod.rs:106-112`)
- The ACME state machine task

These tasks will continue running until the process exits, which is fine for
process termination but means they can't be gracefully stopped in tests or
during a clean shutdown.

**Solution**: For Phase 1, this is acceptable — process termination will clean
up these tasks. For future improvements:
- Pass a `CancellationToken` or `watch::Receiver<bool>` to `start_admin_socket`
  and the eviction task
- On shutdown, signal cancellation before waiting for drain
- The ACME state machine task already exits when the stream ends (`None` branch
  at `src/tls/acme.rs:152-158`), but it should also be cancellable

---

### W3. Fragile Error Detection in Connection Error Handler

**File**: `src/server.rs:95-97`

**Problem**: The check `e.to_string().contains("incomplete message")` to silently
suppress connection errors is fragile. String matching on error descriptions can
break across hyper versions, locale changes, or error message reformatting.

**Solution**: Match on the error type instead. Check if `hyper` exposes a
client-disconnect error variant, or use `e.is_incomplete_message()` if available
in the hyper error API. If no typed variant exists, add a comment explaining
why string matching is used and which version(s) of hyper produce this message.

---

### W4. No Separate Connect Timeout for Upstream Requests

**File**: `src/proxy/handler.rs:73-79`

**Problem**: The proxy uses `tokio::time::timeout` with
`upstream_request_timeout_secs` for the entire request, but there's no separate
connect timeout. A slow DNS resolution or TCP handshake will consume the full
request timeout budget, leaving no time for the actual request/response cycle.

**Solution**: Add a `connect_timeout` (either a fixed default or from
`upstream_connect_timeout_secs` in `SiteConfig`). Structure the proxy call as:

```rust
let connect_timeout = Duration::from_secs(site.upstream_connect_timeout_secs);
let result = tokio::time::timeout(
    request_timeout,
    async {
        let upstream_req = build_upstream_request(req, &upstream_uri)?;
        // The hyper client handles connect internally, but we can wrap
        // the request in a two-phase timeout if needed
        client.request(upstream_req).await
    }
).await;
```

The `SiteConfig` already has `upstream_connect_timeout_secs` but it's not used
in `proxy_handler`. This should be wired up to either set the client's connect
timeout or to implement a two-phase timeout.

---

### W5. Hardcoded `/health` Path Intercepted on All Hosts

**File**: `src/proxy/handler.rs:37-39`

**Problem**: The proxy handler returns 200 OK for `GET /health` regardless of the
Host header. This means any site's `/health` path will be intercepted by the
proxy and never reach the upstream. If an upstream application uses `/health`
for its own health checks, those requests will never reach it.

**Solution**: Either:
1. Use a less common path like `/__health` or `/healthz` that won't collide
   with upstream applications, OR
2. Only intercept `/health` when the Host header doesn't match any known site
   (fallthrough), OR
3. Make the health check path configurable via `StaticConfig`

Option 1 is simplest for Phase 1. Option 3 is most flexible long-term.

---

### W6. Token Bucket Refill Uses Millisecond Precision

**File**: `src/rate_limit/bucket.rs:37`

**Problem**: `elapsed.as_millis()` truncates sub-millisecond time, which can lead
to token refill inaccuracies under high-frequency request bursts. For example,
two requests arriving 500µs apart both see `0ms` elapsed and don't refill tokens.

**Solution**: Use `as_nanos()` for the refill calculation:

```rust
let elapsed = now.duration_since(self.last_refill).as_nanos() as f64;
let tokens_to_add = (elapsed / 1_000_000_000.0) * rate;
```

This provides nanosecond-precision refill while keeping the math in floating point.

---

### W7. Admin Socket Has No Shutdown Mechanism

**File**: `src/admin/socket.rs:99-111`

**Problem**: `start_admin_socket` runs an infinite `loop` accepting connections
with no way to break out. It doesn't accept a shutdown signal, so it can't be
gracefully stopped during process shutdown or in tests.

**Solution**: Accept a `watch::Receiver<bool>` parameter and use `tokio::select!`
to check for shutdown:

```rust
tokio::select! {
    result = listener.accept() => { /* handle connection */ },
    _ = shutdown_rx.changed() => {
        info!("admin socket shutting down");
        break;
    }
}
```

This also requires cleaning up the socket file on exit.

---

### W8. Server Header Unconditionally Stripped from Upstream Response

**File**: `src/proxy/handler.rs:85`

**Problem**: `parts.headers.remove("server")` unconditionally removes the upstream
`Server` header. This is a design choice, not necessarily a bug, but it means
downstream clients can't see what software the upstream is running, which may be
undesirable for debugging.

**Solution**: This is acceptable behavior for a security-focused reverse proxy
(hiding upstream identity is a defense-in-depth measure). Document this decision
with a comment in the code explaining the rationale.

---

### W9. Logging Test Fails Due to Global Subscriber

**File**: `src/logging/mod.rs:99-111`

**Problem**: The test `init_creates_log_directory_and_file` calls `init()` which
sets a global default tracing subscriber. When tests run in parallel, this
conflicts with other tests that may also set a subscriber, causing the test to
fail with "a global default trace dispatcher has already been set."

**Solution**: Use `tracing_subscriber::fmt().with_test_writer()` and guard
against double-initialization. Alternatively, use `std::sync::OnceLock` or
`tracing_subscriber::util::SubscriberInitExt::try_init()` which returns an error
if already set rather than panicking.

---

### W10. Body Limit Middleware Only Checks Content-Length Header

**File**: `src/proxy/body_limit.rs:26-33`

**Problem**: The middleware checks the `Content-Length` header but doesn't handle
requests that lack `Content-Length` (e.g., chunked transfers, HTTP/2). A
malicious client can send `Transfer-Encoding: chunked` without `Content-Length`
and bypass the initial check, though the `Limited` body wrapper at line 37 will
still enforce the limit during streaming.

**Solution**: This is actually acceptable — the `Limited` body wrapper on line
37 is the real enforcement mechanism. The `Content-Length` check on line 26 is
an early-rejection optimization for clients that do include the header. Add a
comment explaining this two-layer defense:

```rust
// Early rejection: if Content-Length is present and exceeds the limit, reject
// immediately without reading the body. For requests without Content-Length
// (chunked, HTTP/2), the Limited body wrapper below enforces the limit during
// streaming.
```

---

### W11. Health Check Port Conflict Check Is Incomplete

**File**: `src/config/validation.rs:169-186`

**Problem**: The validation checks if `health_check_port` conflicts with any
listener's `https_port` or `http_port`, but it doesn't check whether the health
check port's bind address conflicts with a listener on a different bind address.
For example, `health_check_port = 80` bound to `127.0.0.1` shouldn't conflict
with a listener's `http_port = 80` bound to `203.0.113.10`.

**Solution**: This is acceptable for Phase 1 since the health check always binds
to `127.0.0.1` (hardcoded in `src/health.rs:20`). Document that health check
always binds to localhost, so the conflict check is conservative (warns even
when it might not actually conflict). Add a comment in the validation code
explaining this.

---

### W12. `build_upstream_request` Copies All Headers Without Filtering

**File**: `src/proxy/handler.rs:124-128`

**Problem**: `build_upstream_request` copies all headers from the original request
to the upstream request. However, `remove_hop_by_hop` was already called on the
original request's headers at line 59 before `build_upstream_request` is called,
so hop-by-hop headers have already been removed. The proxy headers
(`X-Real-IP`, `X-Forwarded-For`, `X-Forwarded-Proto`) were also already injected
at line 58. This means the function works correctly, but the code path is
spread across multiple locations, making it harder to reason about header
lifecycle.

**Solution**: Consider consolidating header manipulation into a single function
or adding inline comments that trace the header lifecycle:

```rust
// Header lifecycle:
// 1. inject_proxy_headers() — adds X-Real-IP, X-Forwarded-For, X-Forwarded-Proto
// 2. remove_hop_by_hop() — removes Connection, Keep-Alive, etc.
// 3. build_upstream_request() — copies remaining headers to upstream request
// 4. Response: remove_hop_by_hop() on upstream response headers
```

---

## Suggestions

### S1. `http_port` Validation Allows 0 but Not Documented

**File**: `src/config/validation.rs:106`

**Problem**: `http_port = 0` is treated as "disabled" (not validated as a port
number), which is correct, but the validation error message for invalid ports
says "must be 0 (disabled) or 1-65535" while `HttpPortInvalid` only checks
`http_port > 0 && http_port < 1` implicitly. The actual check is `http_port > 0`
to enter the conflict check, and port 0 is always allowed for HTTP.

**Solution**: Add explicit validation for `http_port` being in the range 0 or
1-65535 (currently it only validates conflicts, not the port range itself for
http). Add a `HttpPortInvalid` check similar to `HttpsPortInvalid`:

```rust
if listener.http_port > 65535 {
    errors.push(ValidationError::HttpPortInvalid { ... });
}
```

---

### S2. Consider Using `#[non_exhaustive]` on Public Enums

**Files**: `src/tls/acceptor.rs:49` (`TlsMode`), `src/proxy/error.rs:5` (`ProxyError`),
`src/admin/socket.rs:15` (`AdminSocketError`), `src/config/validation.rs:10` (`ValidationError`)

**Problem**: These public enums can be matched exhaustively by downstream consumers.
Adding a new variant would be a breaking change.

**Solution**: Add `#[non_exhaustive]` to these enums to allow future expansion
without breaking changes. This is especially important for `TlsMode` (which may
gain a `"letsencrypt"` or `"auto"` mode) and `ProxyError` (which may gain
`UpstreamTls` error handling).

---

### S3. `normalize_host` in `dynamic_config.rs` Doesn't Handle Edge Cases

**File**: `src/config/dynamic_config.rs:52-55`

**Problem**: `normalize_host` uses `split(':').next()` to strip ports, but this
fails for IPv6 addresses in brackets (e.g., `[::1]:443` would normalize to
`[::1]` instead of `::1`). The `strip_port_from_host` function in
`src/tls/redirect.rs:16-28` correctly handles this case.

**Solution**: Either reuse the `strip_port_from_host` logic from `redirect.rs`
or add a shared utility function. The IPv6 bracket handling is important for
correctness if the proxy ever receives an IPv6 Host header.

---

### S4. Multiple `#[allow(dead_code)]` Annotations on Public API

**Files**: `src/tls/acceptor.rs:14,33,48,58`, `src/tls/acme.rs:9,11,15,23,55`,
`src/config/static_config.rs:4,31,44,49,54,56,70,76,86,91`

**Problem**: Many public items are annotated with `#[allow(dead_code)]`, which
suggests they're defined but not yet used by the binary crate. This is fine
during initial development but should be cleaned up before release.

**Solution**: Remove `#[allow(dead_code)]` annotations once all features are
wired up (especially after C1 is fixed, since `build_acme_challenge_config` and
`TlsMode::Acme.challenge_config` will be needed). Run `cargo check` to identify
which items are actually dead code vs. just not yet referenced.

---

### S5. `InFlightCounter` Could Use Atomic Usize Directly

**File**: `src/server.rs:16-46`

**Problem**: `InFlightCounter` wraps an `AtomicUsize` with `increment`/`decrement`/`is_zero`
methods, plus a separate `InFlightGuard` RAII type. This is clean, but the
`Arc<InFlightCounter>` pattern means every connection task clones the `Arc`,
which has a small allocation cost.

**Solution**: This is a fine pattern for correctness. No change needed — the
RAII guard pattern correctly ensures `decrement` is always called even on panic.
Mentioning as a suggestion for awareness, not action.

---

### S6. Add `Clone` to `SiteConfig` Derive Is Correct But May Mask Cloning Hot Paths

**File**: `src/config/dynamic_config.rs:70`

**Problem**: `SiteConfig` derives `Clone`, which is used in
`src/proxy/handler.rs:53` where `site.clone()` clones the site config on every
request. For hot paths, this allocates new `String`s for `host`, `upstream`, and
`upstream_scheme`.

**Solution**: Consider using `Arc<SiteConfig>` in the routing table so that
lookups return an `Arc` clone (cheap atomic refcount) instead of a full `String`
clone. This would change `routing_table` from `HashMap<String, SiteConfig>` to
`HashMap<String, Arc<SiteConfig>>`. This is a performance optimization for
later — not a correctness issue.

---

### S7. HTTPS Client Trusts System Root Certificates Unconditionally

**File**: `src/proxy/handler.rs:153-165`

**Problem**: The `root_certs()` function loads native certificates and silently
skips any that fail to parse (`roots.add(cert).ok()`). While this matches the
spec (review #001, W13), certificate validation failures for upstream TLS
connections will produce opaque IO errors.

**Solution**: This is acceptable for Phase 1 per the spec (ADR-017). Consider
logging the number of root certificates loaded for operational visibility:

```rust
let cert_count = result.certs.len();
info!(certs_loaded = cert_count, "loaded system root certificates");
```

---

### S8. Request Timeout Applies to Entire HTTP Exchange, Not Just Response

**File**: `src/proxy/handler.rs:75-79`

**Problem**: `tokio::time::timeout(request_timeout, client.request(...))` applies
the timeout to the entire HTTP round-trip including response body streaming.
For large file downloads or slow upstreams, this means a 60-second timeout kills
the response even if the upstream is actively sending data. A more precise
timeout would apply only to the connection + first-byte response, then stream
the body without a timeout.

**Solution**: For Phase 1, this is acceptable behavior — the timeout name
(`upstream_request_timeout_secs`) is documented as applying to the full request.
Consider splitting into connect-timeout and response-timeout in Phase 2. The
`SiteConfig` already has separate `upstream_connect_timeout_secs` and
`upstream_request_timeout_secs` fields, but `upstream_connect_timeout_secs` is
unused (see W4).

### W13. No Request Access Logging

**File**: `src/proxy/handler.rs`, `src/logging/format.rs`

**Problem**: The `proxy_handler` function does not log successful or failed proxied
requests. The `log_request!` macro is defined in `src/logging/format.rs` but is
never called anywhere in the codebase. Without request-level access logging, there
is no observability into what the proxy is doing — no way to see which hosts are
being requested, response status codes, upstream latency, or client IPs for normal
traffic. This also means fail2ban cannot consume proxy access logs for the scanner-
catching jails (nginx-badbots, nginx-botsearch) that were part of the original
nginx setup.

**Solution**: Add request logging in `proxy_handler` that emits a structured log
line for every proxied request (method, host, path, status code, upstream address,
duration, client IP). Use the `log_request!` macro that already exists. This is
critical for production — without it, the proxy is operationally blind.

---

### W14. `is_https` Hardcoded to `true` on ProxyState

**File**: `src/main.rs:77`

**Problem**: `ProxyState.is_https` is always set to `true`:

```rust
let proxy_state = Arc::new(ProxyState {
    config: config_arc.clone(),
    http_client,
    https_client,
    is_https: true,
});
```

This means `X-Forwarded-Proto` will always be set to `https` in
`inject_proxy_headers`. For a TLS-terminating reverse proxy this is correct
(since all connections arrive over TLS), but it's a latent bug if the proxy is
ever used in a non-TLS context or if someone adds an HTTP listener that routes
through the same handler.

**Solution**: Either remove the `is_https` field and hardcode `"https"` in the
header injection (with a comment explaining why), or derive the value from the
listener configuration so each listener sets it correctly. For Phase 1, a comment
explaining the assumption is sufficient.

---

### S9. Rate Limiting Silently Bypassed When No Client IP Found

**File**: `src/rate_limit/mod.rs:78-79`

**Problem**: When `rate_limit_middleware` cannot extract a client IP (no
`X-Forwarded-For` header and no `ConnectInfo` extension), the request passes
through without rate limiting. In the current deployment, `ConnectInfo` is always
set by `ConnectInfoService` in `server.rs`, so this won't happen in practice.
However, if the proxy were ever deployed without `ConnectInfo` propagation (e.g.,
behind another load balancer that strips it), rate limiting would silently become
a no-op for all requests.

**Solution**: Consider logging a warning when a request has no identifiable
client IP, and optionally returning a 429 or 503 instead of passing through.
For Phase 1, add a comment documenting the fallthrough behavior.

---

### S10. Integration Test TOML Has Double-Nested Listeners Key

**File**: `tests/integration_test.rs:514`

**Problem**: The `write_valid_config` helper contains:

```toml
[[listeners.listeners.sites]]
```

This should be `[[listeners.sites]]` — the double-nested `listeners.listeners`
is a TOML path that doesn't match the config schema. The test passes because the
binary validates the file and exits with an error, and the test checks that
`--validate` with this file fails (it's used as an invalid config test fixture).
However, the variable name `write_valid_config` suggests it was intended to produce
a valid config, and the invalid TOML path makes the test's intent unclear.

**Solution**: Fix the TOML key to `[[listeners.sites]]` and verify the test still
passes, or rename the function to `write_invalid_config` if the invalid TOML is
intentional.

---

### S11. No Server Response Header Added by Proxy

**File**: `src/proxy/handler.rs:85`

**Problem**: The proxy strips the upstream `Server` header (per W8) but does not
add its own `Server` header. Some load balancers, monitoring tools, and HTTP
spec-compliant clients expect a `Server` header in responses. This is a minor
observation, not a bug — omitting `Server` is a valid security hardening choice
(hiding server identity).

**Solution**: This is acceptable for Phase 1. If desired later, add a configurable
`Server` header (e.g., `Server: reverse-proxy`) or leave it absent.

---

## Summary Statistics

| Severity | Count | Status |
|----------|-------|--------|
| Critical | 4 | Must fix before production |
| Warning | 14 | Should fix — correctness and robustness |
| Suggestion | 11 | Consider for code quality |

## Recommended Fix Priority

1. **C1 (ACME challenge listener)** — Without this, ACME cert provisioning is
   completely broken. This is the highest priority fix.
2. **C2 (ACME contact email)** — Without this, production Let's Encrypt
   registration fails.
3. **C4 (ConfigReloadHandle static config drift)** — Every reload will produce
   stale warnings, confusing operators.
4. **C3 (X-Forwarded-For comment)** — Correct behavior, just needs a clarifying
   comment to prevent future "fixes."
5. **W13 (No request access logging)** — Proxy is operationally blind without
   access logs; fail2ban cannot consume proxy traffic data.
6. **W1 (Shutdown drain)** — Can cause connection drops on graceful restart.
7. **W4 (Connect timeout)** — Slow upstreams can exhaust the full request
   timeout budget.
8. **W5 (Health path collision)** — Upstream `/health` endpoints are silently
   intercepted.
9. **W7 (Admin socket shutdown)** — Cannot gracefully stop.
10. **W14 (is_https hardcoded)** — Latent bug for non-TLS contexts; needs comment.
11. **Remaining W and S findings** — Fix opportunistically.