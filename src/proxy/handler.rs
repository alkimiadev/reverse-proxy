use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{Request, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use tracing::warn;

use crate::config::dynamic_config::DynamicConfig;
use crate::proxy::error::ProxyError;
use crate::proxy::headers::{inject_proxy_headers, remove_hop_by_hop};

pub struct ProxyState {
    pub config: Arc<ArcSwap<DynamicConfig>>,
    pub http_client: Client<HttpConnector, Body>,
    pub https_client: Client<hyper_rustls::HttpsConnector<HttpConnector>, Body>,
}

async fn health_handler() -> impl IntoResponse {
    StatusCode::OK
}

async fn proxy_handler(
    ConnectInfo(remote_addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<ProxyState>>,
    mut req: Request<Body>,
) -> Response {
    if req.uri().path() == "/health" {
        return StatusCode::OK.into_response();
    }

    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());

    let host = match host {
        Some(h) => h,
        None => return ProxyError::MissingHost.into_response(),
    };

    let config = state.config.load();
    let site = match config.lookup(host) {
        Some(s) => s.clone(),
        None => return ProxyError::UnknownHost.into_response(),
    };

    let is_https = determine_if_https(host);
    inject_proxy_headers(req.headers_mut(), remote_addr, is_https);
    remove_hop_by_hop(req.headers_mut());

    let upstream_scheme = site.upstream_scheme.clone();
    let upstream = site.upstream.clone();
    let upstream_uri = build_upstream_uri(&upstream_scheme, &upstream, req.uri());

    let upstream_req = match build_upstream_request(req, &upstream_uri) {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "failed to build upstream request");
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };

    let request_timeout = Duration::from_secs(site.upstream_request_timeout_secs);

    let result = if upstream_scheme == "https" {
        tokio::time::timeout(request_timeout, state.https_client.request(upstream_req)).await
    } else {
        tokio::time::timeout(request_timeout, state.http_client.request(upstream_req)).await
    };

    match result {
        Ok(Ok(upstream_resp)) => {
            let (mut parts, body) = upstream_resp.into_parts();
            remove_hop_by_hop(&mut parts.headers);
            parts.headers.remove("server");
            let body = Body::new(body);
            Response::from_parts(parts, body)
        }
        Ok(Err(e)) => {
            if e.is_connect() {
                ProxyError::UpstreamConnection(e).into_response()
            } else {
                warn!(error = %e, "upstream request failed");
                StatusCode::BAD_GATEWAY.into_response()
            }
        }
        Err(_) => ProxyError::UpstreamTimeout.into_response(),
    }
}

fn determine_if_https(host: &str) -> bool {
    let port_str = host.split(':').nth(1);
    if let Some(port) = port_str {
        if let Ok(p) = port.parse::<u16>() {
            return p == 443;
        }
    }
    true
}

fn build_upstream_uri(scheme: &str, upstream: &str, original_uri: &Uri) -> Uri {
    let path = original_uri.path();
    let query = original_uri
        .query()
        .map(|q| format!("?{}", q))
        .unwrap_or_default();
    let uri_string = format!("{}://{}{}{}", scheme, upstream, path, query);
    uri_string.parse::<Uri>().unwrap_or_else(|_| {
        format!("{}://{}{}", scheme, upstream, path)
            .parse::<Uri>()
            .unwrap()
    })
}

fn build_upstream_request(req: Request<Body>, upstream_uri: &Uri) -> anyhow::Result<Request<Body>> {
    let mut builder = Request::builder()
        .method(req.method().clone())
        .uri(upstream_uri.clone());

    for (name, value) in req.headers().iter() {
        builder = builder.header(name.as_str(), value);
    }

    builder.body(req.into_body()).map_err(Into::into)
}

pub fn create_http_client() -> Client<HttpConnector, Body> {
    Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(90))
        .build_http()
}

pub fn create_https_client() -> Client<hyper_rustls::HttpsConnector<HttpConnector>, Body> {
    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_certs())
        .with_no_client_auth();

    let https_connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(tls_config)
        .https_or_http()
        .enable_http1()
        .build();

    Client::builder(TokioExecutor::new())
        .pool_idle_timeout(Duration::from_secs(90))
        .build(https_connector)
}

fn root_certs() -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();
    let result = rustls_native_certs::load_native_certs();
    for cert in result.certs {
        roots.add(cert).ok();
    }
    if !result.errors.is_empty() {
        for err in &result.errors {
            warn!(error = %err, "failed to load native certificate");
        }
    }
    roots
}

pub fn proxy_router(state: Arc<ProxyState>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .fallback(proxy_handler)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dynamic_config::{BodyConfig, DynamicConfig, RateLimitConfig};
    use crate::config::SiteConfig;
    use axum::body::Body;
    use axum::http::Request;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tower::ServiceExt;

    fn make_proxy_state(sites: Vec<SiteConfig>) -> Arc<ProxyState> {
        Arc::new(ProxyState {
            config: Arc::new(ArcSwap::from_pointee(DynamicConfig::from_sites(
                sites,
                RateLimitConfig {
                    requests_per_second: 10,
                    burst: 20,
                },
                BodyConfig {
                    limit_bytes: 104857600,
                },
            ))),
            http_client: create_http_client(),
            https_client: create_https_client(),
        })
    }

    fn make_request_with_connect_info(
        method: &str,
        uri: &str,
        host: Option<&str>,
        remote_addr: SocketAddr,
    ) -> Request<Body> {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(h) = host {
            builder = builder.header("Host", h);
        }
        let mut req = builder.body(Body::empty()).unwrap();
        req.extensions_mut().insert(ConnectInfo(remote_addr));
        req
    }

    #[tokio::test]
    async fn health_path_returns_200_regardless_of_host() {
        let state = make_proxy_state(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let router = proxy_router(state);
        let req = make_request_with_connect_info(
            "GET",
            "/health",
            None,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345),
        );
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_with_unknown_host_returns_200() {
        let state = make_proxy_state(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let router = proxy_router(state);
        let req = make_request_with_connect_info(
            "GET",
            "/health",
            Some("unknown.host"),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345),
        );
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_host_returns_400() {
        let state = make_proxy_state(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let router = proxy_router(state);
        let req = make_request_with_connect_info(
            "GET",
            "/some/path",
            None,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345),
        );
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unknown_host_returns_404() {
        let state = make_proxy_state(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let router = proxy_router(state);
        let req = make_request_with_connect_info(
            "GET",
            "/some/path",
            Some("unknown.host"),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345),
        );
        let resp = router.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_determine_if_https_port_443() {
        assert!(determine_if_https("example.com:443"));
    }

    #[test]
    fn test_determine_if_https_port_80() {
        assert!(!determine_if_https("example.com:80"));
    }

    #[test]
    fn test_determine_if_https_no_port() {
        assert!(determine_if_https("example.com"));
    }

    #[test]
    fn test_determine_if_https_port_8443() {
        assert!(!determine_if_https("example.com:8443"));
    }

    #[test]
    fn test_build_upstream_uri_with_query() {
        let uri: Uri = "/path?foo=bar".parse().unwrap();
        let result = build_upstream_uri("http", "127.0.0.1:8080", &uri);
        assert_eq!(result.to_string(), "http://127.0.0.1:8080/path?foo=bar");
    }

    #[test]
    fn test_build_upstream_uri_without_query() {
        let uri: Uri = "/path".parse().unwrap();
        let result = build_upstream_uri("http", "127.0.0.1:8080", &uri);
        assert_eq!(result.to_string(), "http://127.0.0.1:8080/path");
    }

    #[test]
    fn test_build_upstream_uri_https() {
        let uri: Uri = "/secure".parse().unwrap();
        let result = build_upstream_uri("https", "upstream.example.com", &uri);
        assert_eq!(result.to_string(), "https://upstream.example.com/secure");
    }
}
