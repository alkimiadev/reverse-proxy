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

fn make_redirect_listener_config(
    bind_addr: &str,
    http_port: u16,
    https_port: u16,
) -> reverse_proxy::config::static_config::ListenerConfig {
    reverse_proxy::config::static_config::ListenerConfig {
        bind_addr: bind_addr.to_string(),
        http_port,
        https_port,
        tls: reverse_proxy::config::static_config::TlsConfig {
            mode: "manual".to_string(),
            acme_domains: vec![],
            acme_cache_dir: String::new(),
            acme_directory: "production".to_string(),
            cert_path: String::new(),
            key_path: String::new(),
        },
        sites: vec![],
    }
}

#[tokio::test]
async fn test_http_redirect_returns_301_with_location() {
    let config = make_redirect_listener_config("127.0.0.1", 0, 443);
    let (addr, handle) = reverse_proxy::tls::redirect::start_http_redirect_listener(&config)
        .await
        .unwrap();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://127.0.0.1:{}/some/path", addr.port()))
        .header("Host", "example.com")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::MOVED_PERMANENTLY);
    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "https://example.com/some/path");

    handle.abort();
}

#[tokio::test]
async fn test_http_redirect_port_443_omitted_from_url() {
    let config = make_redirect_listener_config("127.0.0.1", 0, 443);
    let (addr, handle) = reverse_proxy::tls::redirect::start_http_redirect_listener(&config)
        .await
        .unwrap();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .header("Host", "example.com")
        .send()
        .await
        .unwrap();

    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "https://example.com/");

    handle.abort();
}

#[tokio::test]
async fn test_http_redirect_non_443_port_included_in_url() {
    let config = make_redirect_listener_config("127.0.0.1", 0, 8443);
    let (addr, handle) = reverse_proxy::tls::redirect::start_http_redirect_listener(&config)
        .await
        .unwrap();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .header("Host", "example.com")
        .send()
        .await
        .unwrap();

    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "https://example.com:8443/");

    handle.abort();
}

#[tokio::test]
async fn test_http_redirect_empty_host_returns_400() {
    let config = make_redirect_listener_config("127.0.0.1", 0, 443);
    let (addr, handle) = reverse_proxy::tls::redirect::start_http_redirect_listener(&config)
        .await
        .unwrap();

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: \r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    let mut response = vec![0u8; 4096];
    let n = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    let response_str = String::from_utf8_lossy(&response[..n]);
    assert!(
        response_str.contains(" 400 "),
        "expected 400 status, got: {response_str}"
    );

    handle.abort();
}

#[tokio::test]
async fn test_http_redirect_no_host_header_returns_400() {
    let config = make_redirect_listener_config("127.0.0.1", 0, 443);
    let (addr, handle) = reverse_proxy::tls::redirect::start_http_redirect_listener(&config)
        .await
        .unwrap();

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
    stream
        .write_all(b"GET / HTTP/1.0\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();

    let mut response = vec![0u8; 4096];
    let n = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    let response_str = String::from_utf8_lossy(&response[..n]);
    assert!(
        response_str.contains(" 400 "),
        "expected 400 status, got: {response_str}"
    );

    handle.abort();
}

#[tokio::test]
async fn test_http_redirect_strips_host_port() {
    let config = make_redirect_listener_config("127.0.0.1", 0, 443);
    let (addr, handle) = reverse_proxy::tls::redirect::start_http_redirect_listener(&config)
        .await
        .unwrap();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!("http://127.0.0.1:{}/path", addr.port()))
        .header("Host", "example.com:8080")
        .send()
        .await
        .unwrap();

    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "https://example.com/path");

    handle.abort();
}

#[tokio::test]
async fn test_http_redirect_preserves_query_string() {
    let config = make_redirect_listener_config("127.0.0.1", 0, 443);
    let (addr, handle) = reverse_proxy::tls::redirect::start_http_redirect_listener(&config)
        .await
        .unwrap();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();

    let resp = client
        .get(format!(
            "http://127.0.0.1:{}/search?q=test&page=1",
            addr.port()
        ))
        .header("Host", "git.alk.dev")
        .send()
        .await
        .unwrap();

    let location = resp.headers().get("location").unwrap().to_str().unwrap();
    assert_eq!(location, "https://git.alk.dev/search?q=test&page=1");

    handle.abort();
}

#[tokio::test]
async fn test_http_redirect_acme_challenge_returns_404() {
    let config = make_redirect_listener_config("127.0.0.1", 0, 443);
    let (addr, handle) = reverse_proxy::tls::redirect::start_http_redirect_listener(&config)
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let resp = client
        .get(format!(
            "http://127.0.0.1:{}/.well-known/acme-challenge/abc123",
            addr.port()
        ))
        .header("Host", "example.com")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);

    handle.abort();
}
