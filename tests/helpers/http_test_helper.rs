use std::net::SocketAddr;
use std::time::Duration;

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

    /// Upstream that emits one chunk every `chunk_interval` for `num_chunks`
    /// chunks, then ends. The body stream outlasts the idle timeout so the
    /// watchdog's behaviour for in-progress streaming responses is observable.
    /// Emits `b"<n>"` per chunk so the client can verify it received the whole
    /// stream.
    pub async fn spawn_slow_stream(chunk_interval: Duration, num_chunks: usize) -> Self {
        Self::spawn(move || {
            let interval = chunk_interval;
            let n = num_chunks;
            Router::new().route(
                "/",
                get(move || async move {
                    let (tx, rx) = tokio::sync::mpsc::channel::<
                        Result<axum::body::Bytes, std::convert::Infallible>,
                    >(16);
                    tokio::spawn(async move {
                        for i in 0..n {
                            tokio::time::sleep(interval).await;
                            let _ = tx
                                .send(Ok(format!("<{i}>").into_bytes().into()))
                                .await;
                        }
                    });
                    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
                    axum::body::Body::from_stream(stream)
                }),
            )
        })
        .await
    }
}
