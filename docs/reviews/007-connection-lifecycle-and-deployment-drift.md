---
status: open
last_updated: 2026-07-28
reviewed_code:
  - src/server.rs
  - src/proxy/handler.rs
  - src/proxy/mod.rs
  - src/main.rs
  - src/shutdown.rs
  - src/logging/mod.rs
  - src/logging/format.rs
  - src/config/static_config.rs
  - src/config/mod.rs
  - src/health.rs
  - src/rate_limit/mod.rs
  - deploy/Dockerfile
  - deploy/docker-compose.yml
  - deploy/fail2ban/jail.d/reverse-proxy.conf
  - deploy/fail2ban/filter.d/reverse-proxy.conf
  - deploy/fail2ban/filter.d/reverse-proxy-4xx.conf
  - deploy/fail2ban/filter.d/reverse-proxy-badbots.conf
reviewer: code-reviewer
based_on: docs/reviews/006-attack-surface-review.md
trigger: Production incident (reverse-proxy FD exhaustion, 2026-07-24) and follow-up fail2ban audit
---

# Operational Review #007 — Connection Lifecycle, Logging, and Deployment Drift

## Purpose

This review was triggered by a production incident where the
reverse-proxy container exhausted its 1024 file-descriptor limit after ~6 weeks
of uptime, making a public Gitea instance unresponsive. A subsequent fail2ban
audit revealed that the HTTP-level ban protection had been silently broken since
the nginx→reverse-proxy migration (0 bans ever from the `reverse-proxy` jail).

The review examines the **current HEAD** (`f6e62a3`, 2026-06-15) source for
connection-lifecycle, logging, and deployment issues that contributed to the
incident or were uncovered during the audit. It also catalogs the gap between
a deployed binary (built from `cfe0ae5`, 2026-06-15 — *before* the admin
socket removal) and HEAD, since that production server has not yet been updated.

Findings are grouped into:
- **Critical** — caused or directly contributed to the production outage
- **Warning** — operational risks that will recur or cause future outages
- **Suggestion** — improvements to robustness, observability, or maintainability

Each finding includes the code location in HEAD, why it matters, and a
recommended fix. Findings marked **[new code required]** need source changes;
those marked **[deploy existing]** are already fixed in HEAD and only require
shipping a new image.

---

## Critical Findings

### C1. No server-side idle/keep-alive timeout on TLS connections [new code required]

**Location**: `src/server.rs:102-125`

The HTTPS listener spawns a task per accepted connection and calls
`serve_connection` / `serve_connection_with_upgrades` on the hyper builder.
Neither the `http2::Builder` (line 103) nor the `auto::Builder` (line 112)
sets any keep-alive or idle timeout:

```rust
// line 102-110 (HTTP/2 path)
let mut builder = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
if let Err(e) = builder
    .enable_connect_protocol()
    .serve_connection(io, svc)   // no keep_alive_interval / keep_alive_timeout
    .await
{ ... }

// line 111-125 (HTTP/1.1 + auto path)
let mut builder = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
builder.http2().enable_connect_protocol();
if let Err(e) = builder
    .serve_connection_with_upgrades(io, svc)   // no http1 keep_alive_timeout
    .await
{ ... }
```

**Impact**: This is the primary root cause of the FD exhaustion. A connection
that the client opens but never cleanly closes (half-open TCP, abandoned
scanner socket, or a bot that stops sending without FIN) keeps its spawned
task + socket FD alive **forever**. Over 6 weeks of bot traffic (a crawler
blasting hundreds of `/raw/commit/...` and `/blame/commit/...` URLs),
abandoned TLS connections accumulated until the process hit its 1024 soft FD
limit. At the time of the incident, the container had exactly 1014 `socket:`
FDs open — all held by spawned connection tasks with no timeout to reap them.

The proxy became unable to accept new connections:
```
ERROR reverse_proxy::server: failed to accept TCP connection error=No file descriptors available (os error 24)
```

Every new HTTPS request to the proxied site timed out, while the upstream
service itself (on `127.0.0.1:3000`) remained healthy — only the proxy was
broken.

**Recommendation**: Set idle/keep-alive timeouts on both builders:

```rust
use std::time::Duration;
const SERVER_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

// HTTP/2 path
let mut builder = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
builder
    .keep_alive_interval(Some(Duration::from_secs(15)))
    .keep_alive_timeout(SERVER_IDLE_TIMEOUT)
    .enable_connect_protocol()
    .serve_connection(io, svc)

// HTTP/1.1 + auto path
let mut builder = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
builder
    .http1().keep_alive_timeout(SERVER_IDLE_TIMEOUT)
    .http2().keep_alive_interval(Some(Duration::from_secs(15)))
        .keep_alive_timeout(SERVER_IDLE_TIMEOUT)
    .enable_connect_protocol();
builder.serve_connection_with_upgrades(io, svc)
```

This makes idle connections close after 60s of inactivity (with HTTP/2
keep-alive pings at 15s to detect dead peers faster). The timeout should be
configurable via `StaticConfig` (e.g. `connection_idle_timeout_secs`, default
60).

**Severity rationale**: This caused a complete outage of the public Gitea
instance. It will recur on any deployment with sustained traffic, with a
time-to-failure proportional to the FD limit and traffic pattern.

---

### C2. No concurrency cap on accepted connections [new code required]

**Location**: `src/server.rs:65-127`

The accept loop calls `tokio::spawn` for every accepted connection with no
bound on the number of concurrent tasks:

```rust
loop {
    tokio::select! {
        accept_result = tcp_listener.accept() => {
            // ...
            tokio::spawn(async move {
                let _guard = InFlightGuard::new(in_flight.clone());
                // ... serve connection ...
            });
        }
        // ...
    }
}
```

The `InFlightCounter` (lines 17-54) only **counts** active connections for
graceful-shutdown draining — it does not **limit** them. There is no
semaphore, no `max_connections` config, and no backpressure on the accept
loop. An attacker (or a misbehaving crawler) can open thousands of
simultaneous TLS connections, each consuming a task + FD + TLS state, with
nothing to stop the process from exhausting FDs or memory.

This is the same class of issue flagged as **W1** in review #005 ("No
connection concurrency limit"), but that finding was scoped to the admin
socket and declared "eliminated" by removing the admin socket (ADR-028, line
168). The **public HTTPS listener** was never bounded. Review #006 (attack
surface review) section 1.1 noted "No resource limiting beyond OS TCP
backlog" but did not escalate it.

**Recommendation**: Add a `tokio::sync::Semaphore` that gates connection
acceptance, with a configurable `max_connections` (default e.g. 1024):

```rust
use tokio::sync::Semaphore;
use std::sync::Arc;

let conn_sem = Arc::new(Semaphore::new(max_connections));

loop {
    tokio::select! {
        accept_result = tcp_listener.accept() => {
            let (tcp_stream, remote_addr) = // ...;
            let permit = conn_sem.clone().acquire_owned().await.unwrap();
            let in_flight = in_flight.clone();
            let tls_acceptor = tls_acceptor.clone();
            let router = router.clone();

            tokio::spawn(async move {
                let _guard = InFlightGuard::new(in_flight.clone());
                let _permit = permit;  // released when task ends
                // ... serve connection ...
            });
        }
        // ...
    }
}
```

The semaphore `acquire_owned().await` blocks the accept loop when all permits
are taken, providing natural backpressure (the OS TCP backlog holds pending
connections). Combined with C1's idle timeout, this bounds both concurrent and
leaked connections.

**Severity rationale**: Without this, the FD limit is the only backstop, and
hitting it takes down the entire service. With C1 fixed, this becomes less
urgent, but it's still needed for defense against intentional FD exhaustion
attacks and to prevent memory unbounded growth under load.

---

## Warning Findings

### W1. No SIGHUP log-reopen support (sparse-file hazard) [new code required]

**Location**: `src/shutdown.rs:70-73`, `src/logging/mod.rs:36-37,74-75`

SIGHUP is handled exclusively as a config-reload signal (`shutdown.rs:70-73`):
```rust
SIGHUP => {
    tracing::info!(event = "SIGNAL", signal = "SIGHUP");
    handle_sighup_reload(&reload_handle, &config_path).await;
}
```

The log file is opened once at startup with `File::create(path)` and held as
an `Arc<File>` writer (`logging/mod.rs:36-37,74-75`). There is no mechanism to
reopen the log file — not on SIGHUP, not on any signal.

**Impact**: This breaks `logrotate` and any external log rotation that relies
on moving/renaming the file. The standard approaches are:
1. **`copytruncate`** — logrotate copies the file, then truncates it to zero.
   The process keeps its FD and offset. **But**: if the process's FD offset
   is large (e.g. the file was 1GB), `truncate -s 0` resets the file size to
   0 while the process's FD offset stays at 1GB. The next write lands at
   offset 1GB, creating a **sparse file** with a 1GB hole. This is exactly
   what happened during the incident response: after `truncate -s 0` on the
   1.15GB access log, the file immediately reported 1.15GB apparent size
   (with only 8KB of real blocks). Fail2ban then tried to scan this "1.15GB"
   file, loaded the sparse hole into memory, and bloated to 7.3GB RSS,
   wedging the fail2ban server.
2. **`postrotate` with signal** — logrotate renames the file, then signals
   the process to reopen. This is the standard nginx/apache pattern. But the
   proxy doesn't support log-reopen on any signal (SIGHUP is config reload).

The production logrotate config currently uses `copytruncate` with `maxsize 100M`
to bound the offset growth, but this is a workaround, not a fix. The sparse
file will still appear whenever the log is truncated while the offset is high.

**Recommendation**: Add a log-reopen signal handler. Options:

- **Option A (preferred)**: Use `SIGUSR1` for log reopen (the conventional
  choice, used by nginx, Apache, etc.). Keep SIGHUP for config reload. The
  handler closes the old `Arc<File>` and opens a new one at the same path,
  swapping it atomically. This enables standard `postrotate` logrotate
  configs without `copytruncate`.

- **Option B**: Reuse SIGHUP to do both config reload AND log reopen. Less
  clean (conflates two operations) but avoids adding a new signal handler.

In either case, the `Arc<File>` writer in `logging/mod.rs` needs to become an
`Arc<ArcSwap<File>>` or similar so the reopen can swap the writer atomically
without restarting the tracing subscriber. The `tracing_subscriber` `MakeWriter`
trait is the clean way to support this (implement a custom writer that holds
an `Arc<ArcSwap<File>>` and reads the current file on each write).

**Severity rationale**: This caused the fail2ban wedge during incident
response and makes log rotation fragile. It won't cause the original outage
on its own, but it complicates recovery and will recur on every log rotation.

---

### W2. Upstream client pool has no max-idle-per-host bound [new code required]

**Location**: `src/proxy/handler.rs:232-258`

Both the HTTP and HTTPS upstream clients set `pool_idle_timeout(90s)` but do
not set `pool_max_idle_per_host`:

```rust
pub fn create_http_client() -> Client<HttpConnector, Body> {
    let mut connector = HttpConnector::new();
    connector.set_connect_timeout(Some(Duration::from_secs(CONNECT_TIMEOUT_CEILING_SECS)));
    Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(90))
        // no .pool_max_idle_per_host(...)
        .build(connector)
}
```

`hyper-util`'s default `pool_max_idle_per_host` is **unbounded** (or very
high, depending on version). For a single-upstream deployment (e.g. a proxy
where everything routes to `127.0.0.1:3000`), every concurrent
client request can leave an idle connection in the pool, and all of them are
to the same host. Under burst traffic (e.g. a crawler hitting 20+ URLs in
rapid succession), the pool accumulates idle upstream connections, each
holding a socket FD on the upstream side.

This is a secondary contributor to the FD exhaustion — the 1014 leaked FDs
were primarily client-facing TLS sockets (C1), but upstream pool connections
also consume FDs and have no bound.

**Recommendation**: Set `pool_max_idle_per_host` to a reasonable value (e.g.
10-20) on both clients:

```rust
Client::builder(TokioExecutor::new())
    .pool_idle_timeout(Duration::from_secs(90))
    .pool_max_idle_per_host(10)
    .build(connector)
```

This bounds the upstream pool to at most 10 idle connections to any single
upstream, which is plenty for a single-user Gitea instance.

---

### W3. No connection idle timeout is configurable [new code required]

**Location**: `src/config/static_config.rs` (no such field)

There is no config option for server-side connection idle timeout. Even after
C1 is fixed with hardcoded timeouts, operators should be able to tune this
without a rebuild. A site serving long-lived HTTP/2 streams (e.g. Gitea's
event-source endpoints) might need a longer timeout than a static-file site.

**Recommendation**: Add `connection_idle_timeout_secs` to `StaticConfig`
(default 60), and use it for the keep-alive timeouts in C1. Document it in
the config schema and README.

---

### W4. No `max_connections` is configurable [new code required]

**Location**: `src/config/static_config.rs` (no such field)

Related to C2. The concurrency cap should be operator-tunable, not hardcoded.
A high-traffic deployment might need more than 1024; a low-memory deployment
might want fewer.

**Recommendation**: Add `max_connections` to `StaticConfig` (default 1024),
wire it to the `Semaphore` in C2.

---

### W5. Deployed binary predates admin socket removal — config format drift [deploy existing]

**Location**: Production `/etc/reverse-proxy/config.toml` vs `src/config/static_config.rs` at HEAD

The production binary was built from `cfe0ae5` (2026-06-15 05:23 UTC), which
is the commit **immediately before** `3ea3f56` ("Replace Unix socket admin API
with authenticated HTTP admin API"). The live config still uses:

```toml
admin_socket_path = "/run/reverse-proxy/admin.sock"
```

At HEAD (`f6e62a3`), this field was replaced with:

```toml
admin_key_path = "/etc/reverse-proxy/admin-key"
```

The deployed binary has the Unix socket admin API (with the vulnerabilities
documented in review #005: symlink race C1, no auth C2, info leak C3, no
concurrency limit W1). HEAD replaced it with the authenticated HTTP admin API
(ADR-028).

**Commits in HEAD not yet deployed** (4 commits, `3ea3f56..f6e62a3`):
1. `3ea3f56` — Replace Unix socket admin API with authenticated HTTP admin API
2. `c6dda71` — Add mtime TOCTOU check and wildcard flag to ConfigReloadHandle
   (ADR-029/030: fixes W2 config-reload TOCTOU and W5 wildcard-flag drift
   from review #005)
3. `143ebaa` — Add wildcard bind acceptance/rejection tests for reload path
4. `f6e62a3` — Update README/AGENTS.md for admin HTTP API

**Impact**: The production server is running with known admin-API
vulnerabilities (review #005 C1-C3) that are already fixed in source. The
config file uses a field name that HEAD doesn't recognize, so deploying HEAD
as-is would break admin functionality until the config is migrated.

**Recommendation**: When deploying the new image (after C1/C2/W1-W4 are
fixed), the config migration is:
1. Replace `admin_socket_path = "..."` with `admin_key_path = "/etc/reverse-proxy/admin-key"`
   (or set to empty string `""` to disable admin endpoints)
2. If enabling admin: `openssl rand -hex 32 > /etc/reverse-proxy/admin-key && chmod 600 /etc/reverse-proxy/admin-key`
3. Remove the `/run/reverse-proxy` volume mount from docker-compose (no longer
   needed for the admin socket)
4. Add `/etc/reverse-proxy/admin-key:/etc/reverse-proxy/admin-key:ro` volume
   mount if admin is enabled

This is a **deploy existing** fix — no new code needed, just ship HEAD (plus
the C1/C2/W1-W4 fixes) and migrate the config.

---

### W6. fail2ban jail backend mismatch (already fixed in project, but worth documenting) [deploy existing]

**Location**: `deploy/fail2ban/jail.d/reverse-proxy.conf` (already updated)

The original jail config did not set `backend =`, so it inherited
`backend = systemd` from `/etc/fail2ban/jail.d/defaults-debian.conf`. The
systemd backend ignores `logpath` and reads journald — but the reverse-proxy
uses Docker's `json-file` driver and writes to a file, not journald. Result:
the jail silently matched nothing (0 bans ever despite 3830 RATE_LIMIT lines
in the access log).

This is now fixed in the project's `deploy/fail2ban/` files (`backend = auto`
added). Documented here so future deployments don't regress.

**Recommendation**: Already addressed. No further action needed beyond
ensuring the updated `deploy/fail2ban/` files are used in future deployments.

---

## Suggestion Findings

### S1. Add FD usage / connection count metrics [new code required]

**Location**: `src/server.rs` (InFlightCounter), `src/health.rs`

The `InFlightCounter` already tracks active connections for graceful shutdown.
Expose it via the `/health` endpoint (or a new `/metrics` endpoint) so
operators and monitoring can see connection count trends. Additionally, log
periodic FD usage (read `/proc/self/fd` count on Linux) so FD exhaustion is
detected before it hits the limit.

**Recommendation**:
- Add `in_flight_connections` to the `/health` JSON response
- Add a periodic (every 60s) info log line: `in_flight=N fds=M` where M is
  the count of open FDs from `/proc/self/fd`
- Optionally add a warning log when FDs exceed 80% of the soft limit

This would have given early warning of the FD exhaustion weeks before the
outage.

---

### S2. Default rate limit (10 rps / burst 20) is permissive for single-user Gitea [config, no code change]

**Location**: Production `/etc/reverse-proxy/config.toml`

The live config has `requests_per_second = 10, burst = 20`. This is per-IP,
so a crawler gets 20 burst + 10/s indefinitely before
seeing a 429. For a single-user Gitea instance, this is generous — legitimate
traffic (git clone, web UI browsing) rarely exceeds 2-3 rps.

The permissive rate limit meant the crawler could blast hundreds of requests
per minute, each opening a TLS connection that (per C1) never timed out,
accelerating the FD exhaustion. With C1 fixed, the impact is reduced, but
tighter rate limits would further reduce crawler pressure and the volume of
RATE_LIMIT log lines fail2ban must process.

**Recommendation**: Consider `requests_per_second = 3, burst = 5` for
single-user deployments. This is a config change, not a code change, and
should be tuned per deployment. The fail2ban `reverse-proxy` jail
(maxretry=10, findtime=60s) will ban IPs that consistently exceed even the
tighter limit.

---

### S3. Log file grows unbounded between logrotate runs [config/ops, partially addressed]

**Location**: Production `/etc/logrotate.d/reverse-proxy`

The logrotate config uses `maxsize 100M` with `copytruncate`. This bounds the
file to ~100MB before rotation. However, as documented in W1, `copytruncate`
on a file with a high FD offset creates a sparse file. The logrotate will
trigger on the 100MB **apparent** size, but the actual disk usage may be far
smaller (sparse hole), and the truncated file will immediately re-report a
large apparent size.

**Recommendation**: This is fully resolved by W1 (SIGHUP log-reopen +
`postrotate` logrotate). Until W1 is implemented, the workaround is to
restart the proxy in a logrotate `postrotate` script:
```
postrotate
    docker restart reverse-proxy 2>/dev/null || true
endscript
```
This is heavy-handed (brief downtime on each rotation) but avoids the sparse
file problem. The proper fix is W1.

---

### S4. Document the connection-lifecycle config in the README [docs, after C1/W3/W4]

**Location**: `README.md` — Configuration section

Once `connection_idle_timeout_secs` (W3) and `max_connections` (W4) are added
to `StaticConfig`, document them in the README's static config table and the
architecture docs (`docs/architecture/config.md`).

Include a "Tuning for your traffic" section with guidance:
- Single-user / low-traffic: `max_connections=256, connection_idle_timeout_secs=60`
- Multi-user / high-traffic: `max_connections=2048, connection_idle_timeout_secs=120`
- Behind a load balancer: higher `max_connections`, shorter idle timeout

---

### S5. Add integration test for connection cleanup under idle timeout [new code required, after C1]

**Location**: `tests/integration_test.rs`

After C1 is implemented, add an integration test that:
1. Opens a TLS connection to the proxy
2. Sends no data (or an incomplete request)
3. Waits for `connection_idle_timeout_secs + 5s`
4. Verifies the connection is closed (socket FD released)

This prevents regressions in the idle-timeout cleanup. The test can use
`rcgen` for a self-signed cert (existing test pattern) and a raw `TcpStream`
to control the connection lifecycle.

---

## Deployment Drift Summary

The production server is running a binary built from `cfe0ae5`
(2026-06-15 05:23 UTC). HEAD is `f6e62a3` (2026-06-15 06:34 UTC). The 4
commits between them are:

| Commit | What it fixes | Deploy status |
|--------|---------------|---------------|
| `3ea3f56` | Admin socket → authenticated HTTP API (review #005 C1-C3, W1) | Not deployed |
| `c6dda71` | Config reload mtime TOCTOU + wildcard flag drift (review #005 W2, W5) | Not deployed |
| `143ebaa` | Tests for reload wildcard path | Not deployed |
| `f6e62a3` | Docs for admin HTTP API | Not deployed |

Additionally, the following source-level issues identified in this review are
**not yet fixed in HEAD** and require new code:

| Finding | Fix needed | Severity |
|---------|-----------|----------|
| C1 — no server idle timeout | keep_alive_timeout on builders | Critical |
| C2 — no concurrency cap | Semaphore on accept loop | Critical |
| W1 — no SIGHUP log reopen | SIGUSR1 handler + ArcSwap writer | Warning |
| W2 — no upstream pool max-idle | pool_max_idle_per_host on clients | Warning |
| W3 — no idle timeout config | StaticConfig field | Warning |
| W4 — no max_connections config | StaticConfig field | Warning |
| S1 — FD/connection metrics | health endpoint + periodic log | Suggestion |
| S5 — idle timeout integration test | new test | Suggestion |

## Recommended Migration Sequence

When the source fixes (C1, C2, W1-W4) are implemented and tested:

1. **Build new image** from HEAD+fixes
2. **Migrate config**:
   - `admin_socket_path` → `admin_key_path` (W5)
   - Add `connection_idle_timeout_secs = 60` (W3)
   - Add `max_connections = 1024` (W4)
   - Optionally tighten rate limit to `3 rps / burst 5` (S2)
3. **Create admin key** (if enabling admin API): `openssl rand -hex 32 > /etc/reverse-proxy/admin-key`
4. **Update docker-compose**: remove `/run/reverse-proxy` mount, add admin-key mount
5. **Update logrotate**: switch from `copytruncate` to `postrotate` + `kill -USR1 $(pidof reverse-proxy)` (W1)
6. **Deploy**: `docker compose up -d` (pulls new image, restarts with new config)
7. **Verify**: check health, FD count, fail2ban jails, make a test request
8. **Monitor**: watch the new FD/connection metrics (S1) for the first 24h

## References

- [Review #005](005-admin-socket-security-review.md) — admin socket vulnerabilities (fixed in HEAD by ADR-028)
- [Review #006](006-attack-surface-review.md) — attack surface enumeration (noted "no resource limiting" at 1.1)
- [ADR-028](../architecture/decisions/028-admin-http-api.md) — admin HTTP API replacement for Unix socket
- [ADR-029/030](../architecture/decisions/) — config reload TOCTOU and wildcard flag