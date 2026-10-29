---
status: open
last_updated: 2026-10-29
reviewed_code:
  - src/proxy/handler.rs
  - src/server.rs
  - src/config/static_config.rs
  - deploy/fail2ban/filter.d/reverse-proxy-badbots.conf
reviewer: code-reviewer
based_on: Production incident 2026-10-29 (fail2ban self-ban via /-/ws retry storm); dev1 system docs dev1/fail2ban.md, dev1/gitea.md
trigger: Gitea UI websocket endpoint degrades to 405/426 through the proxy; a middle-box retry storm on the operator's new ISP path trip-hacked the badbots fail2ban jail and banned the operator's own home network
---

# Operational Review #011 — Websocket Upgrade Not Supported (Gitea `/-/ws` Fallback, Polling, Self-Ban Incident)

## Purpose

On 2026-10-29 the operator's home network (`<operator home /24>`, new ISP after a
move) was banned by the `reverse-proxy-badbots` fail2ban jail on dev1. The
banning traffic was not hostile: it was the operator's own open Gitea tab
retrying Gitea's websocket endpoint, `/-/ws`, which the proxy cannot
upgrade. The incident exposed the gap this review documents: **the proxy has
no websocket (or any 101-upgrade) passthrough**, which (a) silently degrades
every Gitea session to HTTP polling and (b) generates a 405/426 noise floor
that user-side middle-boxes can amplify into fail2ban false positives.

Related deploy-side work already completed outside this repo on 2026-10-29:

- dev1's live `reverse-proxy-badbots` jail got a negative lookahead
  `(?!path=/-/ws\b)` on the connect-line so Gitea websocket retries can never
  trip it again (verified with `fail2ban-regex` in both directions:
  `CONNECT /-/ws status=405` → 0 matches; `CONNECT /proxy.php status=404` → 1
  match — real scanner CONNECTs still ban).
- `<operator home /24>` added to jail `ignoreip` (stale old-ISP
  `<previous home /24>` removed; backup of the pre-fix filter at
  `/tmp/badbots.conf.bak-20261029` on dev1).

The deploy-side incident write-up lives in the system repo at
`/workspace/system/dev1/fail2ban.md` ("Known trap: logged-in browsers trip
badbots via /-/ws"); the filter fix itself is synced to
`deploy/fail2ban/filter.d/reverse-proxy-badbots.conf` in this commit.

Findings are grouped as in prior reviews:
- **Critical** — the functional gap itself
- **Warning** — operational interactions that produce incidents
- **Suggestion** — tests, config, observability for the fix

---

## Evidence (production access log, 2026-10-29)

Gitea's UI, after a failed websocket handshake, degrades to a polling loop:
`GET /user/stopwatches` + `GET /notifications/new` every ~10s (all 200 OK —
the UI works, only live notification push is lost).

The websocket attempts themselves:

```
2026-10-29T03:47:44Z ... client_ip=<operator home IP> method=CONNECT path=/-/ws status=405 upstream=127.0.0.1:3000 duration_ms=2
2026-10-29T03:48:18Z ... client_ip=<operator home IP> method=CONNECT path=/-/ws status=405 upstream=127.0.0.1:3000 duration_ms=1
GET /-/ws → status=426 (Upgrade Required)
```

Historical rate on the operator's previous ISP: 22 `CONNECT /-/ws` 405s
spread over six+ weeks (below fail2ban's maxretry) — the UI effectively polls
and retries the websocket only occasionally. Under the new ISP's network
path, retries fired every few seconds, crossing the badbots jail maxretry
within minutes.

---

## Critical Findings

### C1. Upgrade requests are forwarded through a pooled hyper-util client that cannot complete a 101 handshake [new code required]

**Location**: `src/proxy/handler.rs:25-26` (client fields),
`src/proxy/handler.rs:206-250` (`build_upstream_request` header copy),
`src/proxy/handler.rs:107-112` (upstream send via pooled client)

The forward path builds an upstream request with `Request::builder()...`
copies **all** client headers (`Connection`, `Upgrade` included — they are
hop-by-hop) and sends it through the pooled `hyper_util::client::legacy`
clients (`Client<HttpConnector, Body>` / rustls-wrapped). This path:

1. Does not guarantee `Connection: Upgrade`/`Upgrade: websocket` survive
   hyper's hop-by-hop handling to the upstream — and even when they do, a
   **101 Switching Protocols response cannot be spliced back** through the
   pooled-connection plumbing: the caller has no `OnUpgrade` handle and no
   bidirectional copy between the client-side (axum/hyper server, served via
   `serve_connection_with_upgrades`, `src/server.rs:494`) and the
   upstream-side response.
2. So upstream Gitea receives a plain request for `/-/ws` with no upgrade →
   answers **426 Upgrade Required** (GET path) and the browser never gets a
   live socket. For `CONNECT`, upstream's router answers **405**.

Observed end state: every session falls back to the ~10s polling loop
described above. Gitea itself fully supports the endpoint; the proxy is the
bottleneck.

**Recommended fix** — one of:

- **Option A (hyper plumbing)**: for requests bearing an `Upgrade` header,
  bypass the pooled client: open a dedicated upstream stream
  (`HttpConnector` per connection or hyper's low-level send with
  `OnUpgrade` on the response, `hyper::upgrade::on()` server-side),
  complete the 101 handshake, then `tokio::io::copy_bidirectional()` between
  the client-side upgraded stream and the upstream stream. Requires passing
  the upgraded *server-side* stream into the handler — `axum`/`hyper`
  `on()` at the server layer, not just the response body.
- **Option B (TCP-level splice, simpler + robust)**: on the TLS-terminated
  accept path, peek the request head before handing the connection to
  hyper; if it is an upgrade request, forward the (rewritten-host) head to a
  fresh upstream TCP connection, forward the upstream's response head, then
  `tokio::io::copy_bidirectional` raw streams for the lifetime of the
  connection. Bypasses all hyper/axum upgrade machinery entirely; the
  handshake and every subsequent frame are preserved byte-for-byte.
  Interacts with C2 below (h2 vs h1 detection) since peeking is only
  meaningful on h1.

Both options must preserve:

- the rewritten `Host` header (existing logic, `build_upstream_request`)
- read/write byte limits are **not** the same as the body-`limit_bytes`
  config — the splice must not be subject to the 100 MiB request-body cap
- graceful shutdown: upgraded streams should be excluded from (or drained
  on) `InFlightCounter` shutdown with a bounded grace period; long-lived
  websockets make "wait for all in-flight connections" hang the drain
  without a cap

**Severity rationale**: feature is fully broken today (polling fallback for
every user), and the 405/426 noise it generates is the fuel for
self-ban-class incidents (see W1). It is not currently an outage vector, but
the combination with user-side middle-boxes makes it user-impacting in the
field.

---

### C2. HTTP/2 websocket (RFC 8441 / extended CONNECT) is half-wired [new code required]

**Location**: `src/server.rs:464,491` (`enable_connect_protocol()` on both
builders)

Both builders enable `connect_protocol`, which is necessary for
HTTP/2 extended CONNECT (`:protocol: websocket`) — but no handler path
supports completing it (same gap as C1, plus the h2 variant). Browsers that
negotiate HTTP/2 with ALPN (the common case for this TLS-terminating proxy)
use the extended-CONNECT form for `wss://`; they do not fall back to h1 for
websocket attempts on an h2 connection. Result: for upgraded sessions the
browser can never complete the handshake regardless of C1's h1 outcome,
until either:

- the h2 path implements extended-CONNECT upgrade splicing, or
- the proxy forces websocket-bound requests onto h1 (not generally possible
  once ALPN negotiated h2 for the client connection), or
- `/-/ws` requests get answered with a fast, clean h2 `RST`-style refusal so
  the client application layer gives up retrying quickly (mitigation only).

Note the interplay with review #007 C1 (now implemented
`connection_idle_timeout_secs` / keep-alive): extended-CONNECT streams are
long-lived; h2 keep-alive settings must not reap them. Fold this into the
fix (see W2).

---

## Warning Findings

### W1. fail2ban badbots jail treats the proxy's own 405s as bot traffic — self-ban path [deploy existing, synced 2026-10-29]

**Location**: `deploy/fail2ban/filter.d/reverse-proxy-badbots.conf:3`

The connect-line failregex counted **any** `CONNECT`/`PROPFIND` regardless of
path. Gitea clients send `CONNECT /-/ws` (retry) which the proxy answers 405
via upstream; a client-side middle-box that retries aggressively (observed
on the operator's new ISP) crosses maxretry and gets the *logged-in
legitimate user* banned by their own browser tab.

**Fix (shipped 2026-10-29)**: negative lookahead
`method=(PROPFIND|CONNECT) (?!path=/-/ws\b)[^ ]* status=\d+` — synced from
the dev1 live file into this repo (this commit). Keep the two in sync in
future deployment procedure; treat any new "garbage method" pattern with the
same care (add path-based exclusions for endpoints real clients legitimately
touch).

### W2. Upgraded connections vs idle/keep-alive timeouts and FD budget [verify during fix]

**Locations**: `src/config/static_config.rs` (`connection_idle_timeout_secs`,
`max_connections`), `src/server.rs` (builders), `src/config/fd_budget.rs`

Once C1/C2 land, every long-lived websocket is a permanently-open
connection. Two constraints:

- keep-alive / idle timeouts introduced by review #007 C1 must be verified to
  not reap upgraded streams (h1: after 101, hyper is out of the loop — fine
  via Option B or plain upgrade splicing; h2: extended-CONNECT streams are
  hyper-managed — confirm keep_alive_interval/timeout semantics)
- `max_connections` + `fd_budget` budget each WS as a full connection slot;
  a browser with many tabs (or an open wiki page per LLM worker — this box
  serves per-LLM accounts that poll constantly) can pin slots indefinitely.
  Bump the default budget accounting or track upgraded connections under a
  separate cap.

### W3. Gitea UI polling loop is per-tab and chatty enough to matter [ops note]

`GET /user/stopwatches`/`/notifications/new` every ~10s per open tab, all the
way through the stack (proxy → gitea → Postgres). With per-LLM accounts
(operators + agents) driving sessions in parallel, the polling loop is
non-trivial background load. This is the same order of magnitude as the
bot-render burn documented in the crawler blocklist work (2026-10-25,
dev1/firewall.md) and is the strongest argument for C1: websockets replace
the polling loop entirely.

---

## Suggestion Findings

### S1. Integration tests for upgrade passthrough [new code required, with C1]

Cover at minimum:
1. h1 `Connection: Upgrade` → upstream 101 → bidirectional echo through the
   proxy (use a tiny local WS echo server; `websocat` or `tokio-tungstenite`
   in tests)
2. mid-stream teardown (client or upstream drops) releases FDs (guards the
   review #007 C1 family of regressions)
3. upgrade request to a dead upstream → 502, client socket closes cleanly
4. h2 client negotiating websockets: either full RFC 8441 support verified
   with a h2 client, or a fast clean refusal (never a hang)
5. fail2ban filter file test: `CONNECT /-/ws ` never matches; scanner
   CONNECTs still match (mirror the dev1 fail2ban-regex checks)

### S2. Config toggle + log handling for upgrade traffic [config, with C1]

Add `enable_websockets` (default true once C1 lands) so operators can fall
back to current behavior if a regression ships. While WS is unsupported,
`CONNECT` requests currently log at INFO with tiny durations — fine — but
retry storms of hundreds of lines/min suggest a WARN with coalescing once
WS support exists, since a working WS makes `CONNECT` noise vanish instead.

### S3. Observability: count upgraded vs polling sessions [nice to have]

Expose `upgraded_connections` alongside `in_flight_connections`
([review #007 S1](007-connection-lifecycle-and-deployment-drift.md) family)
so the polling-loop share of sessions is visible and the C1 payoff can be
measured after rollout.

---

## Recommended Fix Sequence

1. Land C1 (Option B splice preferred for robustness + minimal plumbing), with S1
2. Verify W2 (timeouts, FD budget accounting with long-lived connections)
3. Decide C2: implement RFC 8441 or ship fast-refusal + document browsers-on-h2 behavior
4. Ship image + config (`enable_websockets = true`) — deployment procedure follows the standard one (review #007's migration list applies: admin key, logrotate postrotate/USR1, config fields)
5. Verify in production: `wss://git.alk.dev/-/ws` completes via browser; Gitea UI stops the 10s polling loop; no `/-/ws` 405s in `/var/log/reverse-proxy/access.log`; fail2ban jails stable over 24h
6. Keep the 2026-10-29 badbots lookahead in place after WS support (defense in depth against middle-box weirdness)

## References

- [Review #007](007-connection-lifecycle-and-deployment-drift.md) — idle/concurrency caps this interacts with (C1/C2 there)
- [Review #009](009-fix-never-deployed-and-streaming-body-bug.md) — streaming-body bug family; the 100 MiB body cap must not apply to spliced upgrade streams
- dev1 docs — `/workspace/system/dev1/fail2ban.md` (2026-10-29 incident note + filter backup path), `dev1/gitea.md`
- RFC 7230 §6.1 (Connection/Upgrade hop-by-hop), RFC 8441 (Bootstrapping WebSockets with HTTP/2)