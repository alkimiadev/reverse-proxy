use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::response::Response;
use hyper_util::rt::TokioExecutor;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower::Service;
use tracing::{error, info, warn};

use crate::admin::{start_admin_socket, AdminSocket, AdminSocketError};
use crate::config::dynamic_config::DynamicConfig;
use crate::config::static_config::StaticConfig;
use crate::config::ConfigReloadHandle;
use crate::health;
use crate::logging;
use crate::proxy::{create_http_client, create_https_client, proxy_router, ProxyState};
use crate::rate_limit::{start_eviction_task, RateLimiter};
use crate::tls::acceptor::{setup_tls, TlsMode};
use crate::tls::redirect;

fn notify_systemd_ready() {
    if std::env::var("NOTIFY_SOCKET").is_ok() {
        match sd_notify::notify(true, &[sd_notify::NotifyState::Ready]) {
            Ok(()) => info!("sd_notify: READY=1 sent"),
            Err(e) => warn!("sd_notify: failed to notify systemd: {}", e),
        }
    }
}

pub async fn run(static_config: StaticConfig, dynamic_config: DynamicConfig) -> Result<()> {
    logging::init(&static_config.logging).context("failed to initialize logging")?;

    info!("reverse-proxy starting");

    let dynamic_config = Arc::new(ArcSwap::from_pointee(dynamic_config));

    let http_client = create_http_client();
    let https_client = create_https_client();

    let rate_limiter = Arc::new(RateLimiter::new(dynamic_config.clone()));

    let proxy_state = Arc::new(ProxyState {
        config: dynamic_config.clone(),
        http_client,
        https_client,
    });

    if static_config.health_check_port > 0 {
        let (health_addr, _health_handle) =
            health::start_health_check_listener(static_config.health_check_port)
                .await
                .context("failed to bind health check port")?;
        info!(addr = %health_addr, "Health check listener started");
    }

    let reload_handle = Arc::new(ConfigReloadHandle::new(
        dynamic_config.clone(),
        static_config.clone(),
    ));

    if !static_config.admin_socket_path.is_empty() {
        let admin_socket = Arc::new(AdminSocket::new(
            static_config.admin_socket_path.clone(),
            reload_handle.clone(),
            std::env::args().next().unwrap_or_default(),
        ));
        let admin_socket_clone = admin_socket.clone();
        tokio::spawn(async move {
            if let Err(e) = start_admin_socket(admin_socket_clone).await {
                match e {
                    AdminSocketError::Disabled => {}
                    AdminSocketError::SocketInUse(path) => {
                        warn!("admin socket disabled: {} is in use", path);
                    }
                    AdminSocketError::BindFailed(msg) => {
                        error!("admin socket bind failed: {}", msg);
                    }
                    AdminSocketError::Io(e) => {
                        error!("admin socket IO error: {}", e);
                    }
                }
            }
        });
    }

    let _eviction_handle = start_eviction_task(
        rate_limiter.clone(),
        std::time::Duration::from_secs(60),
        std::time::Duration::from_secs(300),
    );

    let mut bound_https_listeners = Vec::new();

    for listener_config in &static_config.listeners {
        let https_addr: SocketAddr = format!(
            "{}:{}",
            listener_config.bind_addr, listener_config.https_port
        )
        .parse()
        .context(format!(
            "invalid HTTPS bind address {}:{}",
            listener_config.bind_addr, listener_config.https_port
        ))?;

        let https_tcp = TcpListener::bind(https_addr).await.context(format!(
            "failed to bind HTTPS listener on {}:{}",
            listener_config.bind_addr, listener_config.https_port
        ))?;

        let local_addr = https_tcp.local_addr()?;
        info!(addr = %local_addr, "HTTPS listener bound");

        bound_https_listeners.push((listener_config.clone(), https_tcp));
    }

    for listener_config in &static_config.listeners {
        if listener_config.http_port > 0 {
            let (http_addr, _http_handle) = redirect::start_http_redirect_listener(listener_config)
                .await
                .context(format!(
                    "failed to start HTTP redirect listener for {}:{}",
                    listener_config.bind_addr, listener_config.http_port
                ))?;
            info!(addr = %http_addr, "HTTP redirect listener started");
        }
    }

    let mut tls_acceptors = Vec::new();
    for (listener_config, _) in &bound_https_listeners {
        let tls_mode = setup_tls(&listener_config.tls).context(format!(
            "failed to setup TLS for listener {}",
            listener_config.bind_addr
        ))?;

        match tls_mode {
            TlsMode::Manual(server_config) => {
                let acceptor = TlsAcceptor::from(server_config);
                tls_acceptors.push(acceptor);
                info!(
                    addr = %listener_config.bind_addr,
                    "Manual TLS configured"
                );
            }
            TlsMode::Acme {
                default_config,
                challenge_config: _,
                resolver: _,
            } => {
                let acceptor = TlsAcceptor::from(default_config);
                tls_acceptors.push(acceptor);
                info!(
                    addr = %listener_config.bind_addr,
                    "ACME TLS configured"
                );
            }
        }
    }

    for ((listener_config, tcp_listener), tls_acceptor) in bound_https_listeners
        .into_iter()
        .zip(tls_acceptors.into_iter())
    {
        let state = proxy_state.clone();

        tokio::spawn(serve_https_listener(tcp_listener, tls_acceptor, state));

        info!(
            bind_addr = %listener_config.bind_addr,
            https_port = listener_config.https_port,
            "HTTPS listener accepting connections"
        );
    }

    info!("all listeners started");
    notify_systemd_ready();

    tokio::signal::ctrl_c()
        .await
        .context("failed to listen for ctrl-c")?;
    info!("shutting down");

    Ok(())
}

async fn serve_https_listener(
    tcp_listener: TcpListener,
    tls_acceptor: TlsAcceptor,
    state: Arc<ProxyState>,
) {
    let router = proxy_router(state);

    loop {
        let (tcp_stream, remote_addr) = match tcp_listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                error!(error = %e, "failed to accept TCP connection");
                continue;
            }
        };

        let tls_acceptor = tls_acceptor.clone();
        let router = router.clone();

        tokio::spawn(async move {
            let tls_stream = match tls_acceptor.accept(tcp_stream).await {
                Ok(stream) => stream,
                Err(e) => {
                    warn!(error = %e, "TLS handshake failed");
                    return;
                }
            };

            let svc = ConnectInfoService {
                inner: router.into_service::<hyper::body::Incoming>(),
                remote_addr,
            };

            let svc = TowerToHyperService::new(svc);

            let io = hyper_util::rt::TokioIo::new(tls_stream);

            if let Err(e) = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                .serve_connection_with_upgrades(io, svc)
                .await
            {
                if e.to_string().contains("incomplete message") {
                    return;
                }
                error!(error = %e, "HTTPS connection error");
            }
        });
    }
}

#[derive(Clone)]
struct ConnectInfoService<S> {
    inner: S,
    remote_addr: SocketAddr,
}

impl<S, B> Service<Request<B>> for ConnectInfoService<S>
where
    S: Service<Request<B>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<B>) -> Self::Future {
        req.extensions_mut().insert(ConnectInfo(self.remote_addr));
        self.inner.call(req)
    }
}
