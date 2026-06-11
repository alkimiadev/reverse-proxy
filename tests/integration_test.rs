mod helpers;

use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use axum::routing::get;
use axum::Router;

#[tokio::test]
async fn test_upstream_spawn_and_connect() {
    let upstream = helpers::http_test_helper::TestUpstream::spawn_ok().await;
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/", upstream.addr.port()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let _ = upstream.shutdown_tx.send(());
}

#[test]
fn test_self_signed_cert_generation() {
    let cert = helpers::tls_test_helper::generate_self_signed_cert(&["test.local"]);
    assert!(!cert.cert_pem.is_empty());
    assert!(!cert.key_pem.is_empty());
    assert!(cert.cert_pem.contains("BEGIN CERTIFICATE"));
    assert!(cert.key_pem.contains("BEGIN"));
}

#[test]
fn test_config_fixtures() {
    let static_config = reverse_proxy::config::test_fixtures::test_static_config();
    assert!(!static_config.listeners.is_empty());

    let dynamic_config = reverse_proxy::config::test_fixtures::test_dynamic_config();
    assert!(!dynamic_config.sites.is_empty());
}

#[tokio::test]
async fn test_health_check_local_port_returns_200() {
    let (addr, handle) = reverse_proxy::health::start_health_check_listener(0)
        .await
        .unwrap();

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
async fn test_health_check_local_port_binds_localhost() {
    let (addr, handle) = reverse_proxy::health::start_health_check_listener(0)
        .await
        .unwrap();

    assert!(addr.ip().is_loopback());
    assert_eq!(addr.ip().to_string(), "127.0.0.1");

    handle.abort();
}

#[tokio::test]
async fn test_health_check_disabled_when_port_zero() {
    let result = reverse_proxy::health::start_health_check_listener(0).await;
    assert!(result.is_ok());
    let (addr, handle) = result.unwrap();
    assert_ne!(addr.port(), 0);
    handle.abort();
}

fn make_rate_limit_app(limiter: Arc<reverse_proxy::rate_limit::RateLimiter>) -> Router {
    Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn_with_state(
            limiter,
            reverse_proxy::rate_limit::rate_limit_middleware,
        ))
}

#[tokio::test]
async fn test_rate_limit_allows_within_burst() {
    let mut config = reverse_proxy::config::test_fixtures::test_dynamic_config();
    config.rate_limit = reverse_proxy::config::RateLimitConfig {
        requests_per_second: 10,
        burst: 5,
    };
    let config_arc = Arc::new(ArcSwap::from_pointee(config));
    let limiter = Arc::new(reverse_proxy::rate_limit::RateLimiter::new(config_arc));

    let app = make_rate_limit_app(limiter);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    for _ in 0..5 {
        let resp = client
            .get(format!("http://127.0.0.1:{}/", addr.port()))
            .header("x-forwarded-for", "192.168.1.1")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
    }
}

#[tokio::test]
async fn test_rate_limit_rejects_above_burst() {
    let mut config = reverse_proxy::config::test_fixtures::test_dynamic_config();
    config.rate_limit = reverse_proxy::config::RateLimitConfig {
        requests_per_second: 10,
        burst: 2,
    };
    let config_arc = Arc::new(ArcSwap::from_pointee(config));
    let limiter = Arc::new(reverse_proxy::rate_limit::RateLimiter::new(config_arc));

    let app = make_rate_limit_app(limiter);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    for _ in 0..2 {
        let resp = client
            .get(format!("http://127.0.0.1:{}/", addr.port()))
            .header("x-forwarded-for", "10.0.0.50")
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
    }

    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .header("x-forwarded-for", "10.0.0.50")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "Too Many Requests");
}

#[tokio::test]
async fn test_rate_limit_429_response_body() {
    let mut config = reverse_proxy::config::test_fixtures::test_dynamic_config();
    config.rate_limit = reverse_proxy::config::RateLimitConfig {
        requests_per_second: 10,
        burst: 1,
    };
    let config_arc = Arc::new(ArcSwap::from_pointee(config));
    let limiter = Arc::new(reverse_proxy::rate_limit::RateLimiter::new(config_arc));

    let app = make_rate_limit_app(limiter);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .header("x-forwarded-for", "203.0.113.50")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .header("x-forwarded-for", "203.0.113.50")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "Too Many Requests");
}

#[tokio::test]
async fn test_rate_limit_per_ip_independent() {
    let mut config = reverse_proxy::config::test_fixtures::test_dynamic_config();
    config.rate_limit = reverse_proxy::config::RateLimitConfig {
        requests_per_second: 10,
        burst: 1,
    };
    let config_arc = Arc::new(ArcSwap::from_pointee(config));
    let limiter = Arc::new(reverse_proxy::rate_limit::RateLimiter::new(config_arc));

    let app = make_rate_limit_app(limiter);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .header("x-forwarded-for", "192.168.1.1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp2 = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .header("x-forwarded-for", "192.168.1.2")
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), reqwest::StatusCode::OK);
}

#[tokio::test]
async fn test_rate_limit_eviction_task() {
    let mut config = reverse_proxy::config::test_fixtures::test_dynamic_config();
    config.rate_limit = reverse_proxy::config::RateLimitConfig {
        requests_per_second: 10,
        burst: 20,
    };
    let config_arc = Arc::new(ArcSwap::from_pointee(config));
    let limiter = Arc::new(reverse_proxy::rate_limit::RateLimiter::new(config_arc));

    limiter.check_and_consume(std::net::IpAddr::from([192, 168, 1, 1]));

    let handle = reverse_proxy::rate_limit::start_eviction_task(
        limiter.clone(),
        Duration::from_millis(50),
        Duration::from_millis(100),
    );

    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(!limiter.contains_ip(std::net::IpAddr::from([192, 168, 1, 1])));

    handle.abort();
}

#[tokio::test]
async fn test_proxy_forwards_request_to_upstream() {
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use reverse_proxy::config::dynamic_config::{BodyConfig, DynamicConfig, RateLimitConfig};
    use reverse_proxy::config::SiteConfig;
    use reverse_proxy::proxy::{create_http_client, create_https_client, proxy_router, ProxyState};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tower::ServiceExt;

    let upstream = helpers::http_test_helper::TestUpstream::spawn(|| {
        axum::Router::new().route(
            "/test",
            axum::routing::get(|req: axum::extract::Request| async move {
                let x_real_ip = req
                    .headers()
                    .get("x-real-ip")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("missing");
                let x_fwd_for = req
                    .headers()
                    .get("x-forwarded-for")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("missing");
                let x_fwd_proto = req
                    .headers()
                    .get("x-forwarded-proto")
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("missing");
                axum::response::IntoResponse::into_response(format!(
                    "ip={}|for={}|proto={}",
                    x_real_ip, x_fwd_for, x_fwd_proto
                ))
            }),
        )
    })
    .await;

    let upstream_addr = format!("127.0.0.1:{}", upstream.addr.port());

    let state = Arc::new(ProxyState {
        config: Arc::new(ArcSwap::from_pointee(DynamicConfig::from_sites(
            vec![SiteConfig {
                host: "test.local".to_string(),
                upstream: upstream_addr.clone(),
                upstream_scheme: "http".to_string(),
                upstream_connect_timeout_secs: 5,
                upstream_request_timeout_secs: 60,
            }],
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
    });

    let router = proxy_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/test")
        .header("Host", "test.local")
        .extension(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)),
            54321,
        )))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("ip=192.168.1.100"));
    assert!(body_str.contains("for=192.168.1.100"));
    assert!(body_str.contains("proto=https"));

    let _ = upstream.shutdown_tx.send(());
}

#[tokio::test]
async fn test_proxy_removes_hop_by_hop_from_response() {
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use reverse_proxy::config::dynamic_config::{BodyConfig, DynamicConfig, RateLimitConfig};
    use reverse_proxy::config::SiteConfig;
    use reverse_proxy::proxy::{create_http_client, create_https_client, proxy_router, ProxyState};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tower::ServiceExt;

    let upstream = helpers::http_test_helper::TestUpstream::spawn(|| {
        axum::Router::new().route(
            "/",
            axum::routing::get(|| async {
                ([(axum::http::header::CONNECTION, "keep-alive")], "hello")
            }),
        )
    })
    .await;

    let upstream_addr = format!("127.0.0.1:{}", upstream.addr.port());

    let state = Arc::new(ProxyState {
        config: Arc::new(ArcSwap::from_pointee(DynamicConfig::from_sites(
            vec![SiteConfig {
                host: "test.local".to_string(),
                upstream: upstream_addr.clone(),
                upstream_scheme: "http".to_string(),
                upstream_connect_timeout_secs: 5,
                upstream_request_timeout_secs: 60,
            }],
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
    });

    let router = proxy_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/")
        .header("Host", "test.local")
        .extension(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            12345,
        )))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(resp.headers().get("connection").is_none());

    let _ = upstream.shutdown_tx.send(());
}

#[tokio::test]
async fn test_proxy_returns_502_on_unreachable_upstream() {
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use reverse_proxy::config::dynamic_config::{BodyConfig, DynamicConfig, RateLimitConfig};
    use reverse_proxy::config::SiteConfig;
    use reverse_proxy::proxy::{create_http_client, create_https_client, proxy_router, ProxyState};
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tower::ServiceExt;

    let state = Arc::new(ProxyState {
        config: Arc::new(ArcSwap::from_pointee(DynamicConfig::from_sites(
            vec![SiteConfig {
                host: "unreachable.local".to_string(),
                upstream: "127.0.0.1:1".to_string(),
                upstream_scheme: "http".to_string(),
                upstream_connect_timeout_secs: 1,
                upstream_request_timeout_secs: 2,
            }],
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
    });

    let router = proxy_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/")
        .header("Host", "unreachable.local")
        .extension(ConnectInfo(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
            12345,
        )))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
}
