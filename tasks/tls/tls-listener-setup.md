---
id: tls/tls-listener-setup
name: Implement multi-listener TLS setup with ConnectInfo propagation and per-listener routers
status: complete
depends_on: [tls/manual-tls, tls/acme-tls, config/static-config, config/dynamic-config]
scope: broad
risk: high
impact: phase
level: implementation
---

## Description

Wire up the TLS listeners — this is the core integration task that brings together manual TLS, ACME TLS, and the config system to create running TLS listeners.

For each `ListenerConfig`:
1. Bind TCP listener on `bind_addr:https_port`
2. Construct the appropriate `ServerConfig` (manual or ACME)
3. Create `tokio_rustls::TlsAcceptor` from the `ServerConfig`
4. Accept connections, extract `peer_addr()` before wrapping in TLS
5. Create a per-listener `axum::Router` with its middleware stack
6. Provide `ConnectInfo<SocketAddr>` to the router via `into_make_service_with_connect_info::<SocketAddr>()`

### ConnectInfo Propagation

`ConnectInfo<SocketAddr>` is critical for the proxy handler — it provides the real client IP for `X-Real-IP` and `X-Forwarded-For` headers. The peer address must be extracted from the `TcpStream` before wrapping in `TlsStream`.

### Per-Listener Routers

Each listener has its own `axum::Router` instance with its own middleware stack. All routers share `Arc<ArcSwap<DynamicConfig>>` and `Arc<Mutex<HashMap<IpAddr, TokenBucket>>>` via axum State.

### Startup Sequence

The TLS listener setup follows the startup sequence from operations.md:
1. Parse and validate config
2. Initialize DynamicConfig in ArcSwap
3. Initialize shared state (rate limiter, hyper client, logging)
4. Bind health check port
5. Bind admin socket
6. Bind all listener ports (TCP bind)
7. Load TLS configuration (manual certs or ACME init)
8. Start TCP listeners
9. Start background tasks (ACME renewal, rate limiter eviction, signal handler, admin socket)

Fail-fast if any bind or TLS load fails.

### Health Endpoint on HTTPS

When the local health check port is enabled, `/health` is also available on the HTTPS listener(s) as a fallback for TLS-level health verification.

## Acceptance Criteria

- [ ] Multi-listener setup: each `ListenerConfig` creates its own TCP listener + TLS acceptor
- [ ] `ConnectInfo<SocketAddr>` populated from `TcpStream::peer_addr()` before TLS wrapping
- [ ] Per-listener `axum::Router` instances sharing `Arc<ArcSwap<DynamicConfig>>` state
- [ ] Both manual and ACME TLS modes work for different listeners
- [ ] Fail-fast behavior: if any bind or TLS load fails, exit with non-zero code
- [ ] All ports bound before any connections accepted
- [ ] `/health` endpoint available on HTTPS listener(s)
- [ ] `sd_notify("READY=1")` sent after all listeners started (systemd integration)
- [ ] Integration test: start proxy with test config, verify HTTPS listener accepts connections
- [ ] Integration test: multi-listener config with both manual and ACME listeners

## References

- docs/architecture/tls.md — multi-listener architecture, ConnectInfo
- docs/architecture/proxy.md — Host-based routing, ConnectInfo propagation
- docs/architecture/operations.md — startup sequence, health check
- docs/architecture/config.md — ListenerConfig, StaticConfig

## Notes

> This task is the critical integration point. It depends on manual TLS, ACME TLS, static config, and dynamic config all being complete. The implementation agent should wire these together carefully, following the startup sequence in operations.md.

## Summary

> To be filled on completion