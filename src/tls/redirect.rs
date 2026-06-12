use std::net::SocketAddr;

use axum::extract::Request;
use axum::http::header::{HeaderName, HOST, LOCATION};
use axum::http::{HeaderValue, StatusCode};
use axum::response::IntoResponse;
use axum::routing::any;
use axum::Router;
use tokio::net::TcpListener;
use tracing::info;

use crate::config::static_config::ListenerConfig;

const ACME_CHALLENGE_PREFIX: &str = "/.well-known/acme-challenge/";

use crate::utils::strip_port_from_host;

pub fn build_redirect_url(host: &str, https_port: u16, path: &str, query: &str) -> String {
    let hostname = strip_port_from_host(host);

    let port_suffix = if https_port == 443 {
        String::new()
    } else {
        format!(":{https_port}")
    };

    let path_part = if path.is_empty() || !path.starts_with('/') {
        format!("/{path}")
    } else {
        path.to_string()
    };

    if query.is_empty() {
        format!("https://{hostname}{port_suffix}{path_part}")
    } else {
        format!("https://{hostname}{port_suffix}{path_part}?{query}")
    }
}

async fn redirect_handler(https_port: u16, request: Request) -> axum::response::Response {
    let host = request
        .headers()
        .get(HOST)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    let Some(host) = host else {
        return (StatusCode::BAD_REQUEST, "Bad Request").into_response();
    };

    let path = request.uri().path().to_string();
    let query = request.uri().query().unwrap_or("").to_string();

    if path.starts_with(ACME_CHALLENGE_PREFIX) {
        return (
            StatusCode::NOT_FOUND,
            [(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("text/plain; charset=utf-8"),
            )],
            "Not Found",
        )
            .into_response();
    }

    let location = build_redirect_url(&host, https_port, &path, &query);

    match HeaderValue::from_str(&location) {
        Ok(location_value) => (
            StatusCode::MOVED_PERMANENTLY,
            [(LOCATION, location_value)],
            StatusCode::MOVED_PERMANENTLY.to_string(),
        )
            .into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response(),
    }
}

pub fn redirect_router(https_port: u16) -> Router {
    Router::new().fallback(any(move |req| redirect_handler(https_port, req)))
}

pub async fn start_http_redirect_listener(
    listener_config: &ListenerConfig,
) -> anyhow::Result<(SocketAddr, tokio::task::JoinHandle<anyhow::Result<()>>)> {
    let bind_addr: SocketAddr = format!(
        "{}:{}",
        listener_config.bind_addr, listener_config.http_port
    )
    .parse()
    .map_err(|e| {
        anyhow::anyhow!(
            "invalid bind address {}:{} for HTTP redirect: {}",
            listener_config.bind_addr,
            listener_config.http_port,
            e
        )
    })?;

    let tcp_listener = TcpListener::bind(bind_addr).await?;
    let local_addr = tcp_listener.local_addr()?;

    info!(
        addr = %local_addr,
        https_port = listener_config.https_port,
        "HTTP redirect listener bound"
    );

    let https_port = listener_config.https_port;
    let app = redirect_router(https_port);

    let handle = tokio::spawn(async move {
        axum::serve(tcp_listener, app)
            .await
            .map_err(anyhow::Error::from)
    });

    Ok((local_addr, handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redirect_url_standard_443() {
        let url = build_redirect_url("example.com", 443, "/", "");
        assert_eq!(url, "https://example.com/");
    }

    #[test]
    fn test_redirect_url_non_standard_port() {
        let url = build_redirect_url("example.com", 8443, "/", "");
        assert_eq!(url, "https://example.com:8443/");
    }

    #[test]
    fn test_redirect_url_with_path() {
        let url = build_redirect_url("example.com", 443, "/some/path", "");
        assert_eq!(url, "https://example.com/some/path");
    }

    #[test]
    fn test_redirect_url_with_query() {
        let url = build_redirect_url("example.com", 443, "/path", "key=val");
        assert_eq!(url, "https://example.com/path?key=val");
    }

    #[test]
    fn test_redirect_url_with_path_and_query() {
        let url = build_redirect_url("example.com", 8443, "/path", "a=b&c=d");
        assert_eq!(url, "https://example.com:8443/path?a=b&c=d");
    }

    #[test]
    fn test_redirect_url_strips_host_port() {
        let url = build_redirect_url("example.com:8080", 443, "/", "");
        assert_eq!(url, "https://example.com/");
    }

    #[test]
    fn test_redirect_url_strips_host_port_non_standard_https() {
        let url = build_redirect_url("example.com:8080", 8443, "/api", "token=abc");
        assert_eq!(url, "https://example.com:8443/api?token=abc");
    }

    #[test]
    fn test_redirect_url_empty_path() {
        let url = build_redirect_url("example.com", 443, "", "");
        assert_eq!(url, "https://example.com/");
    }

    #[test]
    fn test_redirect_url_path_without_leading_slash() {
        let url = build_redirect_url("example.com", 443, "path", "");
        assert_eq!(url, "https://example.com/path");
    }

    #[test]
    fn test_redirect_url_root_path_with_query() {
        let url = build_redirect_url("git.alk.dev", 443, "/", "repo=test");
        assert_eq!(url, "https://git.alk.dev/?repo=test");
    }

    #[test]
    fn test_redirect_url_ipv6_host() {
        let url = build_redirect_url("[::1]", 443, "/", "");
        assert_eq!(url, "https://[::1]/");
    }

    #[test]
    fn test_redirect_url_ipv6_host_with_port() {
        let url = build_redirect_url("[::1]:8080", 443, "/", "");
        assert_eq!(url, "https://[::1]/");
    }

    #[test]
    fn test_redirect_url_ipv6_host_non_standard_https_port() {
        let url = build_redirect_url("[::1]:8080", 8443, "/", "");
        assert_eq!(url, "https://[::1]:8443/");
    }

    #[test]
    fn test_redirect_url_ipv4_host() {
        let url = build_redirect_url("203.0.113.10", 443, "/", "");
        assert_eq!(url, "https://203.0.113.10/");
    }
}
