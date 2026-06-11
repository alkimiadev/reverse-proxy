---
status: draft
last_updated: 2026-06-11
reviewed_documents:
  - overview.md
  - proxy.md
  - tls.md
  - config.md
  - operations.md
  - decisions/001 through 020
  - open-questions.md
reviewer: architect
---

# Architecture Gap Review #001

## Purpose

This document captures gaps, ambiguities, and inconsistencies found in the
architecture specifications before decomposition into implementation tasks. Each
finding is categorized by severity and structured as **Problem** →
**Suggestion** (or **Open Question** where no solution is yet known).

## Severity Definitions

| Severity | Meaning |
|----------|---------|
| **Critical** | Must resolve before implementation — would cause divergent implementations or security issues |
| **Warning** | Should resolve — missing specifications that could cause confusion or rework |
| **Suggestion** | Consider — improvements for clarity or completeness |

---

## Critical Findings

### C1. Per-Listener vs. Global Site Routing Contradiction

**Documents**: config.md (lines 144–148), proxy.md (lines 76–83), config.md validation rule 7 (line 307)

**Problem**: The spec contains contradictory statements about site routing:

- config.md says "Each listener routes its own sites independently" (per-listener
  routing)
- config.md also says "The `DynamicConfig` collects all sites across all
  listeners for hot-reload via `ArcSwap`" (global collection)
- Validation rule 7 says "Site `host` values must be unique across all listeners
  (no duplicate hostnames, even across different listeners). Duplicate hostnames
  would create ambiguous routing — the proxy would not know which listener's
  upstream to route a request to when the `Host` header matches multiple sites."

The uniqueness constraint only makes sense if site lookup is global. If routing
is per-listener, the same hostname on different listeners is not ambiguous —
each listener only routes to its own sites. These two statements cannot both be
true simultaneously.

**Suggestion**: Adopt **global routing** as the model. Rationale:

- ArcSwap atomically swaps all routing at once — per-listener routers that each
  read their own slice would need coordination
- Rate limiting and body limiting are global, so a global site lookup is already
  happening
- Per-listener uniqueness constraint simplifies matching: one Host → one
  upstream, always
- This matches how nginx `server_name` works — Host header always resolves to
  one backend

Relevant spec changes:
- config.md: rewrite the sites description to say "Sites are defined per-listener
  in config but collected into a single global routing table in `DynamicConfig`.
  Hostnames must be unique across all listeners."
- proxy.md: clarify that host-based routing uses a global lookup across all sites
- overview.md architecture diagram: show that all sites are in one `DynamicConfig`
  accessible to all listeners
- The per-listener `[[listeners.sites]]` in TOML is purely a config organization
  concern — the runtime behavior is global routing

---

### C2. X-Forwarded-For Security Model Is Underspecified

**Documents**: proxy.md (lines 87–98)

**Problem**: The spec says X-Forwarded-For should "append the client IP to any
existing value" to "support chained proxies correctly." But this creates a
security vulnerability: any client can inject a fake `X-Forwarded-For` header,
and the proxy will append to it, making the header unreliable for upstream
services.

The spec doesn't define:
- Whether the proxy should **trust** existing X-Forwarded-For headers
- Whether there's a concept of "trusted proxies" whose X-Forwarded-For values
  should be preserved
- What IP is used as the "client IP" — is it always `ConnectInfo<SocketAddr>`
  or does it consider X-Forwarded-For from trusted proxies?

For a security-focused reverse proxy (the entire motivation is replacing nginx
due to CVEs), this is a critical gap.

**Suggestion**: For Phase 1, since the proxy is the **outermost proxy** directly
facing the internet, there are no trusted proxies in front of it. Therefore:

- **X-Forwarded-For**: Set to the client's IP from `ConnectInfo<SocketAddr>`.
  Replace any existing value rather than appending. A client connecting directly
  to the proxy has no legitimate X-Forwarded-For chain.
- **X-Real-IP**: Set to the client's IP from `ConnectInfo<SocketAddr>`.
- Document this as a Phase 1 decision: the proxy is an edge proxy, not a
  middle proxy in a chain. If chained proxy support is needed in the future,
  a "trusted proxies" configuration can be added.

This should be documented as a new ADR or added to the proxy.md spec.

---

### C3. Request Header Forwarding Rules Are Incomplete

**Documents**: proxy.md (lines 100–109)

**Problem**: The spec says "Copy the request method, headers, and body from the
original" but doesn't specify which headers should be **removed** before
forwarding. HTTP reverse proxies must handle hop-by-hop headers (per RFC 2616
§13.5.1): `Connection`, `Keep-Alive`, `Transfer-Encoding`, `TE`, `Trailers`,
`Upgrade`, and `Proxy-*` headers. Blindly copying all headers can cause:

- Connection header conflicts between client and upstream
- Transfer-Encoding mismatches (especially with chunked encoding)
- Security issues from Proxy-Authorization leakage

The spec also doesn't specify whether the proxy adds a `Via` header (RFC 7230
§5.7.1) or a `Server` header on responses.

**Suggestion**: Add a "Header Handling" section to proxy.md that explicitly
lists:

**Headers to remove before forwarding (hop-by-hop):**
- `Connection`
- `Keep-Alive`
- `Proxy-Authorization`
- `Proxy-Authenticate`
- `TE`
- `Trailers`
- `Transfer-Encoding`
- `Upgrade`

**Headers to add/modify:**
- `X-Real-IP`: Set to client IP from `ConnectInfo<SocketAddr>`
- `X-Forwarded-For`: Set to client IP (replacing existing — see C2)
- `X-Forwarded-Proto`: Set to `https` or `http` based on listener port
- `Host`: Preserved as-is from original request

**Headers NOT added in Phase 1:**
- `Via`: Not added. The proxy is an edge proxy and `Via` is primarily for
  tracking proxy chains. Can be added in Phase 2 if needed.

**Response headers:**
- Upstream response headers are forwarded as-is with the exception of
  hop-by-hop headers listed above.
- The proxy does not add a `Server` header to responses.

---

### C4. ACME Startup Failure Behavior Is Undefined

**Documents**: tls.md (lines 66–109)

**Problem**: The spec describes ACME certificate provisioning in detail but never
specifies what happens when ACME provisioning fails. Key undefined scenarios:

- **First start, no cached cert, ACME unreachable**: The proxy cannot serve TLS
  at all. Does it fail to start? Start without TLS? Start and serve TLS errors?
- **First start, no cached cert, ACME succeeds**: Normal case, already described.
- **Start with cached cert, ACME unreachable for renewal**: The proxy has a
  valid cert but can't renew. Does it start? Log errors? Retry?
- **Renewal failure after startup**: The cert is near expiry and renewal fails.
  What happens?

Let's Encrypt unavailability is a real operational scenario (network outages,
rate limiting, DNS issues).

**Suggestion**: Define ACME failure behavior explicitly:

| Scenario | Behavior |
|----------|----------|
| First start, no cached cert, ACME fails | **Fail to start** with clear error message. Proxy cannot serve TLS without a certificate. |
| Start with cached cert, ACME renewal fails | **Start normally** with cached cert. Log error at `warn` level. `rustls-acme` retries per its built-in schedule. |
| Renewal failure after startup | **Continue serving existing cert**. Log error at `warn` level. `rustls-acme` retries per its built-in schedule. |
| Cached cert expired, renewal fails | **Fail to start** if cert is expired at startup. If cert expires during runtime, continue serving it (clients will receive certificate errors — this is the correct behavior, not silently dropping TLS). |

Add a "Certificate Failure Behavior" section to tls.md documenting these
scenarios. The key principle: **never start without a valid TLS certificate**,
but **always continue serving if a valid cert exists**, even if renewal fails.

---

### C5. Startup Sequence and Partial Failure Not Specified

**Documents**: overview.md (architecture diagram), operations.md (systemd integration), config.md (validation)

**Problem**: The startup sequence is not documented anywhere. With multiple
listeners, each with their own TLS configuration, there's a complex startup
dependency chain. Undefined scenarios:

- If listener 1 binds successfully but listener 2 fails (port in use), does the
  proxy partially start or fail entirely?
- If manual TLS cert files are missing, does startup fail before or after
  binding ports?
- What order do components start in? (health check port, admin socket, HTTP
  listeners, HTTPS listeners, ACME background tasks, rate limiter eviction task)
- The systemd unit uses `Type=notify` with `sd_notify`, but when exactly is
  "ready" signaled?

**Suggestion**: Add a "Startup Sequence" section to operations.md:

1. **Parse and validate config** — If validation fails, exit with non-zero code
   and log errors. No ports are bound.
2. **Initialize DynamicConfig** — Load sites, rate limits, body limits into
   ArcSwap.
3. **Initialize shared state** — Rate limiter HashMap, hyper Client, tracing
   subscriber.
4. **Bind health check port** (if enabled) — Fail-fast if bind fails.
5. **Bind admin socket** (if enabled) — Remove stale socket file first. Fail-fast
   if bind fails.
6. **Bind all listener ports** — For each listener: bind HTTP port, bind HTTPS
   port. If any bind fails, fail-fast and exit. All ports are bound before
   proceeding.
7. **Load TLS configuration** — For each listener: load manual certs or
   initialize ACME state machine. If manual cert loading fails, fail-fast and
   exit. For ACME: if no cached cert exists and ACME provisioning fails, fail-fast
   and exit (see C4).
8. **Start TCP listeners** — Begin accepting connections on all bound ports.
9. **Start background tasks** — ACME renewal tasks, rate limiter eviction task,
   signal handler task.
10. **Signal readiness** — `sd_notify("READY=1")` to systemd.

**Failure semantics**: **Fail-fast**. If any step fails, the process exits with
a non-zero code. The proxy does not partially start. All ports are bound before
any connections are accepted.

---

### C6. Shared vs. Per-Listener Router Architecture Is Ambiguous

**Documents**: tls.md (lines 38–50), proxy.md (lines 76–83), overview.md (lines 83–122)

**Problem**: The architecture diagram shows each listener having its own "axum
router (per-listener sites)." However, the `ArcSwap<DynamicConfig>` pattern
suggests a single global config that all routers read from. This raises an
unanswered design question: does each listener get its own `axum::Router`
instance, or do all listeners share a single `Router`?

This affects:
- Rate limiter state sharing
- Site routing lookup logic
- Middleware ordering
- How `ConnectInfo<SocketAddr>` is available for X-Real-IP injection

**Suggestion**: Each listener gets its own `axum::Router` instance. Rationale:

- The architecture diagram already shows this model
- Each listener has its own TCP stream context, making `ConnectInfo` per-listener
- Each listener can have its own middleware stack (e.g., different body limits
  in Phase 2)
- Shared state is achieved via `Arc<ArcSwap<DynamicConfig>>` and
  `Arc<Mutex<HashMap<IpAddr, TokenBucket>>>` passed as axum State

However, despite having per-listener Routers, **site routing is global** (see C1).
All routers read from the same `DynamicConfig` site table. Host uniqueness across
listeners ensures unambiguous routing.

Update the architecture diagram and proxy.md to clarify:
- Each listener has its own `axum::Router` instance
- All routers share `Arc<ArcSwap<DynamicConfig>>` and
  `Arc<Mutex<HashMap<IpAddr, TokenBucket>>>` via axum State
- Host-based routing uses a global lookup across all sites

---

### C7. Rate Limiter State Behavior on Config Reload Is Undefined

**Documents**: config.md (lines 155–168), operations.md (lines 30–48), ADR-008 (lines 38–39)

**Problem**: `DynamicConfig` includes `rate_limit.requests_per_second` and
`rate_limit.burst`, which are hot-reloadable via ArcSwap. The rate limiter uses
an in-memory `HashMap<IpAddr, TokenBucket>`. When the rate limit parameters
change (e.g., from 10 req/s burst 20 to 20 req/s burst 40):

- Do existing token buckets get their parameters updated immediately?
- Are existing buckets drained/reset to match the new burst capacity?
- Are they left unchanged with old parameters while only new IPs get new
  parameters?
- Is the entire HashMap cleared on reload (losing all state)?

This is not specified anywhere and directly affects production behavior. A
reload that clears all buckets creates a temporary rate-limiting gap; a reload
that doesn't update existing buckets creates inconsistent rate limiting.

**Suggestion**: On config reload, existing token buckets **adopt the new
rate/burst parameters on their next request**. The bucket's current token count
is capped at the new burst maximum. The HashMap is NOT cleared — this avoids
creating a rate-limiting gap. Specifically:

1. New `DynamicConfig` is swapped in via ArcSwap
2. On the next request from an existing IP, the rate limiter reads the current
   `DynamicConfig` for rate/burst parameters
3. The token bucket refills using the new rate, and its capacity is set to the
   new burst maximum
4. If the current token count exceeds the new burst maximum, it's capped to
   the new burst maximum

This means: no gap in rate limiting, no burst of uncontrolled traffic, and
parameters converge for all IPs within one request cycle.

Document this explicitly in operations.md under the "Rate Limiting" section.

---

## Warning Findings

### W1. Admin Socket Wire Protocol Is Underspecified

**Documents**: ADR-014 (lines 33–37), config.md (line 91), operations.md (lines 224–228)

**Problem**: ADR-014 specifies that the admin socket accepts `reload` and
`status` commands returning JSON, but doesn't define:
- Connection lifecycle: Is each command a new connection, or can multiple
  commands be sent on one connection?
- Message framing: How does the client know when the response is complete?
- Error cases: What happens with invalid commands, binary data, extremely long
  lines?
- Concurrency: Can multiple clients connect simultaneously?
- Socket cleanup: What happens if the proxy starts and a stale socket file
  exists from a previous crash?

**Suggestion**: Specify the protocol explicitly:

- **Connection lifecycle**: One command per connection. Client connects, sends
  one newline-terminated command, receives one newline-terminated JSON response,
  connection closes.
- **Message framing**: Newline-delimited (`\n`). The response ends with `\n`.
  Client reads until `\n` and then the connection is closed by the server.
- **Error responses**: Unrecognized commands return
  `{"status": "error", "message": "unknown command: <cmd>"}`. Invalid/empty
  input returns `{"status": "error", "message": "invalid input"}`.
- **Concurrency**: Multiple clients can connect simultaneously. Commands are
  serialized (see W6 — reload serialization).
- **Socket cleanup**: The proxy removes any existing socket file at startup
  before binding. If the file exists and another process is listening, the
  proxy logs a warning and fails to start the admin socket (but continues
  running — admin socket is non-critical).

Add this to operations.md in the "Admin Socket" section.

---

### W2. Host Header Port Disambiguation

**Documents**: proxy.md (lines 76–83), config.md (line 138)

**Problem**: The spec says routing uses axum's `Host` extractor and site `host`
values are hostnames like `"git.alk.dev"`. But the HTTP `Host` header can include
a port (e.g., `git.alk.dev:443`). The spec doesn't specify:

- Whether the Host header port is stripped before matching
- Whether site `host` values should include ports
- What happens when a request arrives with `Host: 203.0.113.10` (IP address)

**Suggestion**:
- Host header matching **strips the port** before comparison. The `Host`
  extractor in axum already provides the hostname without port.
- Site `host` values must **not** include ports. Add a validation rule that
  `host` values are valid hostnames (not IP addresses, not including ports,
  lowercase only — matching the case-insensitive rule in S3).
- Requests with an IP address as the Host header (no matching site) receive a
  404 response, same as any unknown host.
- Add this to config.md validation rules and proxy.md request flow.

---

### W3. HTTP Redirect Host Header Handling

**Documents**: proxy.md (lines 134–138)

**Problem**: The spec says the HTTP→HTTPS redirect "reads the `Host` header from
the incoming request and returns a 301 Permanent Redirect to the HTTPS
equivalent URL (preserving the path and query string)." But:

- What if the Host header includes a non-standard port (e.g.,
  `Host: example.com:8080`)?
- Should the redirect use the listener's `https_port` if it's not 443?
- What if the incoming request has no Host header (HTTP/1.0)?

**Suggestion**:
- The redirect URL is constructed as:
  `https://{host}:{https_port}/{path}?{query}`
  where the port is omitted if it's 443 (the default HTTPS port).
- If the incoming Host header includes a port, only the hostname portion is
  used in the redirect URL. The `https_port` from the listener config is used.
- If the incoming request has no Host header, return `400 Bad Request`.
- Add this to proxy.md under "HTTP → HTTPS Redirect."

---

### W4. Health Check HTTPS Endpoint Routing

**Documents**: operations.md (lines 146–161), proxy.md (lines 28–29)

**Problem**: Operations.md says `/health` is "also available on the main HTTPS
listener" but doesn't specify how this is implemented in the routing. The
proxy.md architecture diagram shows `/health → 200 OK` on the axum Router, but
this router does host-based routing. If `/health` is on the same router, it
needs to be matched before host-based routing, or it needs to be available on
any Host header.

**Suggestion**: Specify that `/health` on the HTTPS listener is a **top-level
route** that matches regardless of Host header. It is evaluated before host-based
routing. A request to `GET /health` on any hostname returns `200 OK`.

This means the axum Router has two layers:
1. Path-based routes: `/health` → 200 OK (regardless of Host)
2. Host-based routes: everything else → site lookup → proxy or 404

Add this to proxy.md under "Host-Based Routing."

---

### W5. Config Reload Doesn't Specify Static Config Change Handling

**Documents**: config.md (lines 155–168), ADR-008

**Problem**: The config file contains both StaticConfig and DynamicConfig. If
someone changes a static config field (like `bind_addr` or TLS mode) and sends
SIGHUP:
- Is the static config portion silently ignored?
- Is a warning logged?
- Is the entire reload rejected?

The spec says "Changes [to StaticConfig] require a process restart" but
doesn't specify what happens when the config file contains both static and
dynamic changes during a reload.

**Suggestion**: On reload:

1. Read the entire config file from disk
2. Validate the **full** config (static + dynamic) for syntax and structural
   correctness
3. If the full config fails validation, reject the reload and log the error.
   The old DynamicConfig remains active.
4. If the full config passes validation, compare the static portion against the
   currently running StaticConfig. If any static fields have changed, log a
   warning listing the changed fields and noting that a restart is required for
   those changes to take effect.
5. Swap the DynamicConfig via ArcSwap.

This gives operators early feedback about config drift (warning on static changes)
while still applying dynamic changes immediately. Add this to config.md under
"Config Reload."

---

### W6. Config Reload Race Condition With Simultaneous SIGHUP and Admin Socket

**Documents**: config.md (lines 173–184), operations.md (lines 209–227)

**Problem**: Both SIGHUP and the admin socket `reload` command converge on the
same code path. If a SIGHUP and an admin socket `reload` arrive nearly
simultaneously, two reloads could be in progress at the same time. Without
serialization, concurrent file reads and ArcSwap operations could race,
potentially:
- Applying an older config over a newer one
- Causing confusing log output (two CONFIG_RELOAD events for the same config)
- Admin socket returning stale status

**Suggestion**: Serialize reload operations using a `tokio::sync::Mutex` on the
reload path. This ensures only one reload is in progress at a time. If a
second reload is requested while one is in progress, it waits for the first to
complete, then re-reads the file (getting the latest version) and proceeds.

Document this in config.md under "Config Reload" and note it in ADR-008 or
ADR-014.

---

### W7. Missing Validation: http_port Conflicts and Port Rules

**Documents**: config.md (lines 296–314)

**Problem**: Validation rule 3 states "Each listener's `bind_addr` and
`https_port` combination must be unique." But:

- `http_port` conflicts are not checked (two listeners with the same
  `bind_addr` and same `http_port` would fail at bind time)
- `http_port` and `https_port` could conflict within the same listener
  (e.g., both set to 443)
- `https_port` must be required (cannot be 0) since TLS requires a port
- `http_port` can be 0 (disabled) but must be a valid port otherwise

**Suggestion**: Add these validation rules:

| Rule | Description |
|------|-------------|
| Each `bind_addr` + `http_port` combination must be unique | Prevents bind-time errors |
| Within a listener, `http_port` ≠ `https_port` | Prevents same-port conflict |
| `https_port` must be 1–65535 | TLS requires a port |
| `http_port` must be 0 (disabled) or 1–65535 | Valid port range or explicitly disabled |
| `health_check_port` must not conflict with any listener port | Prevents bind conflict |

Update config.md validation section with these rules.

---

### W8. Missing Validation: `upstream` Field Format

**Documents**: config.md (lines 139–143)

**Problem**: The `upstream` field is described as supporting Docker DNS,
loopback, LAN IPs, and tunnel endpoints, but there's no formal validation rule
for what format it accepts. Ambiguities:

- A hostname without a port? (`gitea` — what port?)
- A URL with a scheme? (`http://gitea:3000` — conflicting with `upstream_scheme`)
- Just an IP address? (`10.0.0.5` — what port?)
- An IPv6 address? (`[::1]:3000`)

Since `upstream_scheme` is a separate field, `upstream` should be `host:port`
without scheme, but this isn't explicitly validated.

**Suggestion**: Add validation rules:

- `upstream` must be in `host:port` format where `host` is a valid DNS name or
  IP address (IPv4 or IPv6 in bracket notation), and `port` is a required
  integer 1–65535
- The `upstream_scheme` field handles the protocol — `upstream` must NOT include
  a scheme prefix (no `http://` or `https://`)
- Examples of valid values: `gitea:3000`, `127.0.0.1:3000`, `[::1]:3000`
- Examples of invalid values: `gitea` (missing port), `http://gitea:3000`
  (includes scheme), `10.0.0.5` (missing port)

Update config.md validation section with these rules.

---

### W9. TLS Handshake Failure Behavior Is Underspecified

**Documents**: tls.md

**Problem**: The spec details certificate provisioning and SNI selection but
never describes what happens when a TLS handshake fails. Possible failure
scenarios:

- Client sends an SNI hostname that doesn't match any certificate (manual mode)
- Client doesn't send SNI at all
- TLS protocol version is too old (TLS 1.0/1.1)
- Cipher suite negotiation fails
- Certificate has expired (manual mode)

**Suggestion**: Add a "TLS Error Handling" section to tls.md:

| Scenario | Behavior |
|----------|----------|
| SNI hostname doesn't match any certificate (manual mode) | TLS handshake fails with `unrecognized_name` alert. Log at `warn` level with client IP and SNI hostname. |
| No SNI extension sent by client | TLS handshake fails with `handshake_failure` alert. Log at `warn` level with client IP. |
| Unsupported TLS version (1.0/1.1) | TLS handshake fails with `protocol_version` alert. Log at `info` level. |
| Cipher suite negotiation fails | TLS handshake fails with `handshake_failure` alert. Log at `info` level with client IP. |
| Certificate expired (manual mode) | Connection fails. Log at `error` level. Proxy continues serving other connections. |
| ACME renewal failure | Continue serving existing certificate. Log at `warn` level. Retry per rustls-acme schedule. |

---

### W10. IPv6 Rate Limiting Not Addressed

**Documents**: operations.md (lines 30–48), ADR-006

**Problem**: Rate limiting is described as "per IP" but doesn't address IPv6.
An attacker with a /64 IPv6 prefix has 2^64 unique addresses, completely
bypassing per-IP rate limiting.

**Suggestion**: Specify IPv6 rate limiting behavior:

- IPv4 addresses are rate-limited per individual address (`/32`)
- IPv6 addresses are rate-limited per `/64` prefix (per RFC 4941 and common
  practice). All addresses in the same `/64` share the same token bucket.
- The rate limiter stores `IpAddr` internally but normalizes IPv6 addresses to
  their `/64` prefix before lookup.

Add this to operations.md under "Rate Limiting" and note it in ADR-006.

---

### W11. Graceful Shutdown Doesn't Specify Connection Draining Behavior

**Documents**: operations.md (lines 205–234), ADR-009

**Problem**: The graceful shutdown description says "stop accepting new
connections, wait up to 30 seconds for in-flight requests to complete" but
doesn't specify:

- Does "stop accepting" mean closing the listening socket, or rejecting new
  connections?
- What happens to idle keep-alive connections?
- After the timeout, how are remaining connections terminated?
- What happens to ACME background tasks?

**Suggestion**: Specify the shutdown sequence explicitly:

1. **Signal received** (SIGTERM/SIGINT)
2. **Stop accepting new connections** — Close all listening sockets. No new
   connections are accepted.
3. **Close idle keep-alive connections** — Send `Connection: close` header on
   any idle connections in the keep-alive pool.
4. **Wait for in-flight requests** — Up to `shutdown_timeout_secs` (default 30)
   for active requests to complete.
5. **Force-close remaining connections** — After timeout, TCP RST on any
   remaining connections.
6. **Cancel background tasks** — ACME renewal tasks, rate limiter eviction
   task, admin socket listener are all cancelled.
7. **Exit with code 0**

Add this to operations.md under "Graceful Shutdown."

---

### W12. Response Error Body Content Is Not Specified

**Documents**: proxy.md (lines 122–131)

**Problem**: The error handling table specifies HTTP status codes (502, 504, 413,
429, 404) but doesn't specify the response body content. For a security-focused
reverse proxy, the error response body matters:

- Should error responses include HTML, plain text, or JSON?
- Should they include the proxy's version/identity?
- Should they include upstream error details?

**Suggestion**: Specify that error responses are minimal plain text, following
this format:

- Response body: Plain text with a brief message (e.g., `Bad Gateway`, `Not
  Found`)
- No proxy version or identity information
- No upstream error details
- Content-Type: `text/plain; charset=utf-8`

Example error responses:
- 404: `Not Found`
- 413: `Payload Too Large`
- 429: `Too Many Requests`
- 502: `Bad Gateway`
- 504: `Gateway Timeout`

Add a "Error Responses" section to proxy.md.

---

### W13. `upstream_scheme` HTTPS Upstream TLS Behavior

**Documents**: config.md (line 140), ADR-017

**Problem**: `upstream_scheme` accepts `"http"` or `"https"`, but when
`upstream_scheme = "https"`, the spec doesn't specify how the proxy validates
the upstream's TLS certificate. Does it trust all certs? Use the system CA
bundle? Pin specific CAs?

If the proxy trusts all upstream certs, it negates the security benefits of TLS
termination.

**Suggestion**: When `upstream_scheme = "https"`:

- The proxy uses the system's native TLS root certificates (via `rustls`
  root cert store) to validate the upstream's certificate.
- No certificate pinning or custom CA support in Phase 1.
- Certificate validation failures result in a 502 Bad Gateway response.
- `upstream_scheme` values are case-sensitive (lowercase only).

Add this to proxy.md under "Upstream Connection" and to config.md validation
rules.

---

### W14. `allow_wildcard_bind` CLI Flag vs Config File Precedence

**Documents**: config.md (line 299), ADR-016, ADR-020

**Problem**: The `allow_wildcard_bind` setting can be enabled either in the
config file or via CLI flag, but the merge semantics aren't specified. What
happens when:

- Config file has `allow_wildcard_bind = false` but CLI flag is
  `--allow-wildcard-bind`?
- Config file has `allow_wildcard_bind = true` but no CLI flag?
- Both are set?

**Suggestion**: CLI flags take precedence over config file settings. Specifically:
`allow_wildcard_bind` is `true` if EITHER the config file sets it to `true` OR
the `--allow-wildcard-bind` CLI flag is present. This is an OR relationship —
either source enables it.

Document this explicitly in config.md and ADR-016.

---

### W15. ADR-010 Phase 2 List Is Stale

**Documents**: ADR-010 (lines 51–53)

**Problem**: ADR-010 lists "Per-site upstream timeouts" as a Phase 2 feature,
but ADR-015 already moved per-site upstream timeouts into Phase 1. The Phase 2
list in ADR-010 is misleading for implementers.

**Suggestion**: Update ADR-010's Phase 2 list to remove "Per-site upstream
timeouts" and add a note that it was moved to Phase 1 via ADR-015.

---

### W16. Docker Compose Example Doesn't Show Multi-Listener Setup

**Documents**: operations.md (lines 326–379)

**Problem**: The Docker Compose example only publishes ports for a single
listener. The architecture supports multiple listeners on different IPs, but the
example doesn't demonstrate how to publish multiple listeners' ports.

**Suggestion**: Add a multi-listener Docker Compose example showing how to
publish ports for multiple listeners. Document that each listener's
`https_port` and `http_port` must be published separately.

---

### W17. LoggingConfig Is Static But `log_file_path` Might Need Runtime Changes

**Documents**: config.md (lines 95–101), ADR-008

**Problem**: The entire `LoggingConfig` including `log_file_path` is under
StaticConfig, meaning the log file path cannot be changed without a restart.
This isn't explicitly called out as a trade-off.

**Suggestion**: Add an explicit note in config.md that `LoggingConfig` (including
`log_file_path`) is static and requires a restart to change. Document the
rationale: log file path changes require reopening file handles, which is
complex and low-value for Phase 1. Log rotation (Phase 2) will be handled
differently (signal-based or built-in rotation).

---

## Suggestions

### S1. Add a Data Flow Diagram Showing Request Path With Data Structures

**Documents**: proxy.md

**Suggestion**: Add a more detailed data flow diagram showing:
- `DynamicConfig` → site lookup (how Host header maps to SiteConfig)
- SiteConfig → timeout selection (upstream_connect_timeout_secs,
  upstream_request_timeout_secs)
- How `hyper::Request` is constructed from the original request + proxy headers
- How the `hyper::Client` dispatches and streams the response back

This would help implementers understand state flow without guessing.

---

### S2. Document ConnectInfo Propagation

**Documents**: proxy.md (line 93)

**Suggestion**: Add a note explaining how `ConnectInfo<SocketAddr>` is populated.
With `tokio-rustls`, the proxy needs to extract the peer address from the
`TlsStream<TcpStream>` and inject it as `ConnectInfo` via axum's server
configuration. This is an implementation detail that should be noted since it
affects both the TLS layer and the proxy handler.

Specifically: when accepting a TLS connection, the proxy extracts
`TcpStream::peer_addr()` before wrapping in `TlsStream`, and provides it to
axum via `axum::ServiceExt::into_make_service_with_connect_info::<SocketAddr>()`.

---

### S3. Specify That Host Matching Is Case-Insensitive

**Documents**: proxy.md (lines 76–83), config.md (line 138)

**Suggestion**: RFC 7230 §2.7.3 specifies that host comparison should be
case-insensitive. The spec should explicitly state:

- Host matching is case-insensitive (`Git.Alk.Dev` matches `git.alk.dev`)
- `host` values in config are normalized to lowercase during validation
- The Host header is normalized to lowercase before matching

---

### S4. Consider Adding Request ID / Correlation ID

**Documents**: proxy.md, operations.md

**Suggestion**: The logging format includes `duration_ms` but no request ID.
Adding a `X-Request-Id` header (UUID) generated per request and included in both
the forwarded request and the access log would significantly improve operational
debugging. This is a common reverse proxy practice.

If this is deferred to Phase 2, note it explicitly as a Phase 2 candidate.

---

### S5. Clarify Response Streaming Behavior

**Documents**: proxy.md (line 69)

**Suggestion**: The spec says "Stream response back to the client" but doesn't
clarify:
- Are upstream response bodies streamed chunk-by-chunk, or buffered entirely?
- What happens if the client disconnects while the upstream is still sending?
- What happens if the upstream disconnects while streaming a response?

Recommend explicitly stating:
- Responses are streamed chunk-by-chunk (not buffered)
- If the client disconnects, the upstream connection is closed and the event
  is logged at `debug` level
- If the upstream disconnects mid-stream, the client receives whatever data
  was already sent and the connection is closed

---

### S6. Specify Rate Limit Bucket Semantics More Precisely

**Documents**: operations.md (lines 25–27), config.md (lines 130–132)

**Suggestion**: The spec says "10 requests/second with burst of 20" but doesn't
clarify exact token bucket semantics:
- When the bucket is empty and a request arrives: immediately rejected (429)
- The burst parameter: maximum bucket capacity (20 tokens). A burst of 20
  means 20 requests can be accepted in a rapid succession before rate limiting
  kicks in.
- `requests_per_second = 10` means 1 token is added every 100ms (not 10 tokens
  every 1 second)

These details affect how nginx `limit_req burst=20 nodelay` maps to the
implementation. The `nodelay` equivalent means requests are immediately rejected
when the bucket is empty — they are not queued.

---

### S7. Explicitly Note That Config File Watching Is Out of Scope

**Documents**: config.md (lines 173–184)

**Suggestion**: The spec uses SIGHUP and admin socket for reload but doesn't
mention file system watching (inotify/FSEvents). This is intentional — SIGHUP
and admin socket are sufficient. Add an explicit note in config.md that
automatic file watching (inotify, fsnotify, etc.) is out of scope for Phase 1.
Config reload is triggered explicitly by SIGHUP or admin socket command.

---

### S8. Specify That All Paths on Known Hosts Are Forwarded

**Documents**: proxy.md (lines 76–83)

**Suggestion**: The spec says the proxy "either proxies to the upstream or
returns 404" based on Host matching, but doesn't explicitly state that ALL paths
and query strings on a known host are forwarded to the upstream without
filtering. Make this explicit: "The proxy does not filter or restrict paths. All
paths and query strings on a known host are forwarded to the upstream."

---

### S9. Reference `shutdown_timeout_secs` in Shutdown Description

**Documents**: config.md (line 92), operations.md (lines 230–234)

**Suggestion**: operations.md says "wait up to 30 seconds" but the actual value
comes from `shutdown_timeout_secs` in StaticConfig. Make the connection
explicit: "wait up to `shutdown_timeout_secs` (default: 30s) for in-flight
requests to complete."

---

### S10. Add "Not in Phase 1" Sections

**Documents**: All documents

**Suggestion**: Several features are mentioned as out of scope (HTTP/2 proxying,
WebSocket, load balancing, per-site rate limits, per-site body limits), but
these are scattered. Add a consolidated "Not in Phase 1" section to each
document listing what is explicitly excluded, to prevent feature creep and
clarify scope for implementers.

---

### S11. Add a Consolidated Defaults Table for All Config Fields

**Documents**: config.md

**Suggestion**: While defaults are mentioned inline, a consolidated table of all
config fields with their types, defaults, and whether they're required would
reduce implementation ambiguity. Some fields have implied defaults that aren't
explicitly stated (e.g., what's the default for `acme_directory`? Is it
`"production"`? What happens if it's omitted?).

Create a table like:

| Field | Type | Default | Required |
|-------|------|---------|----------|
| `allow_wildcard_bind` | `bool` | `false` | No |
| `health_check_port` | `u16` | `9900` | No |
| `admin_socket_path` | `String` | `/run/reverse-proxy/admin.sock` | No |
| `listeners[].http_port` | `u16` | `80` | No |
| `listeners[].https_port` | `u16` | `443` | No |
| `listeners[].tls.acme_directory` | `"production"` or `"staging"` | `"production"` | No |
| ... | ... | ... | ... |

---

## Summary Statistics

| Severity | Count | Status |
|----------|-------|--------|
| Critical | 7 | Needs resolution before implementation |
| Warning | 17 | Should resolve — mostly quick spec clarifications |
| Suggestion | 11 | Consider for clarity |

## Recommended Resolution Order

1. **Critical findings C1–C7** — These involve design decisions that would
   cause divergent implementations. Resolve first.
2. **Warning findings W1–W17** — These are mostly spec clarifications that can
   be resolved quickly by adding text to existing documents.
3. **Suggestions S1–S11** — These improve clarity but won't block
   implementation.