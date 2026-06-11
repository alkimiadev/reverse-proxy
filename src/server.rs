use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::response::Response;
use axum::Router;
use hyper_util::rt::TokioExecutor;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower::Service;
use tracing::{error, info, warn};

pub struct InFlightCounter {
    count: AtomicUsize,
}

struct InFlightGuard(Arc<InFlightCounter>);

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        self.0.decrement();
    }
}

impl InFlightCounter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            count: AtomicUsize::new(0),
        })
    }

    pub fn increment(&self) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }

    pub fn decrement(&self) {
        self.count.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn is_zero(&self) -> bool {
        self.count.load(Ordering::SeqCst) == 0
    }
}

pub async fn serve_https_listener(
    tcp_listener: TcpListener,
    tls_acceptor: TlsAcceptor,
    router: Router,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    in_flight: Arc<InFlightCounter>,
) {
    let local_addr = tcp_listener.local_addr();

    loop {
        tokio::select! {
            accept_result = tcp_listener.accept() => {
                let (tcp_stream, remote_addr) = match accept_result {
                    Ok(conn) => conn,
                    Err(e) => {
                        error!(error = %e, "failed to accept TCP connection");
                        continue;
                    }
                };

                let tls_acceptor = tls_acceptor.clone();
                let router = router.clone();
                let in_flight = in_flight.clone();

                tokio::spawn(async move {
                    let _guard = InFlightGuard(in_flight.clone());

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
            _ = shutdown_rx.changed() => {
                if let Ok(addr) = local_addr {
                    info!(addr = %addr, "HTTPS listener shutting down");
                }
                break;
            }
        }
    }
}

/// Wait for in-flight connections to drain, with a timeout.
/// Returns the number of connections still in-flight when the timeout expired (0 if all drained).
pub async fn drain_in_flight(
    in_flight: &Arc<InFlightCounter>,
    timeout: std::time::Duration,
) -> usize {
    let start = std::time::Instant::now();
    loop {
        if in_flight.is_zero() {
            return 0;
        }
        if start.elapsed() >= timeout {
            return in_flight.count.load(Ordering::SeqCst);
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
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
