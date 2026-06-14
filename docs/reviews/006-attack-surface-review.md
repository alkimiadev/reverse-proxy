---
status: draft
last_updated: 2026-06-14
reviewed_code:
  - src/server.rs
  - src/proxy/handler.rs
  - src/proxy/headers.rs
  - src/proxy/body_limit.rs
  - src/proxy/error.rs
  - src/proxy/mod.rs
  - src/tls/acceptor.rs
  - src/tls/acme.rs
  - src/tls/config.rs
  - src/tls/redirect.rs
  - src/rate_limit/mod.rs
  - src/rate_limit/bucket.rs
  - src/admin/socket.rs
  - src/shutdown.rs
  - src/config/static_config.rs
  - src/config/dynamic_config.rs
  - src/config/validation.rs
  - src/config/mod.rs
  - src/health.rs
  - src/cli.rs
  - src/main.rs
  - src/utils.rs
  - src/logging/mod.rs
  - src/logging/format.rs
reviewer: code-reviewer
---

# Attack Surface Review #006

## Purpose

Systematic enumeration of every point where untrusted input enters the reverse
proxy, with analysis of where that input intersects with logic decisions. The
goal is not to find specific vulnerabilities (see review #005 for that) but to
map the entire attack surface so that current and future reviewers know where to
focus adversarial attention.

This review was motivated by the observation that Rust's memory model eliminates
entire bug classes, meaning the remaining interesting surface is where
**client-controlled data influences a decision or construction** — the "glue"
layer between battle-tested components.

---

## Methodology

Every code path that receives data from outside the process was traced from its
entry point through to where it is consumed, transformed, or forwarded. Each
path was classified by:

- **Source**: Where the data originates (network, filesystem, OS signal, etc.)
- **Entry point**: File and line where the data first enters our code
- **Validation**: What checks are applied before the data is used
- **Sink**: Where the data ends up (upstream connection, filesystem, URL
  construction, routing decision, etc.)
- **Risk**: How much control an attacker has and what the consequences are

Findings are grouped by entry point category.

---

## Category 1: Incoming HTTPS/TLS Connections

### 1.1 TCP Connection Accept

**Source**: Network (any IP)
**Entry**: `src/server.rs:67-68` — `tcp_listener.accept()`
**Input**: Raw TCP SYN from any source
**Validation**: OS TCP handshake only. No IP allowlist/denylist.
**Sink**: TLS handshake
**Risk**: None at this layer. Connection errors are logged and the loop
continues. No resource limiting beyond OS TCP backlog.

### 1.2 TLS ClientHello

**Source**: Network (TLS ClientHello)
**Entry**: `src/server.rs:83` — `tls_acceptor.accept(tcp_stream).await`
**Input**: SNI hostname, cipher suites, ALPN protocols, TLS version range,
client certificate (not requested — `with_no_client_auth()` at
`tls/config.rs:62`)
**Validation**:
- Cipher suites restricted to 7 approved suites (`tls/config.rs:14-22`)
- Protocol versions limited to TLS 1.2/1.3 (`tls/config.rs:9,61`)
- Key exchange limited to X25519, SECP256R1, SECP384R1 (`tls/config.rs:28`)
- Failed handshakes produce a warn log and connection drop (`server.rs:85-88`)
**Sink**: ALPN detection at `server.rs:91-92`, then HTTP handler
**Risk**: **SNI hostname is not validated at the TLS layer.** Any SNI value
completes the handshake. Host-based routing only happens at the HTTP layer
(`handler.rs:39-55`). This means TLS connections for arbitrary hostnames are
accepted and complete before receiving a 404, potentially leaking certificate
information (which certificate is served, what names it covers). This is
standard for multi-host proxies but worth noting.

### 1.3 ALPN Protocol Selection

**Source**: Network (TLS ClientHello ALPN extension)
**Entry**: `src/server.rs:91-92` — `tls_stream.get_ref().1.alpn_protocol()`
**Input**: ALPN protocol identifier bytes (e.g., `b"h2"`, `b"http/1.1"`)
**Validation**: Binary comparison only: `alpn == Some(b"h2")` selects h2;
anything else falls through to the auto-detect builder (`server.rs:92-125`).
No explicit rejection of unexpected ALPN values.
**Sink**: Determines whether HTTP/2 or HTTP/1.1 handler is used
**Risk**: Low. hyper handles unexpected ALPN values by falling back to HTTP/1.1
semantics.

### 1.4 HTTP/2 and HTTP/1.1 Request Streams

**Source**: Network (HTTP frames over TLS)
**Entry**: `src/server.rs:103-125` — hyper connection handlers
**Input**: Full HTTP request stream (method, URI, headers, body)
**Validation**: Delegated entirely to hyper's HTTP parser. No application-level
validation of frame sizes, header counts, or connection settings beyond what
hyper enforces.
**Sink**: Axum router → proxy handler
**Risk**: Medium. hyper is well-vetted, but the proxy applies no additional
request validation layer (no header count limits, no total header size limits,
no max URI length). These are defense-in-depth measures that proxies commonly
implement.

---

## Category 2: HTTP Request Processing

### 2.1 HTTP Method

**Source**: Network (HTTP request line)
**Entry**: `src/proxy/handler.rs:37` — `req.method().clone()`
**Input**: HTTP method string (GET, POST, custom methods, etc.)
**Validation**: None by the application. Method is cloned and forwarded to
upstream as-is via `build_upstream_request` (`handler.rs:218-228`).
**Sink**: Logged, forwarded to upstream
**Risk**: Low. Hyper validates that the method is syntactically valid. Unusual
methods (CONNECT, TRACE, etc.) are forwarded to upstream, which may or may not
handle them. No method allowlist is applied.

### 2.2 Request URI (Path + Query String) — **HIGHEST PRIORITY SURFACE**

**Source**: Network (HTTP request line)
**Entry**: `src/proxy/handler.rs:38` — `req.uri().path().to_string()`
**Input**: Full request path and query string (e.g., `/api/users?foo=bar`)
**Validation**:
- `Uri::parse` provides structural validation in `build_upstream_uri()`
  (`handler.rs:206-216`)
- If URI parse fails, 502 is returned (`handler.rs:67-80`)
- **No application-level sanitization** of:
  - Path traversal sequences (`/../`, `/..%2f`)
  - Double-encoded characters (`%252e%252e`)
  - CRLF injection in query strings (`?foo=bar%0d%0aInjected: header`)
  - Null bytes (`%00`)
  - Unicode normalization differences
**Sink**: Reconstructed as `scheme://upstream/path?query` and forwarded to
upstream service
**Risk**: **High.** This is the most important surface for adversarial
analysis. The client's raw path and query are concatenated verbatim into the
upstream URL. Whether this matters depends on the upstream, but a
security-focused reverse proxy should normalize paths and reject or sanitize
dangerous patterns. Specific attack vectors:

- **Path traversal**: A request to `/../../../etc/passwd` would be forwarded
  as `http://upstream:8080/../../../etc/passwd`. Whether this reaches
  sensitive files depends entirely on the upstream.
- **CRLF injection**: A query string containing `%0d%0a` could inject
  additional headers or HTTP content in the upstream request if the upstream
  or an intermediary interprets raw CR/LF in query parameters.
- **Query string smuggling**: The `build_upstream_uri` function concatenates
  path and query without normalization. Ambiguous URLs like
  `//evil.com%23@target` could potentially confuse upstream parsers.

### 2.3 Host Header — Routing Decision Point

**Source**: Network (HTTP Host header)
**Entry**: `src/proxy/handler.rs:39-44`
**Input**: Host header value or URI host component
**Validation**:
- Empty/missing Host → 400 Bad Request (`handler.rs:46-47`)
- Host is used to look up site in routing table via `normalize_host()`
  (`dynamic_config.rs:52-61`): strips port, lowercases, strips IPv6 brackets
- Unknown host → 404 (`handler.rs:55`)
- `to_str()` on the header value fails on non-ASCII/invalid bytes (implicit
  validation by hyper)
**Sink**: Routing decision (which upstream to proxy to)
**Risk**: Low-Medium. The normalized host is used only for routing lookup, not
for URL construction (upstream host comes from config). However:
- **Unicode homoglyphs**: `normalize_host` lowercases but does not handle
  Unicode confusables (e.g., `ⓔⓧⓐⓜⓟⓛⓔ.com` vs `example.com`). This is
  mitigated by `to_str()` rejecting non-ASCII, so only ASCII hosts reach the
  lookup.
- **Very long hosts**: No length limit on Host header value before routing
  lookup. A multi-megabyte Host header would be hashed for HashMap lookup.
- **IPv6 bracket handling**: `normalize_host` strips brackets from
  `[::1]:443` → `::1`. A malformed Host like `[invalid` would have brackets
  stripped incorrectly, but would simply fail the routing lookup (404).

### 2.4 All Other Client Request Headers

**Source**: Network (HTTP request headers)
**Entry**: `src/proxy/handler.rs:59-60` — `inject_proxy_headers()` and
`remove_hop_by_hop()`, then `handler.rs:223-225` in `build_upstream_request()`
**Input**: Complete set of HTTP headers sent by the client
**Validation/Sanitization**:
- **X-Forwarded-For is REPLACED** with actual client IP (not appended) —
  prevents XFF spoofing (`headers.rs:28-32`)
- **X-Real-IP is SET** to actual client IP (`headers.rs:26`)
- **X-Forwarded-Proto is SET** to "https" (`headers.rs:37-40`)
- **Hop-by-hop headers are REMOVED** (`headers.rs:4-13`): connection,
  keep-alive, proxy-authorization, proxy-authenticate, te, trailers,
  transfer-encoding, upgrade
- **All other headers are forwarded as-is** (`handler.rs:223-225`)
**Sink**: Forwarded to upstream service
**Risk**: Medium. No allowlist or additional blocklist is applied beyond
hop-by-hop headers. Potential concerns:
- **Request smuggling via ambiguous headers**: Headers like
  `Transfer-Encoding: chunked` on a HTTP/1.0 request, or duplicate
  `Content-Length` values, could create discrepancies between how hyper parses
  the request and how the upstream interprets it. Hyper normalizes these, but
  the gap between hyper's parsing and the upstream's parsing is where request
  smuggling lives.
- **Header injection via upstream URL**: Not applicable here since the
  upstream URL is constructed from config values, not client input (except
  path/query, covered in 2.2).

### 2.5 Request Body

**Source**: Network (HTTP request body)
**Entry**: `src/proxy/body_limit.rs:14-45` — `body_limit_middleware`
**Input**: Arbitrary request body bytes
**Validation**:
- **Content-Length early rejection**: If `Content-Length` header value exceeds
  `limit_bytes`, 413 is returned without reading the body (`body_limit.rs:30-38`)
- **Invalid Content-Length**: Silently ignored via `if let Ok` chains
  (`body_limit.rs:31-32`). Streaming limit applies instead.
- **Negative Content-Length**: Cannot exist — `u64::parse` rejects negative
  strings.
- **Streaming body limit**: `Limited::new(body, limit as usize)` enforces the
  limit during streaming for chunked/HTTP2 requests (`body_limit.rs:41`)
- Default limit: 100 MiB (`body_limit.rs:12`)
**Sink**: Forwarded to upstream as-is
**Risk**: Low. Body size is properly bounded. No content-type validation or
body content inspection — the proxy treats the body as opaque bytes, which is
correct for a reverse proxy.

---

## Category 3: HTTP Redirect Listener

### 3.1 Host Header (Redirect)

**Source**: Network (HTTP Host header on plaintext listener)
**Entry**: `src/tls/redirect.rs:41-46`
**Input**: Host header value from unencrypted HTTP request
**Validation**:
- Empty/missing Host → 400 (`redirect.rs:48-49`)
- Port is stripped via `strip_port_from_host()` (`utils.rs:1-13`)
- **`HeaderValue::from_str()` validates** the constructed Location header value
  at `redirect.rs:69`. Rejects non-ASCII and null bytes.
**Sink**: Constructed into `Location: https://{host}:{port}/{path}` redirect
response
**Risk**: **Medium.** This is the classic open redirect / header injection
vector. The Host header is directly interpolated into the Location response
header. While `HeaderValue::from_str()` rejects bytes that would break HTTP
framing (null bytes, non-ASCII), it does **not** reject:
- Hosts with embedded CRLF (`\r\n`) — but `to_str()` should fail on these
  since header values can't contain raw CR/LF per HTTP spec
- Numeric IP addresses pointing to internal services (open redirect to
  `127.0.0.1`)
- Hosts that look like different origins (`evil.example.com` when the proxy
  serves `example.com`)

The `HeaderValue::from_str()` check at line 69 is the primary defense against
header injection. If `to_str()` on the Host header properly rejects `\r\n`,
this is safe. **This should be explicitly tested.**

### 3.2 Request URI Path/Query (Redirect)

**Source**: Network (HTTP request URI on plaintext listener)
**Entry**: `src/tls/redirect.rs:52-53`
**Input**: Request path and query string
**Validation**:
- Paths starting with `/.well-known/acme-challenge/` return 404 instead of
  redirect (`redirect.rs:55-65`)
- Path normalization: empty or non-`/`-prefixed paths get `/` prepended
  (`redirect.rs:27-30`)
- `HeaderValue::from_str()` final safety check on Location value
**Sink**: Concatenated into redirect Location URL via `build_redirect_url()`
**Risk**: Low-Medium. The path is appended to the redirect URL. CRLF in the
path could theoretically inject headers if not properly rejected, but
`HeaderValue::from_str()` provides a final safety net.

---

## Category 4: Config File Input

### 4.1 Config File Read (Startup)

**Source**: Filesystem (TOML config file)
**Entry**: `src/cli.rs:49` — `std::fs::read_to_string(config_path)`
**Input**: Entire contents of the config file
**Validation**:
- File must exist and be readable
- Content parsed as TOML via `serde::Deserialize` — enforces types and required
  fields
- 20+ business-rule validations in `src/config/validation.rs:78-273`
**Sink**: Parsed into `StaticConfig` + `DynamicConfig`, used for all runtime
decisions (bind addresses, TLS, upstream targets, rate limits, etc.)
**Risk**: Medium. Config is trusted input (operator controls it), but:
- **Config file TOCTOU**: Between validation and use, the file could be
  replaced (low practical risk since startup reads it once)
- **Upstream values in config** are validated for format (`host:port`) but
  could point to internal services, creating SSRF-like risks if an attacker
  can modify the config file

### 4.2 Config File Read (Reload — SIGHUP)

**Source**: Filesystem (same config file, re-read on SIGHUP)
**Entry**: `src/shutdown.rs:88` — `tokio::fs::read_to_string(config_path).await`
**Input**: Entire contents of the config file at reload time
**Validation**: Same `FullConfig::parse()` + `validate()` pipeline. Failed
  parse/validation retains old config (failsafe).
**Sink**: Swapped into `ArcSwap<DynamicConfig>` and `ArcSwap<StaticConfig>`
**Risk**: Medium. **TOCTOU between reads**: If another process is writing the
  config file at the exact moment SIGHUP triggers a read, a partial file could
  be read. The parse would likely fail (invalid TOML), triggering a reload
  error, which is safe. But a carefully timed write could produce a valid-but-
  malicious partial file. This is the same issue noted in review #005 (W2).

### 4.3 Config File Read (Reload — Admin Socket)

**Source**: Filesystem (same config file, re-read on admin "reload" command)
**Entry**: `src/admin/socket.rs:257`
**Input**: Same as 4.2
**Validation**: Same pipeline, plus reload mutex serialization
**Risk**: Same as 4.2. Additionally, the admin socket is unauthenticated
(review #005, C2), so any local user can trigger a config re-read.

### 4.4 Config Values Used in URL Construction

**Source**: Filesystem (site `upstream` and `upstream_scheme` from config)
**Entry**: `src/proxy/handler.rs:62-64`
**Input**: `upstream` (e.g., `"127.0.0.1:8080"`) and `upstream_scheme`
  (e.g., `"http"`) from config
**Validation**: `is_valid_upstream()` validates `host:port` format at config
  time (`validation.rs:306-332`). `upstream_scheme` must be `"http"` or
  `"https"` (`validation.rs:251-256`).
**Sink**: Constructed into `format!("{}://{}", upstream_scheme, upstream)` at
  `handler.rs:64`, then used as base URL for all proxied requests
**Risk**: Low. Values come from trusted config, not client input. However, if
  config is compromised, `upstream` could point to an attacker-controlled
  service (SSRF via config).

---

## Category 5: TLS / ACME

### 5.1 ACME TLS-ALPN-01 Challenge

**Source**: Network (TLS ClientHello with ALPN `acme-tls/1`)
**Entry**: `src/tls/acceptor.rs:25-29`
**Input**: TLS connection using ALPN `acme-tls/1` during certificate validation
**Validation**: Handled entirely by `rustls_acme` library. No application code
  processes the challenge request.
**Risk**: Low. Delegated to well-audited library.

### 5.2 ACME HTTP-01 Challenge (Redirect Listener)

**Source**: Network (HTTP request to `/.well-known/acme-challenge/`)
**Entry**: `src/tls/redirect.rs:55-65`
**Input**: HTTP request path
**Validation**: The redirect listener **returns 404** for all ACME challenge
  paths. It does NOT serve challenge responses.
**Risk**: None. The proxy explicitly does not handle HTTP-01 challenges.

### 5.3 ACME Certificate Cache

**Source**: Filesystem (cached certificates in `acme_cache_dir`)
**Entry**: `src/tls/acme.rs:36` — `DirCache::new(self.cache_dir.clone())`
**Input**: Previously cached ACME certificates and account data from disk
**Validation**: Handled by `rustls_acme`. Cache load failures are logged.
  Invalid cached certs produce `CachedCertParse` errors.
**Risk**: Low. If an attacker can write to `acme_cache_dir`, they could
  substitute a cached certificate. This would be detected by ACME validation
  on next renewal, but could allow temporary MITM.

### 5.4 ACME Directory URL

**Source**: Config file
**Entry**: `src/tls/acme.rs:29-33`
**Input**: The `acme_directory` config value — can be `"production"`,
  `"staging"`, or an arbitrary URL string
**Validation**: No URL format validation. Any string that isn't `"production"`
  or `"staging"` is used as a raw URL.
**Risk**: Medium. If config is compromised, an attacker could point the ACME
  client at a malicious server, potentially obtaining fraudulent certificates
  for the configured domains.

### 5.5 Manual Certificate and Key Files

**Source**: Filesystem (PEM certificate and key files)
**Entry**: `src/tls/config.rs:33-53` — `load_certs()` and `load_private_key()`
**Input**: PEM-encoded certificate chain and private key from disk
**Validation**:
- File must be readable
- PEM must parse as certificates/key
- At least one certificate must be present
- Config validation checks file existence at startup
- **Certificate expiry, chain validity, and key-match are NOT checked at load
  time** — left to rustls runtime errors
**Risk**: Low for external attack. Expired or mismatched certs will cause
  runtime TLS errors. The risk is operational (service disruption from
  expired certs that could have been caught at startup).

---

## Category 6: Admin Interface

### 6.1 Admin Socket Connections

**Source**: Local process (Unix domain socket)
**Entry**: `src/admin/socket.rs:110` — `listener.accept()`
**Input**: Unix domain socket connections from local processes
**Validation**: No authentication or peer credential checking. Any process with
  filesystem access to the socket can connect.
**Risk**: Covered extensively in review #005 (C2). Being replaced by
  authenticated HTTP endpoint per the architectural recommendation in that
  review.

### 6.2 Admin Command Input

**Source**: Local process (text sent over Unix socket)
**Entry**: `src/admin/socket.rs:167-252` — `handle_connection()`
**Input**: Text command string (expected: "reload", "status", or empty/unknown)
**Validation**:
- 4 KiB read limit via `.take(4096)` (`socket.rs:171`)
- 5-second read timeout (`socket.rs:174-175`)
- Newline termination required
- Command allowlist: only "reload" and "status" are recognized
- Unknown commands echo input back: `"unknown command: {input}"` (minor
  information disclosure)
**Risk**: Covered in review #005 (C3, W1).

---

## Category 7: OS Signals

### 7.1 SIGTERM/SIGINT

**Source**: OS kernel (signal delivered to process)
**Entry**: `src/shutdown.rs:51` — `Signals::new([SIGTERM, SIGINT, SIGHUP])`
**Input**: Signal number
**Validation**: Only registered signals are processed. SIGTERM/SIGINT trigger
  graceful shutdown.
**Risk**: None. Signals carry no data payload.

### 7.2 SIGHUP (Config Reload Trigger)

**Source**: OS kernel
**Entry**: `src/shutdown.rs:70-72` — calls `handle_sighup_reload()`
**Input**: SIGHUP signal triggers re-reading config file from disk
**Validation**: Full config validation pipeline on the re-read config. Failsafe:
  old config is retained on failure.
**Risk**: See 4.2. The signal itself is harmless; the risk is in the
  subsequent filesystem read.

---

## Category 8: Environment and CLI

### 8.1 CLI Arguments

**Source**: Process invocation (command line)
**Entry**: `src/cli.rs:35-37` — `Cli::parse()` (clap)
**Input**: `--config` (path string), `--validate` (flag),
  `--allow-wildcard-bind` (flag)
**Validation**: Clap enforces type constraints. Config path not validated for
  existence until `load_config()` is called.
**Risk**: Low. CLI args are trusted input (operator controls them).

### 8.2 NOTIFY_SOCKET (systemd)

**Source**: Environment variable
**Entry**: `src/main.rs:24` — `std::env::var("NOTIFY_SOCKET")`
**Input**: Path to systemd notification socket
**Validation**: Only checked for existence. `sd_notify::notify()` handles the
  socket communication.
**Risk**: None. Standard systemd protocol.

### 8.3 RUST_LOG (Tracing Filter)

**Source**: Environment variable
**Entry**: `src/logging/mod.rs:25` — `EnvFilter::from_default_env()`
**Input**: Tracing filter directives (e.g., `RUST_LOG=debug`)
**Validation**: Handled by `tracing_subscriber`. Invalid directives are silently
  ignored.
**Risk**: Low. Could be used to increase log verbosity, potentially leaking
  sensitive request data in logs. Only exploitable by someone who can set
  environment variables (typically operator-level access).

---

## Category 9: Health Check Endpoint

### 9.1 Health Check Request

**Source**: Network (localhost:9900 only)
**Entry**: `src/health.rs:9` — `health_handler()`
**Input**: HTTP GET request to `/health`
**Validation**: Binds only to 127.0.0.1 (`health.rs:20`). Only `/health` path
  is routed. No request body, headers, or parameters are read. Returns 200 OK
  with empty body.
**Risk**: None. Minimal surface.

---

## Category 10: Rate Limiter

### 10.1 Client IP for Rate Limiting

**Source**: Network (TCP connection remote address via ConnectInfo)
**Entry**: `src/rate_limit/mod.rs:66-69`
**Input**: Client IP address from the TCP layer (kernel-provided)
**Validation**:
- Uses `ConnectInfo<SocketAddr>` (kernel-provided, not client-supplied
  X-Forwarded-For) — prevents IP spoofing
- If no ConnectInfo is present, request is rejected with 429 (`mod.rs:71-72`)
- IPv6 addresses are normalized to /64 prefix (`bucket.rs:50-68`) to prevent
  /128 evasion
**Sink**: Used as token bucket key for rate limiting decisions
**Risk**: Low. The IP comes from the kernel, not from client headers. IPv6 /64
normalization prevents address expansion attacks.

---

## Category 11: Upstream Response

### 11.1 Upstream Response Headers

**Source**: Network (upstream service response)
**Entry**: `src/proxy/handler.rs:130-131`
**Input**: All HTTP response headers from upstream
**Validation/Sanitization**:
- Hop-by-hop headers are removed (`handler.rs:131`)
- The `Server` header is explicitly removed (`handler.rs:135`)
- **All other response headers are forwarded to the client as-is**
**Sink**: HTTP response sent to the client
**Risk**: Low-Medium. Headers like `X-Powered-By`, `X-AspNet-Version`, etc.
  that could leak upstream technology stack information are forwarded. This is
  an information disclosure risk, not a functional vulnerability.

### 11.2 Upstream Response Body

**Source**: Network (upstream service response body)
**Entry**: `src/proxy/handler.rs:136-137`
**Input**: Arbitrary response body bytes from upstream
**Validation**: **None.** No size limit on upstream response body. Streamed
  directly to the client.
**Sink**: HTTP response body sent to the client
**Risk**: Low. The proxy streams the response through without buffering the
  entire body (axum/hyper streaming model), so memory impact is bounded by
  chunk size, not total body size. However, there is no response body size
  limit, which could allow bandwidth amplification if an upstream returns very
  large responses.

---

## Category 12: Upstream TLS Certificate Verification

### 12.1 Upstream TLS Certificate

**Source**: Network (upstream server's TLS certificate)
**Entry**: `src/proxy/handler.rs:240-258` — `create_https_client()`
**Input**: TLS certificate presented by the upstream service during HTTPS
  proxy connections
**Validation**: Standard WebPKI validation using system root certificates
  (`rustls_native_certs::load_native_certs()` at `handler.rs:262`). No custom
  certificate pinning or allowlisting.
**Risk**: Low for the proxy itself. Risk is to the upstream connection: if
  system root certs are compromised or missing, upstream connections could be
  intercepted or fail.

---

## Priority Surfaces for Adversarial Analysis

The following entry points are ranked by the combination of attacker control
and logic impact — where untrusted input most directly influences a decision or
construction:

### P1. Request URI Path + Query → Upstream URL Construction

**File**: `src/proxy/handler.rs:206-216` (`build_upstream_uri`)
**Why**: Client-controlled path and query are concatenated verbatim into the
upstream URL with no sanitization. This is the #1 spot for finding real
vulnerabilities. Specific patterns to test:
- Path traversal: `/../../../etc/passwd`
- CRLF injection in query strings: `?x=%0d%0aInjected-Header:%20value`
- Double-encoding: `/%252e%252e%252f`
- Null bytes: `/%00`
- Unicode normalization: `/Ｎ/` (fullwidth Latin) vs `/N/`
- Path confusion between proxy and upstream: `/path/..%2f..%2fsecret`

### P2. Host Header in HTTP→HTTPS Redirect

**File**: `src/tls/redirect.rs:41-69`
**Why**: Client-controlled Host header is interpolated into the Location response
header. Classic header injection / open redirect vector. `HeaderValue::from_str()`
is the primary defense but should be explicitly tested for CRLF bypass.

### P3. All Client Headers Forwarded to Upstream

**File**: `src/proxy/handler.rs:223-225`
**Why**: Only hop-by-hop headers are stripped. All other headers pass through
unchecked. Request smuggling conditions arise when the proxy and upstream
disagree on header parsing. Specific patterns:
- Duplicate `Content-Length` headers
- `Transfer-Encoding: chunked` with HTTP/1.0
- `Transfer-Encoding` obfuscation (e.g., `chunked, identity`)
- `Content-Length` and `Transfer-Encoding` disagreement

### P4. Config File Read on Reload

**File**: `src/shutdown.rs:88`, `src/admin/socket.rs:257`
**Why**: Config is re-read from disk on SIGHUP and admin reload. TOCTOU between
read and use. If config is compromised (via a separate vulnerability or
misconfiguration), the proxy will happily apply attacker-controlled upstream
addresses, creating an effective SSRF.

### P5. ACME Directory URL

**File**: `src/tls/acme.rs:29-33`
**Why**: Arbitrary URL from config used as ACME endpoint without format
validation. If config is compromised, certificate requests go to an
attacker-controlled server.

---

## Surfaces Where Rust's Memory Model Eliminates Entire Bug Classes

These are patterns that would be high-priority in C/C++ but are automatically
safe in Rust:

- **Buffer overflows** in URI parsing, header parsing, body reading — hyper
  and axum handle all buffer management safely
- **Use-after-free** in connection handling — Rust's ownership model prevents
  dangling references to connection state
- **Integer overflow** in Content-Length parsing — `u64::parse` and `u32::parse`
  are checked operations
- **Null pointer dereference** in config access — `Option` and `Result` enforce
  explicit handling
- **Race conditions** in config reload — `ArcSwap` provides lock-free atomic
  swaps; `Mutex` serializes concurrent reloads

The remaining risk surface is **logic bugs in the glue** — incorrect
construction, insufficient validation, or wrong assumptions about what the
battle-tested components (hyper, rustls, tokio) guarantee.