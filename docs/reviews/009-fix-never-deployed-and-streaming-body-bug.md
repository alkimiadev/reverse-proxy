---
status: open
last_updated: 2026-08-19
reviewed_code:
  - src/server.rs
  - src/proxy/handler.rs
  - tests/integration_test.rs
  - /opt/reverse-proxy/reverse-proxy (deployed binary)
reviewer: code-reviewer
based_on: docs/reviews/008-http2-keepalive-defeats-idle-timeout.md
trigger: Post-deployment observation — 9 days after review #008 fix was committed, production still shows the same idle-FD leak (433/1024 FDs, growing)
fixes:
  - C2: FIXED in repo (commit pending) — IdleTrackingBody wrapper owns the
    in_flight decrement until body EOF/drop; streaming reproducer test added
    and passing.
  - C3: FIXED in repo (commit pending) — existing idle-timeout tests rewired
    to spawn a real upstream and assert a 200 round-trip instead of pointing
    at a dead 127.0.0.1:18080.
  - C1: NOT FIXED — deploy still pending (out of scope for this repo session).
---

# Follow-Up Review #009 — Fix for #008 Was Never Deployed + Streaming-Body Bug in the Fix (+ dead-upstream test wiring)

> **Session update (2026-08-19):** C2 (streaming-body bug) and C3
> (dead-upstream test wiring, found during verification) are **fixed in
> the working tree** and verified by tests. C1 (deploy to dev1) remains
> open and out of scope for the repo session. See the Summary table for
> per-finding status.

## Purpose

Review #008 identified that HTTP/2 keep-alive PINGs defeat the
`keep_alive_timeout` idle timeout, causing idle connections to accumulate
toward the 1024 FD cap. A fix was committed (`4ab8c51`, 2026-08-10 09:35 UTC)
adding an `IdleTrackingService` + `idle_watchdog` that races against
`serve_connection` and closes connections with no in-flight requests.

9 days later, production (dev1) still exhibits the identical leak — 433/1024
FDs after 8 days uptime, growing at ~9 FDs/minute. This review determines why.

The answer is two-fold:
1. **The fix was never deployed.** The binary running on dev1 was built
   from pre-fix source. The commit exists in `main` but no build/deploy
   step shipped it.
2. **The fix has a latent bug for streaming responses** (e.g. `git clone`
   over HTTP) that would surface once deployed: the watchdog can kill
   connections mid-stream because `in_flight` is decremented when the
   handler *returns* the response, not when the body *finishes streaming*.

Both need to be addressed before the leak is actually closed in production.

---

## Finding C1: The review #008 fix was never deployed to dev1 [deploy]

**Severity**: Critical — this is the entire reason the leak is still
happening in production.

### Evidence

The fix commit `4ab8c51` was authored 2026-08-10 09:35:21 UTC. The
reverse-proxy container on dev1 started 6 minutes later at
2026-08-10 09:42:17 UTC — which initially looks like a deploy, but the
binary on dev1 does **not** contain the fix:

| Artifact | Location | MD5 | Contains fix? |
|----------|----------|-----|---------------|
| Source @ HEAD (`4ab8c51`) | `/workspace/@alkdev/reverse-proxy/` (this dev server) | `a76908dd...` (fresh build) | yes |
| Deployed binary (in container) | `/usr/local/bin/reverse-proxy` on dev1 | `6030a483...` | **no** |
| Source binary on this dev server | `target/release/reverse-proxy` (built 2026-07-28) | `3eb5e535...` | no (predates commit) |

Definitive check — `strings` of the deployed binary for the fix's
distinctive log messages:

```
$ sudo strings /usr/local/bin/reverse-proxy | grep -c "closing idle"
0
```

A fresh `cargo build --release` of HEAD produces a binary that *does*
contain those strings:

```
$ strings target/release/reverse-proxy | grep -iE "closing idle|no real request activity"
closing idle HTTP/2 connection (no real request activity)
closing idle connection (no real request activity)
```

So the deployed binary was built from pre-`4ab8c51` source — most likely
the `0885486` build from 2026-07-28 (review #007's C1 fix, which is the
one that added the keep-alive PINGs that *cause* this leak). The Aug 10
09:41 timestamp on `/opt/reverse-proxy/reverse-proxy` is when the pre-built
binary was *copied* into the deploy directory, not when the underlying
code was compiled.

### Why this happened

There is no automated CI/CD or deploy script in the project. The deploy
path is manual: build locally, copy binary to `/opt/reverse-proxy/`,
rebuild the Docker image (`Dockerfile` is just `FROM reverse-proxy:latest`
+ `COPY reverse-proxy /usr/local/bin/reverse-proxy`), `docker compose up
-d`. The fix commit was made, the commit message says "Closes review
#008", review #008's frontmatter still says `status: open` — and the
binary that was deployed that same day was the old one sitting in
`target/release/` from July, not a fresh build of the new HEAD.

The review-008 doc itself was never updated to `status: closed` either,
so there was no doc-level signal that a deploy was pending.

### What needs to happen

1. Build a fresh binary from HEAD (`4ab8c51`): `cargo build --release`
2. **Before deploying**, address C2 below (the streaming-body bug) —
   deploying the fix as-is will close the leak but will also intermittently
   kill large `git clone` / streaming responses.
3. Copy the new binary to dev1's `/opt/reverse-proxy/` (back up the old one)
4. Rebuild the image and `docker compose up -d` on dev1
5. After deploy, verify the watchdog is firing: container logs should
   show `closing idle HTTP/2 connection (no real request activity)` at
   `debug` level (note: live config has `level = "info"`, so bump to
   `debug` temporarily or watch FD count drop instead)
6. Mark review #008 `status: closed` and this review's C1 `status: closed`

### Verification after deploy

```bash
# On dev1, after deploying the fixed binary:
HOST_PID=$(sudo ss -tlnp | grep "15.235.125.95:443" | grep -oE "pid=[0-9]+" | head -1 | sed 's/pid=//')
sudo ls /proc/$HOST_PID/fd | wc -l   # should drop to ~10-25 when idle

# Confirm fix is in the running binary:
sudo strings /usr/local/bin/reverse-proxy | grep -c "closing idle"   # should be >= 1
```

---

## Finding C2: Watchdog decrements `in_flight` when the handler returns, not when the body finishes streaming [new code required] — VERIFIED + FIXED in repo

**Severity**: Critical for correctness — deploying the fix without
addressing this will trade the FD leak for intermittent broken large
transfers. Not a regression of the *current* production behavior (the
current binary has no watchdog at all), but a bug in the fix that would
become visible once C1 is deployed.

**Status (2026-08-19)**: Bug **verified empirically** with a reproducer
test, then **fixed** in `src/server.rs` with the `IdleTrackingBody`
wrapper (Option A from the recommended fix below). The reproducer test
`streaming_body_not_killed_by_idle_watchdog` now passes. Full suite green
(226 unit + 40 integration, clippy clean). C1 (deploy) is still pending
and out of scope for the repo-side fix session.

### The bug

`IdleTrackingService::call` in `src/server.rs:114-127`:

```rust
fn call(&mut self, req: Request<Incoming>) -> Self::Future {
    self.idle_state.touch();
    self.idle_state.in_flight.fetch_add(1, Ordering::SeqCst);

    let inner_fut = self.inner.call(req);
    let idle_state = self.idle_state.clone();

    Box::pin(async move {
        let result = inner_fut.await;          // <-- returns when handler returns Response
        idle_state.in_flight.fetch_sub(1, Ordering::SeqCst);   // <-- decremented here
        idle_state.touch();
        result
    })
}
```

The handler in `src/proxy/handler.rs:118-137` returns the upstream
response as a **streaming body**:

```rust
Ok(Ok(Ok(upstream_resp))) => {
    let (mut parts, body) = upstream_resp.into_parts();
    // ... header munging ...
    let body = Body::new(body);          // body is a streaming Body, not buffered
    Response::from_parts(parts, body)    // returned immediately — body still streaming
}
```

`Body::new(body)` wraps hyper's incoming streaming body without buffering.
The handler's `Future` resolves as soon as the `Response` is constructed
(headers received from upstream), **before** the response body has been
streamed to the client. `inner_fut.await` in `IdleTrackingService`
therefore completes while the body is still in flight — but
`in_flight` is decremented at that point, and `touch()` resets the idle
clock.

### Impact

Consider a `git clone` of a large repo over HTTP/2:
1. Client sends GET `/user/repo.git/info/refs?service=git-upload-pack`
2. Handler forwards to Gitea, Gitea responds with headers + streaming
   pack data
3. Handler returns the streaming `Response` — `inner_fut.await` resolves
4. `IdleTrackingService` decrements `in_flight` to 0, calls `touch()`
5. The body is still streaming (could take minutes for a large clone)
6. After 60s of streaming with no *new* request arriving, the watchdog
   sees `in_flight == 0 && idle_for >= 60s` and fires
7. `tokio::select!` cancels `serve_connection`, dropping the TLS stream
   mid-transfer → `git clone` fails with "RPC failed; HTTP 503" or
   similar

The commit message for `4ab8c51` explicitly claims the fix "prevents
killing long-running requests like large git clones" — but the
implementation does not achieve this for streaming responses, which is
exactly what `git clone` is. The unit tests pass because they test the
`IdleState`/`idle_watchdog` logic in isolation with synthetic
`in_flight` increments; the integration tests pass because the test
upstream returns a buffered body that completes immediately.

### Why the tests don't catch it

`tests/integration_test.rs:1129-1220` — both idle-timeout integration
tests use an upstream that returns immediately with a complete
buffered response. Neither test:
- Streams a response body over a duration > `idle_timeout`
- Verifies the watchdog does not fire during an in-progress stream

The relevant test (`active_http1_connection_not_closed_within_timeout`)
only keeps the connection "active" by making the idle timeout longer
than the test window — it does not exercise a long-running stream with
`in_flight == 0`.

**Correction (2026-08-19, during verification):** The reason is
actually worse than "buffered response" — see C3 below. The two tests
pointed at `127.0.0.1:18080`, where **nothing listens**, so they were
validating watchdog behaviour against an immediate 504 error response
(a `HTTP/1.1 ` status line and `in_flight == 1` hold just as well for
a 504 as a 200). They never exercised a real upstream round-trip at
all, buffered or otherwise.

### Recommended fix

The watchdog must consider a connection "active" while a response body
is still streaming, not just while the handler Future is pending. Two
options:

**Option A (preferred): Track body-stream completion, not handler
return.** Wrap the response body in a custom `Body` type that
increments `in_flight` on construction and decrements it when the body
stream reaches EOF or is dropped. Decrement the handler-level
`in_flight` when the handler returns, but keep a separate
`bodies_in_flight` counter (or reuse the same counter with a second
increment for the body). The watchdog's "idle" condition becomes
`in_flight == 0 && bodies_in_flight == 0 && idle_for >= timeout`.

Sketch:
```rust
struct IdleTrackingBody<B> {
    inner: B,
    idle_state: Arc<IdleState>,
}

impl<B: Body> Body for IdleTrackingBody<B> {
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(...) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        match self.inner.poll_frame(cx) {
            Poll::Ready(None) => {
                self.idle_state.in_flight.fetch_sub(1, Ordering::SeqCst);
                self.idle_state.touch();
                Poll::Ready(None)
            }
            other => {
                self.idle_state.touch();   // activity while streaming
                other
            }
        }
    }
}

impl<B> Drop for IdleTrackingBody<B> {
    fn drop(&mut self) {
        // If the body is dropped before EOF (client disconnect, error),
        // still decrement so we don't leak the counter.
        // (Need to be careful about double-decrement if poll_frame already
        // decremented on EOF — use an AtomicBool "decremented" flag.)
    }
}
```

In `IdleTrackingService::call`, wrap the response body:
```rust
Box::pin(async move {
    let result = inner_fut.await;
    if let Ok(mut resp) = result {
        let body = IdleTrackingBody::new(resp.into_body(), idle_state.clone());
        resp = resp.map(|()| body);   // or Response::from_parts
        // do NOT decrement in_flight here — the body will do it on EOF/drop
    } else {
        idle_state.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
    Ok(resp)
})
```

The `Drop` impl must guard against double-decrement (EOF then drop) with
an `AtomicBool`.

**Option B: Simpler but coarser — only `touch()` while streaming, don't
decrement until an outer wrapper sees the body end.** Same idea, less
plumbing, but harder to get right without the custom `Body`.

**Option C: Make the watchdog only fire on `in_flight == 0` *and*
additionally check that no response body is still being written.** This
requires a way to query "is the connection currently writing a body",
which hyper doesn't expose cleanly. Option A is more tractable.

### Test that would catch this

Add an integration test:
1. Upstream returns a response whose body emits one chunk every 200ms
   for 3 seconds total (so the stream outlasts the idle timeout)
2. Set `idle_timeout = 500ms`
3. Make a request, read the body slowly
4. Assert: the full body is received (no mid-stream close)
5. Assert: after the body finishes, the connection is closed within
   `idle_timeout + slack`

This test fails against the current `4ab8c51` code and passes with
Option A.

**Implemented (2026-08-19):** Added as
`tests/integration_test.rs::idle_timeout_tests::streaming_body_not_killed_by_idle_watchdog`.
Upstream emits 15 chunks every 200ms (3s total) via a new
`TestUpstream::spawn_slow_stream` helper; `idle_timeout = 500ms`. The
test asserts all 15 chunks arrive in order. Against `4ab8c51` it fails
after ~2 chunks (watchdog fires at ~500ms); with the `IdleTrackingBody`
fix it passes (all 15 chunks stream through). The fix in `src/server.rs`
follows Option A but reuses the single `in_flight` counter (the body's
decrement happens on EOF/drop via `IdleTrackingBody`, the handler's
decrement happens only on the `Err` branch — so a streaming `Response`
keeps `in_flight == 1` until the body finishes, which is exactly the
watchdog's idle condition). A `Drop` impl guards against double-decrement
(EOF then drop) with an `AtomicBool`.

---

## Finding C3: Existing idle-timeout tests point at a dead upstream (127.0.0.1:18080) [test hygiene] — VERIFIED + FIXED in repo

**Severity**: Medium — not a production bug, but a test-hygiene defect
that explains *why* C2 was never caught and that would have masked any
future regression of the watchdog against real traffic.

### The bug

`tests/integration_test.rs` (`make_proxy_router` / `start_test_https_server`
before this fix) hardcoded the test upstream as `127.0.0.1:18080`, but
**nothing spawns a listener there**. The two idle-timeout tests
(`idle_http1_connection_closed_after_timeout`,
`active_http1_connection_not_closed_within_timeout`) therefore hit an
immediate upstream **connection refused**, and the proxy returns a 504
error response. The tests then assert:

- `response.starts_with("HTTP/1.1 ")` — true for `HTTP/1.1 504 ...` just
  as for `HTTP/1.1 200 OK`
- `in_flight.count() == 1` right after the response — true regardless of
  status, since the connection is still open
- (for the "active" test) `n > 0` — true for the 504 body bytes

Both tests pass, but they have **never exercised a real upstream
round-trip**. They validate watchdog timing behaviour against an error
response, not a 200 from a live upstream — so they could not catch C2
even if they tried to stream.

### Why this matters

C2's "Why the tests don't catch it" section (above) attributed the miss
to "the test upstream returns immediately with a complete buffered
response." That is generous: the upstream returned immediately because
it didn't exist. The structural defect (no live upstream in the idle
test wiring) is what made the streaming blind spot possible — there was
never a mechanism in the test harness to point the proxy at a spawned
upstream, streaming or otherwise.

### Fix (implemented)

- Refactored `make_proxy_router` → `make_proxy_router_with_upstream(upstream)`
  that takes the upstream address as a parameter.
- Refactored `start_test_https_server` →
  `start_test_https_server_with_upstream(idle_timeout, upstream)`.
- Both idle-timeout tests now spawn a real `TestUpstream::spawn_ok()`
  upstream and assert `response.starts_with("HTTP/1.1 200 OK")` (a real
  round-trip) instead of the loose `HTTP/1.1 ` prefix.
- The streaming reproducer (C2) uses the same wiring with
  `TestUpstream::spawn_slow_stream`.
- Removed the dead no-arg wrappers (`make_proxy_router`,
  `start_test_https_server`) since no caller needs the hardcoded
  `18080` default anymore.

`cargo test` is green (226 unit + 40 integration); `cargo clippy
--all-targets` clean.

---

## Production data snapshot (2026-08-19 09:11 UTC)

Captured 8 days after the (non-)deploy of the fix:

| Metric | Value |
|--------|-------|
| Proxy uptime | 8 days (started 2026-08-10 09:42:17Z) |
| Open FDs | 433 / 1024 soft limit (~42%) |
| FD growth rate | ~9 FDs/minute (sampled: 416 → 425 in 60s; 433 ~10min later) |
| ESTAB connections | 0–397 (highly variable; bots are transient) |
| Unique client IPs (peak) | 212 |
| Top offender | 45.88.186.81 — 54 concurrent idle conns (1337 Services GmbH / LEET ASN) |
| 2nd | 124.198.132.108 — 49 conns (anonymized RIPE) |
| 3rd | 192.159.99.234 — 46 conns (RIPE/NL) |
| 4th | 124.198.132.182 — 22 conns (same /24 as 2nd — same operator) |
| Top-3 share of ESTAB | ~38% of all idle connections |
| Top-3 entries in access log | **0** across all 7 rotated logs (9 days) — pure slow-loris, never send a real request |
| Top-3 fail2ban status | not banned (correct — no 429/401/403 to match) |
| Load average | 0.01 / 0.03 / 0.01 (95% CPU idle) — no performance symptom yet |
| Deployed binary contains fix? | **No** (`strings` grep for "closing idle" returns 0) |

### Interpretation

- The leak is the **same bug class** as review #008 (HTTP/2 PINGs keeping
  idle connections alive). The current binary is the `0885486` build that
  has `keep_alive_interval(15s)` + `keep_alive_timeout(60s)` but **no
  `idle_watchdog`** — so PING-responsive idle clients hold connections
  indefinitely, exactly as #008 described.
- The top offenders are slow-loris-style probers: they open dozens of TLS
  connections, never send an HTTP request (0 access-log entries in 9
  days), and respond to PINGs. This is the precise pattern #008's
  watchdog was designed to kill.
- Time-to-cap estimate: at ~9 FDs/min growth, from a cold start the
  proxy hits 1024 in ~10–12 days. The current 8-day/42% state is
  consistent with this.
- No visible lag because the server is idle and 42% != 100%. The failure
  mode is "new TLS handshakes start failing" at 1024, not gradual
  slowdown.

---

## Minor: Review #008 frontmatter still `status: open`

`docs/reviews/008-http2-keepalive-defeats-idle-timeout.md` has
`status: open` despite commit `4ab8c51` saying "Closes review #008".
This is consistent with the fix never having been deployed — the review
should be closed only when the fix is verified in production. Leave as
`open` until C1 (deploy) + C2 (streaming fix) are both done and verified.

---

## Summary

| ID | Issue | Severity | Action | Status |
|----|-------|----------|--------|--------|
| C1 | Review #008 fix never deployed — dev1 running pre-fix binary | Critical | Build fresh from HEAD (after C2 fix), deploy, verify | **Open** (deploy pending — out of scope for repo session) |
| C2 | Watchdog decrements `in_flight` on handler return, not body-stream end → will kill large `git clone` | Critical (latent) | Wrap response body in `IdleTrackingBody` that owns the decrement; add streaming integration test | **Fixed in repo** (commit pending); verified by reproducer test |
| C3 | Existing idle-timeout tests point at dead `127.0.0.1:18080` — never exercised a real upstream round-trip, so couldn't catch C2 | Medium (test hygiene) | Rewire tests to spawn a real upstream; assert `200 OK` | **Fixed in repo** (commit pending) |
| — | Review #008 `status: open` despite "Closes" commit | Minor | Close after C1+C2 verified in production | Open (unchanged) |

> Note: C2 and C3 are fixed in the working tree but **not yet committed**
> as of this writing — they will land in a single follow-up commit. C1
> (deploy) must still happen before this review can move to `closed`,
> since the leak is still live in production until the rebuilt binary is
> shipped to dev1.

## Recommended sequence for the fix session

1. ~~**In this repo**: implement C2 (the `IdleTrackingBody` wrapper + guard
   against double-decrement). Add the streaming-body integration test
   described above. Run `cargo test` — expect the new test to pass and
   existing tests to still pass.~~ **Done (2026-08-19).** C2 + C3 fixed,
   `streaming_body_not_killed_by_idle_watchdog` reproducer added (red on
   `4ab8c51`, green with fix), existing idle-timeout tests rewired to a
   real spawned upstream. `cargo test` 226+40 green, `cargo clippy
   --all-targets` clean. **Commit pending.**
2. **Build**: `cargo build --release` → produces a binary with both the
   watchdog (from `4ab8c51`) and the streaming fix (new).
3. **Deploy to dev1**: copy binary, rebuild image, `docker compose up -d`.
   Back up the current binary first.
4. **Verify C1**: `strings /usr/local/bin/reverse-proxy | grep "closing
   idle"` returns ≥1. Watch FD count drop to baseline over ~10 minutes.
5. **Verify C2**: do a large `git clone` over HTTPS against
   `git.alk.dev` — confirm it completes without mid-stream termination.
6. **Optionally** bump log level to `debug` for an hour to see watchdog
   firing on the slow-loris IPs.
7. **Close reviews**: mark #008 and #009 `status: closed` with a note
   pointing at the deploy verification.

## References

- [Review #008](008-http2-keepalive-defeats-idle-timeout.md) — the
  original finding about HTTP/2 keep-alive defeating idle timeout
- [Review #007](007-connection-lifecycle-and-deployment-drift.md) — C1
  (no idle timeout) and C2 (no concurrency cap), the predecessor fixes
- Commit `4ab8c51` — the undeployed fix for #008
- Commit `0885486` — the review #007 fix that is what dev1 is actually
  running (the keep-alive PING code that causes this leak)
- `src/server.rs:114-127` — `IdleTrackingService::call` (the C2 bug
  location)
- `src/proxy/handler.rs:130-137` — streaming response construction
- `tests/integration_test.rs:1129-1220` — existing idle-timeout tests
  (do not cover streaming)
- Production observation: dev1, 2026-08-19, 433/1024 FDs after 8 days,
  fix absent from deployed binary