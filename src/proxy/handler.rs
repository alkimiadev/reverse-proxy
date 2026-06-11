use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

use arc_swap::ArcSwap;

use crate::config::dynamic_config::DynamicConfig;

async fn health_handler() -> impl IntoResponse {
    StatusCode::OK
}

async fn proxy_handler(
    State(state): State<Arc<ArcSwap<DynamicConfig>>>,
    req: axum::http::Request<axum::body::Body>,
) -> impl IntoResponse {
    if req.uri().path() == "/health" {
        return StatusCode::OK.into_response();
    }

    let host = req
        .headers()
        .get(axum::http::header::HOST)
        .and_then(|v| v.to_str().ok());

    let host = match host {
        Some(h) => h,
        None => return StatusCode::BAD_REQUEST.into_response(),
    };

    let config = state.load();
    match config.lookup(host) {
        Some(_site) => StatusCode::OK.into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

pub fn proxy_router(state: Arc<ArcSwap<DynamicConfig>>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .fallback(proxy_handler)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::dynamic_config::{BodyConfig, RateLimitConfig};
    use crate::config::SiteConfig;
    use axum::body::Body;
    use axum::http::{Request, Response};
    use tower::ServiceExt;

    fn make_config_with_sites(sites: Vec<SiteConfig>) -> Arc<ArcSwap<DynamicConfig>> {
        Arc::new(ArcSwap::from_pointee(DynamicConfig::from_sites(
            sites,
            RateLimitConfig {
                requests_per_second: 10,
                burst: 20,
            },
            BodyConfig {
                limit_bytes: 104857600,
            },
        )))
    }

    async fn send_request(
        router: &mut Router,
        method: &str,
        uri: &str,
        host: Option<&str>,
    ) -> Response<axum::body::Body> {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(h) = host {
            builder = builder.header("Host", h);
        }
        let req = builder.body(Body::empty()).unwrap();
        router.oneshot(req).await.unwrap()
    }

    #[tokio::test]
    async fn health_path_returns_200_regardless_of_host() {
        let state = make_config_with_sites(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let mut router = proxy_router(state);

        let resp = send_request(&mut router, "GET", "/health", None).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn health_with_unknown_host_returns_200() {
        let state = make_config_with_sites(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let mut router = proxy_router(state);

        let resp = send_request(&mut router, "GET", "/health", Some("unknown.host")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_host_returns_400() {
        let state = make_config_with_sites(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let mut router = proxy_router(state);

        let resp = send_request(&mut router, "GET", "/some/path", None).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn unknown_host_returns_404() {
        let state = make_config_with_sites(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let mut router = proxy_router(state);

        let resp = send_request(&mut router, "GET", "/some/path", Some("unknown.host")).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn known_host_returns_200() {
        let state = make_config_with_sites(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let mut router = proxy_router(state);

        let resp = send_request(&mut router, "GET", "/some/path", Some("example.com")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn host_matching_is_case_insensitive() {
        let state = make_config_with_sites(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let mut router = proxy_router(state);

        let resp = send_request(&mut router, "GET", "/path", Some("EXAMPLE.COM")).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = send_request(&mut router, "GET", "/path", Some("Example.Com")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn host_with_port_stripped() {
        let state = make_config_with_sites(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let mut router = proxy_router(state);

        let resp = send_request(&mut router, "GET", "/path", Some("example.com:443")).await;
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = send_request(&mut router, "GET", "/path", Some("EXAMPLE.COM:8443")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn routing_table_update_visible_immediately() {
        let state = make_config_with_sites(vec![SiteConfig {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }]);
        let mut router = proxy_router(state.clone());

        let resp = send_request(&mut router, "GET", "/path", Some("new.example.com")).await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        let new_config = DynamicConfig::from_sites(
            vec![
                SiteConfig {
                    host: "example.com".to_string(),
                    upstream: "127.0.0.1:8080".to_string(),
                    upstream_scheme: "http".to_string(),
                    upstream_connect_timeout_secs: 5,
                    upstream_request_timeout_secs: 60,
                },
                SiteConfig {
                    host: "new.example.com".to_string(),
                    upstream: "127.0.0.1:9090".to_string(),
                    upstream_scheme: "http".to_string(),
                    upstream_connect_timeout_secs: 5,
                    upstream_request_timeout_secs: 60,
                },
            ],
            RateLimitConfig {
                requests_per_second: 10,
                burst: 20,
            },
            BodyConfig {
                limit_bytes: 104857600,
            },
        );
        state.store(Arc::new(new_config));

        let resp = send_request(&mut router, "GET", "/path", Some("new.example.com")).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
