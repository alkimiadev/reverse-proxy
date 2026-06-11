use std::sync::Arc;

use arc_swap::ArcSwap;
use reverse_proxy::admin::{start_admin_socket, AdminSocket};
use reverse_proxy::cli;
use reverse_proxy::config::{ConfigReloadHandle, DynamicConfig};
use reverse_proxy::health::start_health_check_listener;
use reverse_proxy::proxy::{create_http_client, create_https_client, proxy_router, ProxyState};
use reverse_proxy::rate_limit::{start_eviction_task, RateLimiter};
use reverse_proxy::shutdown::GracefulShutdown;
use reverse_proxy::tls::redirect::start_http_redirect_listener;
use tokio::net::TcpListener;
use tracing::info;

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
            tracing::error!("fatal error: {e:#}");
            std::process::exit(1);
        }
    });
}

async fn run_server(loaded_config: cli::LoadedConfig, config_path: &str) -> anyhow::Result<()> {
    let shutdown = Arc::new(GracefulShutdown::new(
        loaded_config.static_config.shutdown_timeout_secs,
    ));

    let dynamic_config: DynamicConfig = loaded_config.dynamic_config;
    let config_arc = Arc::new(ArcSwap::from_pointee(dynamic_config));
    let reload_handle = Arc::new(ConfigReloadHandle::new(
        config_arc.clone(),
        loaded_config.static_config.clone(),
    ));

    reverse_proxy::logging::init(&loaded_config.static_config.logging)?;

    info!("reverse-proxy starting");

    reverse_proxy::shutdown::register_signal_handlers(
        shutdown.clone(),
        reload_handle.clone(),
        config_path.to_string(),
    )?;

    let rate_limiter = Arc::new(RateLimiter::new(config_arc.clone()));

    let proxy_state = Arc::new(ProxyState {
        config: config_arc.clone(),
        http_client: create_http_client(),
        https_client: create_https_client(),
    });

    let mut server_handles: Vec<tokio::task::JoinHandle<anyhow::Result<()>>> = Vec::new();
    let mut tcp_listeners: Vec<TcpListener> = Vec::new();

    if loaded_config.static_config.health_check_port > 0 {
        let (addr, handle) =
            start_health_check_listener(loaded_config.static_config.health_check_port).await?;
        info!(addr = %addr, "Health check listener started");
        server_handles.push(handle);
    }

    let admin_socket = Arc::new(AdminSocket::new(
        loaded_config.static_config.admin_socket_path.clone(),
        reload_handle.clone(),
        config_path.to_string(),
    ));

    let admin_handle = tokio::spawn(start_admin_socket(admin_socket));

    let eviction_handle = start_eviction_task(
        rate_limiter.clone(),
        std::time::Duration::from_secs(60),
        std::time::Duration::from_secs(300),
    );

    for listener_config in &loaded_config.static_config.listeners {
        if listener_config.http_port > 0 {
            let (addr, handle) = start_http_redirect_listener(listener_config).await?;
            info!(addr = %addr, "HTTP redirect listener started");
            server_handles.push(handle);
        }

        let https_bind_addr: std::net::SocketAddr = format!(
            "{}:{}",
            listener_config.bind_addr, listener_config.https_port
        )
        .parse()
        .map_err(|e| {
            anyhow::anyhow!(
                "invalid bind address {}:{}: {}",
                listener_config.bind_addr,
                listener_config.https_port,
                e
            )
        })?;

        let tcp_listener = TcpListener::bind(https_bind_addr).await?;
        let local_addr = tcp_listener.local_addr()?;
        info!(addr = %local_addr, "HTTPS listener bound");
        tcp_listeners.push(tcp_listener);
    }

    let app = proxy_router(proxy_state);
    let app = reverse_proxy::proxy::router_with_body_limit(app, config_arc);

    let mut https_server_handles = Vec::new();
    for tcp_listener in tcp_listeners {
        let shutdown_rx = shutdown.subscribe();
        let handle = tokio::spawn(serve_with_graceful_shutdown(
            tcp_listener,
            app.clone(),
            shutdown_rx,
        ));
        https_server_handles.push(handle);
    }

    info!("reverse-proxy ready");

    let mut shutdown_rx = shutdown.subscribe();
    shutdown_rx
        .changed()
        .await
        .map_err(|_| anyhow::anyhow!("shutdown channel error"))?;

    info!("shutdown signal received, starting graceful shutdown");

    drop(https_server_handles);

    for handle in server_handles {
        handle.abort();
    }

    admin_handle.abort();
    eviction_handle.abort();

    info!("all connections closed, exiting");
    std::process::exit(0);
}

async fn serve_with_graceful_shutdown(
    listener: TcpListener,
    app: axum::Router,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let local_addr = listener.local_addr()?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_rx.changed().await.ok();
            info!(addr = %local_addr, "HTTPS server shutting down");
        })
        .await
        .map_err(anyhow::Error::from)
}
