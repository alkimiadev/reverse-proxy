use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tracing::{error, info, warn};

use reverse_proxy::admin::{start_admin_socket, AdminSocket, AdminSocketError};
use reverse_proxy::cli;
use reverse_proxy::config::ConfigReloadHandle;
use reverse_proxy::config::DynamicConfig;
use reverse_proxy::health;
use reverse_proxy::logging;
use reverse_proxy::proxy::{build_router, create_http_client, create_https_client, ProxyState};
use reverse_proxy::rate_limit::{start_eviction_task, RateLimiter};
use reverse_proxy::server::{drain_in_flight, serve_https_listener, InFlightCounter};
use reverse_proxy::shutdown::GracefulShutdown;
use reverse_proxy::tls::acceptor::{setup_tls, TlsMode};
use reverse_proxy::tls::redirect;

fn notify_systemd_ready() {
    if std::env::var("NOTIFY_SOCKET").is_ok() {
        match sd_notify::notify(true, &[sd_notify::NotifyState::Ready]) {
            Ok(()) => info!("sd_notify: READY=1 sent"),
            Err(e) => warn!("sd_notify: failed to notify systemd: {}", e),
        }
    }
}

fn main() {
    let args = cli::parse();

    if args.validate {
        match cli::run_validate(&args) {
            Ok(()) => std::process::exit(0),
            Err(_) => std::process::exit(1),
        }
    }

    let loaded_config = match cli::load_config(&args) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    };

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");

    rt.block_on(async move {
        if let Err(e) = run_server(loaded_config, &args.config).await {
            error!("fatal error: {e:#}");
            std::process::exit(1);
        }
    });
}

async fn run_server(loaded_config: cli::LoadedConfig, config_path: &str) -> Result<()> {
    logging::init(&loaded_config.static_config.logging).context("failed to initialize logging")?;

    info!("reverse-proxy starting");

    let dynamic_config: DynamicConfig = loaded_config.dynamic_config;
    let config_arc = Arc::new(ArcSwap::from_pointee(dynamic_config));

    let shutdown = Arc::new(GracefulShutdown::new(
        loaded_config.static_config.shutdown_timeout_secs,
    ));

    let rate_limiter = Arc::new(RateLimiter::new(config_arc.clone()));

    let http_client = create_http_client();
    let https_client = create_https_client();

    let proxy_state = Arc::new(ProxyState {
        config: config_arc.clone(),
        http_client,
        https_client,
    });

    let reload_handle = Arc::new(ConfigReloadHandle::new(
        config_arc.clone(),
        loaded_config.static_config.clone(),
    ));

    reverse_proxy::shutdown::register_signal_handlers(
        shutdown.clone(),
        reload_handle.clone(),
        config_path.to_string(),
    )?;

    if loaded_config.static_config.health_check_port > 0 {
        let (health_addr, _health_handle) =
            health::start_health_check_listener(loaded_config.static_config.health_check_port)
                .await
                .context("failed to bind health check port")?;
        info!(addr = %health_addr, "Health check listener bound");
    }

    if !loaded_config.static_config.admin_socket_path.is_empty() {
        let admin_socket = Arc::new(AdminSocket::new(
            loaded_config.static_config.admin_socket_path.clone(),
            reload_handle.clone(),
            config_path.to_string(),
        ));
        let admin_socket_clone = admin_socket.clone();
        let shutdown_for_admin = shutdown.clone();
        tokio::spawn(async move {
            if let Err(e) = start_admin_socket(admin_socket_clone, shutdown_for_admin).await {
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
                    _ => {}
                }
            }
        });
    }

    let mut bound_listeners = Vec::new();

    for listener_config in &loaded_config.static_config.listeners {
        if listener_config.http_port > 0 {
            let (http_addr, _http_handle) = redirect::start_http_redirect_listener(listener_config)
                .await
                .context(format!(
                    "failed to bind HTTP redirect listener on {}:{}",
                    listener_config.bind_addr, listener_config.http_port
                ))?;
            info!(addr = %http_addr, "HTTP redirect listener bound");
        }

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

        bound_listeners.push((listener_config.clone(), https_tcp));
    }

    let mut tls_acceptors = Vec::new();
    for (listener_config, _) in &bound_listeners {
        let tls_mode = setup_tls(&listener_config.tls, shutdown.clone()).context(format!(
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
            TlsMode::Acme { default_config, .. } => {
                let acceptor = TlsAcceptor::from(default_config);
                tls_acceptors.push(acceptor);
                info!(
                    addr = %listener_config.bind_addr,
                    "ACME TLS configured"
                );
            }
            _ => {
                warn!(
                    addr = %listener_config.bind_addr,
                    "unsupported TLS mode"
                );
            }
        }
    }

    let _eviction_handle = start_eviction_task(
        rate_limiter.clone(),
        std::time::Duration::from_secs(60),
        std::time::Duration::from_secs(300),
        shutdown.subscribe(),
    );

    let app = build_router(proxy_state.clone(), config_arc.clone(), rate_limiter);

    let in_flight = InFlightCounter::new();

    let mut https_server_handles = Vec::new();

    for ((listener_config, tcp_listener), tls_acceptor) in
        bound_listeners.into_iter().zip(tls_acceptors.into_iter())
    {
        let shutdown_rx = shutdown.subscribe();

        let handle = tokio::spawn(serve_https_listener(
            tcp_listener,
            tls_acceptor,
            app.clone(),
            shutdown_rx,
            in_flight.clone(),
        ));

        info!(
            bind_addr = %listener_config.bind_addr,
            https_port = listener_config.https_port,
            "HTTPS listener accepting connections"
        );

        https_server_handles.push(handle);
    }

    info!("all listeners started");
    notify_systemd_ready();

    let mut shutdown_rx = shutdown.subscribe();
    shutdown_rx
        .changed()
        .await
        .map_err(|_| anyhow::anyhow!("shutdown channel error"))?;

    info!("shutdown signal received, starting graceful shutdown");

    let shutdown_timeout = shutdown.shutdown_timeout();
    for handle in https_server_handles {
        let result = tokio::time::timeout(shutdown_timeout, handle).await;
        if result.is_err() {
            warn!("shutdown timeout expired, aborting listener task");
        }
    }

    let remaining = drain_in_flight(&in_flight, shutdown.shutdown_timeout()).await;
    if remaining > 0 {
        warn!(
            remaining = remaining,
            "shutdown timeout expired, forcing exit"
        );
    } else {
        info!("all in-flight requests completed");
    }

    Ok(())
}
