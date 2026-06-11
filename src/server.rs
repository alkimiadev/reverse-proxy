use std::net::SocketAddr;

use axum::extract::ConnectInfo;
use axum::http::Request;
use axum::response::Response;
use axum::Router;
use hyper_util::rt::TokioExecutor;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tower::Service;
use tracing::{error, warn};

pub async fn serve_https_listener(
    tcp_listener: TcpListener,
    tls_acceptor: TlsAcceptor,
    router: Router,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
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
            _ = shutdown_rx.changed() => {
                if let Ok(addr) = local_addr {
                    tracing::info!(addr = %addr, "HTTPS listener shutting down");
                }
                break;
            }
        }
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
