# Threat Landscape

## Active Nginx Vulnerabilities (May 2026)

All disclosed by DepthFirst's autonomous security analysis. Four related CVEs from a single audit, plus additional ones discovered separately.

### Critical

**CVE-2026-42945 (CVSS 9.2) — "NGINX Rift"**
- Heap buffer overflow in `ngx_http_rewrite_module`, present since 2008 (18 years)
- Unauthenticated RCE via `rewrite` + `set` directives
- Working PoC publicly released on GitHub
- **Actively exploited in the wild** within 3 days of disclosure
- Our config uses `rewrite`-equivalent logic (HTTP→HTTPS redirect)
- Affects 0.6.27–1.30.0, fixed in 1.31.0/1.30.1
- **We are vulnerable** (running 1.24.0)

### High

**CVE-2026-42946 (CVSS 8.3)**
- Buffer overread in `ngx_http_scgi_module` and `ngx_http_uwsgi_module`
- Worker crash or memory disclosure
- Excessive memory allocation attack (can trigger ~1TB allocation)
- Affects 0.8.42–1.30.0, fixed in 1.31.0/1.30.1
- **We are vulnerable** (running 1.24.0, though we don't use scgi/uwsgi)

### Medium

**CVE-2026-40701**
- Use-after-free in OCSP resolver
- Limited data modification or worker restart
- Affects 1.19.0–1.30.0, fixed in 1.31.0/1.30.1
- **We are vulnerable** (running 1.24.0)

**CVE-2026-9256**
- Buffer overflow in `ngx_http_rewrite_module` (separate from Rift)
- Affects 0.1.17–1.31.0, fixed in 1.31.1+
- **We are vulnerable** (running 1.24.0)

**CVE-2026-42926**
- HTTP/2 request injection in `ngx_http_proxy_module`
- Affects 1.29.4–1.30.0, fixed in 1.31.0/1.30.1
- We are not directly vulnerable (1.24.0 is outside range)

**CVE-2026-40460**
- HTTP/3 address spoofing
- Affects 1.25.0–1.30.0
- We are not directly vulnerable (1.24.0 is outside range)

### Low

**CVE-2026-42934**
- Buffer overread in `ngx_http_charset_module`
- Affects 0.3.50–1.30.0, fixed in 1.31.0/1.30.1
- **We are vulnerable** (running 1.24.0)

## Unreleased Vulnerabilities

Security researchers in relevant communities report at least 4 additional RCE vulnerabilities in nginx that have not yet been publicly disclosed. Researchers are expressing frustration with F5/nginx's slow response times and are considering public disclosure to force action.

This means the known CVEs above are likely just the tip of the iceberg.

## Risk Assessment

| Factor | Level | Notes |
|--------|-------|-------|
| Current exposure | **Critical** | Actively exploited RCE in our nginx version |
| Patch availability | **Available** | 1.30.1/1.31.0+ fix all known CVEs, but requires manual upgrade from Ubuntu default |
| Future risk | **High** | More undisclosed vulns likely; C codebase with systemic memory safety issues |
| Mitigation urgency | **Immediate** | RCE with public PoC and active exploitation |

## Why Rust Helps

- Memory safety by construction eliminates: buffer overflows, use-after-free, double-free, out-of-bounds reads/writes
- This is the **exact class of bugs** affecting nginx right now (6 out of 7 recent CVEs are memory corruption)
- rustls (pure Rust TLS) avoids OpenSSL dependency and its own CVE history
- Does NOT eliminate logic bugs — still need careful rate limiting, header injection, access control
- But provides a fundamentally safer baseline to build on

## Short-term Mitigation (While Developing Replacement)

1. Upgrade nginx to 1.30.1+ or 1.31.1+ immediately
2. Consider removing rewrite directives if possible
3. Ensure fail2ban is actively monitoring
4. Firewall restrictions on port 80/443 if feasible
5. Prioritize the Rust proxy project