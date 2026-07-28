use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::response::Response;
use axum::Router;
use hyper::body::Incoming;
use hyper_util::rt::TokioExecutor;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;
use tower::Service;
use tracing::{error, info, warn};

const HTTP2_KEEP_ALIVE_INTERVAL_SECS: u64 = 15;

pub struct InFlightCounter {
    count: AtomicUsize,
}

struct InFlightGuard(Arc<InFlightCounter>);

impl InFlightGuard {
    fn new(counter: Arc<InFlightCounter>) -> Self {
        counter.increment();
        Self(counter)
    }
}

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
    connection_idle_timeout: Duration,
    max_connections: usize,
) {
    let local_addr = tcp_listener.local_addr();
    let conn_sem = Arc::new(Semaphore::new(max_connections));

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
                let conn_sem = conn_sem.clone();

                let permit = match conn_sem.acquire_owned().await {
                    Ok(permit) => permit,
                    Err(e) => {
                        error!(error = %e, "connection semaphore closed");
                        continue;
                    }
                };

                tokio::spawn(async move {
                    let _guard = InFlightGuard::new(in_flight.clone());
                    let _permit = permit;

                    let tls_stream = match tls_acceptor.accept(tcp_stream).await {
                        Ok(stream) => stream,
                        Err(e) => {
                            warn!(error = %e, "TLS handshake failed");
                            return;
                        }
                    };

                    let alpn = tls_stream.get_ref().1.alpn_protocol();
                    let is_h2 = alpn == Some(b"h2");

                    let svc = ConnectInfoService {
                        inner: router.into_service::<Incoming>(),
                        remote_addr,
                    };
                    let svc = TowerToHyperService::new(svc);

                    let io = hyper_util::rt::TokioIo::new(tls_stream);

                    if is_h2 {
                        let mut builder = hyper::server::conn::http2::Builder::new(TokioExecutor::new());
                        builder
                            .timer(hyper_util::rt::TokioTimer::new())
                            .keep_alive_interval(Some(Duration::from_secs(HTTP2_KEEP_ALIVE_INTERVAL_SECS)))
                            .keep_alive_timeout(connection_idle_timeout)
                            .enable_connect_protocol();
                        if let Err(e) = builder.serve_connection(io, svc).await
                        {
                            error!(error = %e, "HTTPS/2 connection error");
                        }
                    } else {
                        let mut builder = hyper_util::server::conn::auto::Builder::new(TokioExecutor::new());
                        builder
                            .http1()
                            .timer(hyper_util::rt::TokioTimer::new())
                            .header_read_timeout(Some(connection_idle_timeout));
                        builder
                            .http2()
                            .timer(hyper_util::rt::TokioTimer::new())
                            .keep_alive_interval(Some(Duration::from_secs(HTTP2_KEEP_ALIVE_INTERVAL_SECS)))
                            .keep_alive_timeout(connection_idle_timeout)
                            .enable_connect_protocol();
                        if let Err(e) = builder.serve_connection_with_upgrades(io, svc).await
                        {
                            if let Some(hyper_err) = e.downcast_ref::<hyper::Error>() {
                                if hyper_err.is_incomplete_message() {
                                    return;
                                }
                            }
                            error!(error = %e, "HTTPS connection error");
                        }
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
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[derive(Clone)]
struct ConnectInfoService<S> {
    inner: S,
    remote_addr: SocketAddr,
}

impl<S> Service<Request<Incoming>> for ConnectInfoService<S>
where
    S: Service<Request<Incoming>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
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

    fn call(&mut self, mut req: Request<Incoming>) -> Self::Future {
        req.extensions_mut().insert(ConnectInfo(self.remote_addr));
        self.inner.call(req)
    }
}
