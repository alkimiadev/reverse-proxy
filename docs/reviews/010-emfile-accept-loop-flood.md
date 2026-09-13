---
status: mitigated
last_updated: 2026-09-13
reviewed_code:
  - src/server.rs
  - src/config/static_config.rs
  - /opt/reverse-proxy/docker-compose.yml (dev1 deploy)
  - /etc/reverse-proxy/config.toml (dev1 deploy)
reviewer: code-reviewer
based_on: docs/reviews/009-fix-never-deployed-and-streaming-body-bug.md
trigger: >-
  Production incident on dev1 (2026-09-11 ~21:00-23:00 UTC) — EMFILE
  busy-loop wrote ~6M ERROR lines (~1.9 GB log) in 2 hours
fixes:
  - >-
    M1: MITIGATED in production 2026-09-12 — container RLIMIT_NOFILE raised
    to 8192 (compose ulimits) and max_connections lowered 1024 → 800.
    Code fixes (C1 backoff, C2 validation) remained open.
  - >-
    C1: FIXED 2026-09-13 — accept-loop error classification + backoff +
    signature-keyed log de-duplication (src/server.rs).
  - >-
    C3: FIXED 2026-09-13 — TLS handshake timeout (tls_handshake_timeout_secs,
    default 10s) wraps tls_acceptor.accept() in src/server.rs; stalled
    handshakes release FD + permit.
  - >-
    C4: OPEN — connection semaphore is per-listener (src/server.rs:338), so
    the effective cap is max_connections × listeners; sequence before C2 so
    the RLIMIT cross-check uses the real FD budget.
  - >-
    C2: OPEN — no startup cross-check that max_connections fits under
    RLIMIT_NOFILE with headroom. Sequenced after C3/C4.
---

# Review #010 — EMFILE Accept-Loop Flood (6M ERROR lines, 1.9 GB log in 2 hours)

## Summary

On 2026-09-11, between approximately 21:00 and 23:00 UTC, the production
reverse proxy on dev1 hit `EMFILE` ("No file descriptors available", os
error 24) and its accept loop **busy-spun, emitting ~6M ERROR log lines
(~1.9 GB) in roughly 2 hours** — approximately 460K ERROR lines in the
22:00 hour alone, sustained at ~127 lines/second.

The accept loop has no error backoff: `tcp_listener.accept()` fails
instantly and repeatedly when the process is out of file descriptors, and
the `continue` on error turns that into a hot spin. Every iteration writes
an `error!` line both to stdout (Docker json-file, unbounded by rotation)
and to the access log file. The access log grew to 1.9 GB in a day
(vs. ~60 MB on normal days); 99% of it was the EMFILE flood. This is the
same failure shape as a task leak: an unbounded resource (log bytes) with
no circuit breaker.

Two contributing design gaps made the EMFILE reachable at all:

1. **C1**: the accept loop treats accept errors as transient
   ("log and continue"), which is correct for `EINTR`/`EAGAIN` but
   catastrophic for persistent errors like `EMFILE`/`ENFILE` —
   retry-without-delay on those is a busy loop.
2. **C2**: `max_connections = 1024` (the semaphore cap on concurrent TLS
   connections) equals the container's Docker-default `RLIMIT_NOFILE = 1024`
   — zero headroom for the listener sockets, health/admin listener, log
   file FD, ACME renewal sockets, stdin/out/err, and epoll instances.
   The semaphore therefore admits connections right up to (and past,
   counting non-connection FDs) the FD ceiling.

### Production mitigation applied (2026-09-12)

- `/opt/reverse-proxy/docker-compose.yml`: added
  `ulimits: nofile: {soft: 8192, hard: 8192}`.
- `/etc/reverse-proxy/config.toml`: `max_connections` 1024 → 800.
- Container restarted; verified: process limit shows 8192, health green,
  h2 traffic normal, zero EMFILE lines since.

With `nofile = 8192` and `max_connections = 800`, the semaphore now caps
connection FDs at ~10% of the limit, leaving ample headroom. This removes
the trigger, **not** the failure mode — see C1/C2 for the code fixes that
remain open.

## Evidence

### Timeline (from /var/log/reverse-proxy/access.log.1, since rotated)

- 20:00 hour: 54 ERROR lines (baseline noise all day was 8–18/2h block)
- 21:00 hour: 5,558,249 ERROR lines (grep count) — flood begins
- 22:00 hour: 457,419 ERROR lines (flood tail)
- First ERROR overall: `2026-09-11T00:15:37Z`; last:
  `2026-09-12T07:30:27Z` (the 07:30 last line is the deploy restart
  cutting off the tail)

Error line shape:

```
2026-09-11T22:00:00.000011Z ERROR reverse_proxy::server: failed to accept TCP connection error=No file descriptors available (os error 24)
2026-09-11T22:00:00.000024Z ERROR ... (repeat, ~80 µs apart)
```

Note the ~13 µs spacing between consecutive EMFILE lines at 22:00:00.000 —
a pure busy loop, no backoff, no yield.

### Rotated-log size comparison

| File | Size | Notes |
|------|------|-------|
| access.log.2.gz (Sep 11→12) | 55 MB gz / 1.9 GB raw | flood day |
| access.log.3–7.gz (Sep 6→11) | 4.9–8.1 MB gz each | normal days |

### Trigger conditions

The EMFILE state was reachable because concurrent TLS connections
(throttled only by the 1024 semaphore) plus the proxy's own FDs exceeded
the container's Docker-default `RLIMIT_NOFILE = 1024`. Verified live on
dev1 before mitigation:

```
Max open files   1024  519288  files      (containerized process)
```

Traffic context: the flood hour followed sustained crawler load (~77K
requests from a 63-IP crawler fleet that day, plus a 30K-request poller
from 74.7.242.34). A burst of slow/held TLS connections (e.g. crawlers
opening connections and stalling handshakes) suffices to exhaust 1024 FDs;
TLS handshakes in progress each hold an FD, and `connection_idle_timeout`
only frees them after 60 s.

After mitigation (`nofile 8192`, `max_connections 800`): 0 EMFILE lines,
21 FDs in use at idle, health checks green.

## Finding C1: Accept loop busy-spins on persistent accept errors [server]

**Status**: FIXED 2026-09-13. The accept loop in `serve_https_listener()`
now classifies accept errors and backs off (see "C1 fix" below).

**Severity**: High — turns any FD/socket exhaustion into a log flood that
burns disk and I/O (1.9 GB in 2 hours observed) and starves the accept
loop entirely.

**Location**: `src/server.rs`, `serve_https_listener()` (lines ~255–264):

```rust
loop {
    tokio::select! {
        accept_result = tcp_listener.accept() => {
            let (tcp_stream, remote_addr) = match accept_result {
                Ok(conn) => conn,
                Err(e) => {
                    error!(error = %e, "failed to accept TCP connection");
                    continue;   // <- busy spin on EMFILE/ENFILE
                }
            };
            ...
```

**Why it matters**: `accept()` fails *immediately* with `EMFILE` — there
is no blocking wait between retries, so `continue` yields a ~13 µs
error-log cycle. The log write itself (to file + stdout) makes the spin
slower and more destructive.

**C1 fix (landed)**: `src/server.rs` implements:

- `is_transient_accept_error()` — `WouldBlock`/`Interrupted` retry
  immediately with no log and no sleep (correct under load).
- `is_resource_accept_error()` — `ConnectionAborted`, `EMFILE`, `ENFILE`,
  `ENOBUFS`, `ENOMEM` → log + 1 s backoff.
- Everything else → log + 100 ms backoff.
- `AcceptErrorReporter` — de-duplicates by error signature: first
  occurrence logs at `error!`, repeats within a 10 s window are counted,
  and each window close emits one summary line with `suppressed=N`
  `backoff_ms=1000`. The 6M-line incident shape would now produce ~720
  log lines over 2 hours instead of 6M.
- The `conn_sem.acquire_owned()` error path (semaphore closed) also
  sleeps 100 ms instead of spinning.

Notes from implementation:

- The redirect (:80) and health (:9900) listeners use `axum::serve`,
  which already applies the hyper-style 1 s backoff on non-connection
  accept errors (axum 0.8.9 `src/serve/listener.rs` →
  `handle_accept_error`). No change needed there; the busy-spin existed
  only in the custom HTTPS loop.
- Two follow-up findings surfaced while fixing C1 (tracked in
  "Recommended next steps"):
  - ~~**C3 (new)**~~ — **FIXED 2026-09-13**: `tls_acceptor.accept()` had
    no timeout, so a stalled TLS handshake held an FD *and* a semaphore
    permit indefinitely (the idle watchdog only starts after the
    handshake completes). Fixed via `tls_handshake_timeout_secs`
    (default 10s) wrapping the accept in `tokio::time::timeout`. This
    was the likely actual FD-exhaustion vector for slow/held crawler
    handshakes, and a slowloris amplifier.
  - **C4 (open)**: `conn_sem` is per-listener (`main.rs` spawns one
    `serve_https_listener` per listener, each creating its own
    semaphore), so the effective connection cap is
    `max_connections × listeners`. Any C2 RLIMIT cross-check must
    account for this (or the semaphore should be shared).

### Original finding (pre-fix, preserved for context)

**Location**: `src/server.rs`, `serve_https_listener()` (lines ~255–264):

```rust
loop {
    tokio::select! {
        accept_result = tcp_listener.accept() => {
            let (tcp_stream, remote_addr) = match accept_result {
                Ok(conn) => conn,
                Err(e) => {
                    error!(error = %e, "failed to accept TCP connection");
                    continue;   // <- busy spin on EMFILE/ENFILE
                }
            };
            ...
```

**Why it matters**: `accept()` fails *immediately* with `EMFILE` — there
is no blocking wait between retries, so `continue` yields a ~13 µs
error-log cycle. The log write itself (to file + stdout) makes the spin
slower and more destructive.

Options worth considering while fixing:
- Log the first N occurrences of a repeated accept error at `error!`, then
  demote to a periodic (e.g. once/10s) summary until the error clears —
  bounds log damage from *any* repeating accept failure.
- On EMFILE specifically, a small sleep (500 ms–1 s) is both correct and
  cheap: the process cannot make progress on accepts until FDs are
  released anyway.

**Note**: the redirect (HTTP:80) listener and health listener use a
different loop shape (`axum::serve`) which already backs off; see the
implementation notes above.

## Finding C2: max_connections is not cross-checked against RLIMIT_NOFILE [config]

**Severity**: Medium — the semaphore is the intended protection against FD
exhaustion, but it can be configured at/above the process's real ceiling,
making it a false safety net.

**Location**: `src/config/static_config.rs` (`default_max_connections()`
= 1024) and validation in `src/config/validation.rs`.

**Why it matters**: `max_connections` counts only TLS connections; the
process also needs FDs for: HTTPS + HTTP + health-check listener sockets,
the log file, stdin/stdout/stderr, epoll/timer FDs, and ACME renewal
outbound sockets (rustls-acme opens TCP + TLS per ACME interaction).
With `max_connections == RLIMIT_NOFILE` (the dev1 situation, both 1024),
the semaphore guarantees the ceiling can be hit under load.

**Recommended fix**: at startup (and reload), read the process soft
`RLIMIT_NOFILE` (via `rlimit` crate or `getrlimit`) and reject config (or
warn + clamp) if:

```
max_connections + reserved_fds > soft_limit
```

with `reserved_fds` ~64 (listeners + log + ACME + epoll headroom). Warn
prominently when the soft limit is the Docker default 1024; better, the
project's Docker/systemd docs should recommend `nofile 8192` alongside
`max_connections = 1024` (or vice versa: derive a sane default from the
observed limit).

## Finding M1 (mitigation): raise nofile + lower max_connections [deploy]

**Status**: Applied 2026-09-12 (see Summary). Backups:
`docker-compose.yml.bak.20260912-080241`, `config.toml.bak.20260912-080241`.

Residual risk after mitigation: none identified for FD exhaustion at
current traffic (peak concurrent connections observed ≪ 800); the code
findings C4/C2 remain the durable fix (C1 + C3 landed 2026-09-13).

## Traffic-analysis side note (from the same investigation)

The EMFILE hunt began as a traffic-volume estimate for Gitea, which
surfaced the flood because yesterday's rotated log was 1.9 GB vs ~60 MB
normal. Traffic profile (2026-09-11, 204K REQUEST lines):

- ~77K requests from a 63-IP crawler fleet (57.141.20.x — GitHub's
  published crawler range), ~1.3K req/IP, commit/blame/raw pages
- ~30K requests from 74.7.242.34 (Microsoft) polling `/pulls` + `/issues`
  on 3 repos (`alkdev/alktunnels`, `alkdev/alktls`, `alkdev/alkgen`)
  plus 56 on `alkdev/typemap`; all GET, no POSTs — dashboard-style
  monitoring, first seen that day
- ~20 HTTPS git fetches, ~22 API calls, zero HTTPS pushes
- Zero POSTs to `/user/login`; zero 401/403 that day — no credential
  attacks over HTTPS

None of this traffic is hostile per se, but it does mean the "1024
concurrent connections" ceiling is reachable from ordinary crawler
behavior, which is exactly what made the EMFILE state reachable.

## Recommended next steps

1. ~~Land C1 (accept-loop error backoff + log de-duplication)~~ — DONE
   2026-09-13 (see Finding C1).
2. ~~Land C3 (TLS handshake timeout)~~ — DONE 2026-09-13. New static config
   `tls_handshake_timeout_secs` (default 10s, must be > 0) wraps
   `tls_acceptor.accept()` in `tokio::time::timeout`
   (`accept_tls_with_timeout()`, src/server.rs). On timeout the handshake
   future is dropped, releasing the TCP FD and the semaphore permit; the
   idle watchdog never needs to run for a stalled handshake. Closes the
   crawler slow-handshake vector described under "Trigger conditions".
3. Land C4 (shared connection semaphore across listeners) so
   `max_connections` is a global cap rather than per-listener. Sequenced
   before C2 because the RLIMIT cross-check's FD budget depends on the
   final semaphore topology (shared vs per-listener).
4. Land C2 (RLIMIT cross-check at startup) with a prominent warning or
   hard validation error. Must account for C4 (per-listener semaphore
   multiplication → shared after C4 lands) when computing the FD budget.
   Lower urgency after M1: with the deploy baseline (`nofile 8192`,
   `max_connections 800`) the ceiling is ~10% of the limit, so C2 is a
   validation guard, not an active exposure.
5. Consider a doc note in `docs/architecture/operations.md` describing the
   nofile/max_connections relationship (the mitigation values above are a
   working baseline: `nofile 8192`, `max_connections 800`).
6. Docker image: the deployment Dockerfile on dev1 is a two-line stub
   (FROM + COPY); if the project's own `deploy/Dockerfile` is ever used,
   it should also set `ulimits` guidance or rely on compose as done here.
   The repo's own `deploy/docker-compose.yml` still lacks the `ulimits`
   block applied to dev1, and `deploy/reverse-proxy.service` lacks
   `LimitNOFILE`.