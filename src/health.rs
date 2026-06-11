use std::net::SocketAddr;

use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tokio::net::TcpListener;
use tracing::info;

async fn health_handler() -> impl IntoResponse {
    axum::http::StatusCode::OK
}

pub fn health_router() -> Router {
    Router::new().route("/health", get(health_handler))
}

pub async fn start_health_check_listener(
    port: u16,
) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    info!(
        addr = %local_addr,
        "Health check listener bound"
    );

    let handle = tokio::spawn(async move {
        axum::serve(listener, health_router())
            .await
            .map_err(anyhow::Error::from)
    });

    Ok((local_addr, handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check_returns_200() {
        let (addr, handle) = start_health_check_listener(0).await.unwrap();

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{}/health", addr.port()))
            .send()
            .await
            .unwrap();

        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body = resp.text().await.unwrap();
        assert!(body.is_empty());

        handle.abort();
    }

    #[tokio::test]
    async fn test_health_check_binds_to_localhost() {
        let (addr, _handle) = start_health_check_listener(0).await.unwrap();
        assert!(addr.ip().is_loopback());
    }
}
