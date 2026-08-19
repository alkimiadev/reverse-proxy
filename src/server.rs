use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::response::Response;
use axum::Router;
use http_body::Frame;
use hyper::body::Incoming;
use hyper_util::rt::TokioExecutor;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio_rustls::TlsAcceptor;
use tower::Service;
use tracing::{debug, error, info, warn};

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

    pub fn count(&self) -> usize {
        self.count.load(Ordering::SeqCst)
    }
}

#[derive(Debug)]
struct IdleState {
    last_activity: Mutex<Instant>,
    in_flight: AtomicUsize,
}

impl IdleState {
    fn new() -> Self {
        Self {
            last_activity: Mutex::new(Instant::now()),
            in_flight: AtomicUsize::new(0),
        }
    }

    fn touch(&self) {
        *self.last_activity.lock().unwrap() = Instant::now();
    }

    fn idle_for(&self) -> Duration {
        Instant::now().duration_since(*self.last_activity.lock().unwrap())
    }

    fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
struct IdleTrackingService<S> {
    inner: S,
    idle_state: Arc<IdleState>,
}

struct IdleTrackingBody<B> {
    inner: Option<B>,
    idle_state: Arc<IdleState>,
    decremented: AtomicBool,
}

impl<B> IdleTrackingBody<B> {
    fn new(inner: B, idle_state: Arc<IdleState>) -> Self {
        Self {
            inner: Some(inner),
            idle_state,
            decremented: AtomicBool::new(false),
        }
    }

    fn release(&self) {
        if !self.decremented.swap(true, Ordering::SeqCst) {
            self.idle_state.in_flight.fetch_sub(1, Ordering::SeqCst);
            self.idle_state.touch();
        }
    }
}

impl<B> Drop for IdleTrackingBody<B> {
    fn drop(&mut self) {
        self.release();
    }
}

impl<B> http_body::Body for IdleTrackingBody<B>
where
    B: http_body::Body + Unpin + Send + 'static,
{
    type Data = B::Data;
    type Error = B::Error;

    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        let inner = match this.inner.as_mut() {
            Some(b) => b,
            None => return Poll::Ready(None),
        };

        match Pin::new(inner).poll_frame(cx) {
            Poll::Ready(None) => {
                this.release();
                Poll::Ready(None)
            }
            Poll::Ready(Some(Err(e))) => {
                this.release();
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(Some(Ok(frame))) => {
                this.idle_state.touch();
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner
            .as_ref()
            .map(|b| b.is_end_stream())
            .unwrap_or(true)
    }

    fn size_hint(&self) -> http_body::SizeHint {
        self.inner
            .as_ref()
            .map(|b| b.size_hint())
            .unwrap_or_default()
    }
}

impl<S> Service<Request<Incoming>> for IdleTrackingService<S>
where
    S: Service<Request<Incoming>, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = std::pin::Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<Incoming>) -> Self::Future {
        self.idle_state.touch();
        self.idle_state.in_flight.fetch_add(1, Ordering::SeqCst);

        let inner_fut = self.inner.call(req);
        let idle_state = self.idle_state.clone();

        Box::pin(async move {
            let result = inner_fut.await;
            match result {
                Ok(resp) => {
                    let (parts, body) = resp.into_parts();
                    let tracked = IdleTrackingBody::new(body, idle_state.clone());
                    let new_body = Body::new(tracked);
                    Ok(Response::from_parts(parts, new_body))
                }
                Err(e) => {
                    idle_state.in_flight.fetch_sub(1, Ordering::SeqCst);
                    idle_state.touch();
                    Err(e)
                }
            }
        })
    }
}

async fn idle_watchdog(idle_state: Arc<IdleState>, timeout: Duration) {
    loop {
        let idle_for = idle_state.idle_for();
        let remaining = timeout.saturating_sub(idle_for);

        let sleep_dur = if idle_state.in_flight() > 0 {
            timeout
        } else if !remaining.is_zero() {
            remaining
        } else {
            return;
        };

        tokio::time::sleep(sleep_dur).await;

        if idle_state.in_flight() == 0 && idle_state.idle_for() >= timeout {
            return;
        }
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

                    let idle_state = Arc::new(IdleState::new());

                    let svc = ConnectInfoService {
                        inner: router.into_service::<Incoming>(),
                        remote_addr,
                    };
                    let svc = IdleTrackingService {
                        inner: svc,
                        idle_state: idle_state.clone(),
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

                        tokio::select! {
                            result = builder.serve_connection(io, svc) => {
                                if let Err(e) = result {
                                    error!(error = %e, "HTTPS/2 connection error");
                                }
                            }
                            _ = idle_watchdog(idle_state.clone(), connection_idle_timeout) => {
                                debug!(
                                    remote_addr = %remote_addr,
                                    idle_timeout_secs = connection_idle_timeout.as_secs(),
                                    "closing idle HTTP/2 connection (no real request activity)"
                                );
                            }
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

                        tokio::select! {
                            result = builder.serve_connection_with_upgrades(io, svc) => {
                                if let Err(e) = result {
                                    if let Some(hyper_err) = e.downcast_ref::<hyper::Error>() {
                                        if hyper_err.is_incomplete_message() {
                                            return;
                                        }
                                    }
                                    error!(error = %e, "HTTPS connection error");
                                }
                            }
                            _ = idle_watchdog(idle_state.clone(), connection_idle_timeout) => {
                                debug!(
                                    remote_addr = %remote_addr,
                                    idle_timeout_secs = connection_idle_timeout.as_secs(),
                                    "closing idle connection (no real request activity)"
                                );
                            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn idle_state_starts_not_idle() {
        let state = IdleState::new();
        assert!(state.idle_for() < Duration::from_millis(100));
        assert_eq!(state.in_flight(), 0);
    }

    #[test]
    fn idle_state_touch_updates_last_activity() {
        let state = IdleState::new();
        std::thread::sleep(Duration::from_millis(50));
        assert!(state.idle_for() >= Duration::from_millis(50));

        state.touch();
        assert!(state.idle_for() < Duration::from_millis(10));
    }

    #[test]
    fn idle_state_in_flight_tracks_increments_and_decrements() {
        let state = IdleState::new();
        assert_eq!(state.in_flight(), 0);
        state.in_flight.fetch_add(1, Ordering::SeqCst);
        assert_eq!(state.in_flight(), 1);
        state.in_flight.fetch_add(1, Ordering::SeqCst);
        assert_eq!(state.in_flight(), 2);
        state.in_flight.fetch_sub(1, Ordering::SeqCst);
        assert_eq!(state.in_flight(), 1);
    }

    #[tokio::test]
    async fn idle_watchdog_fires_after_timeout_when_idle() {
        let state = Arc::new(IdleState::new());
        let timeout = Duration::from_millis(50);

        let start = Instant::now();
        idle_watchdog(state.clone(), timeout).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed >= timeout,
            "watchdog fired before timeout: {:?} < {:?}",
            elapsed,
            timeout
        );
        assert!(
            elapsed < timeout + Duration::from_millis(100),
            "watchdog took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn idle_watchdog_does_not_fire_while_request_in_flight() {
        let state = Arc::new(IdleState::new());
        let timeout = Duration::from_millis(50);

        state.in_flight.fetch_add(1, Ordering::SeqCst);

        let watchdog = tokio::time::timeout(
            Duration::from_millis(200),
            idle_watchdog(state.clone(), timeout),
        );

        let result = watchdog.await;
        assert!(
            result.is_err(),
            "watchdog fired while a request was in-flight (should have kept sleeping)"
        );
        assert_eq!(state.in_flight(), 1);
    }

    #[tokio::test]
    async fn idle_watchdog_resets_after_request_completes() {
        let state = Arc::new(IdleState::new());
        let timeout = Duration::from_millis(50);

        state.in_flight.fetch_add(1, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(30));

        state.in_flight.fetch_sub(1, Ordering::SeqCst);
        state.touch();

        let start = Instant::now();
        idle_watchdog(state.clone(), timeout).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed >= timeout,
            "watchdog fired before full timeout after touch(): {:?} < {:?}",
            elapsed,
            timeout
        );
    }

    #[tokio::test]
    async fn idle_watchdog_rechecks_after_short_sleep_when_partially_idle() {
        let state = Arc::new(IdleState::new());
        let timeout = Duration::from_millis(100);

        std::thread::sleep(Duration::from_millis(40));

        let start = Instant::now();
        idle_watchdog(state.clone(), timeout).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(60),
            "watchdog fired too soon after partial idle: {:?}",
            elapsed
        );
        assert!(
            elapsed < Duration::from_millis(200),
            "watchdog took too long: {:?}",
            elapsed
        );
    }

    #[tokio::test]
    async fn idle_watchdog_never_fires_with_persistent_in_flight() {
        let state = Arc::new(IdleState::new());
        let timeout = Duration::from_millis(50);

        state.in_flight.fetch_add(1, Ordering::SeqCst);

        let result = tokio::time::timeout(
            Duration::from_millis(300),
            idle_watchdog(state.clone(), timeout),
        )
        .await;

        assert!(result.is_err(), "watchdog should not fire with in-flight > 0");
    }
}