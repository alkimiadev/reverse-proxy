use std::net::SocketAddr;

use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;

pub struct TestUpstream {
    pub addr: SocketAddr,
    pub shutdown_tx: tokio::sync::oneshot::Sender<()>,
}

impl TestUpstream {
    pub async fn spawn<F>(handler_factory: F) -> Self
    where
        F: FnOnce() -> Router,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let app = handler_factory();
        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .unwrap();
        });

        Self { addr, shutdown_tx }
    }

    pub async fn spawn_ok() -> Self {
        Self::spawn(|| Router::new().route("/", get(|| async { "ok" }))).await
    }

    #[allow(dead_code)]
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    #[allow(dead_code)]
    pub fn upstream_addr(&self) -> String {
        format!("127.0.0.1:{}", self.addr.port())
    }
}
