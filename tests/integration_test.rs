mod helpers;

use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use axum::routing::{get, post};
use axum::Router;

use reverse_proxy::config::dynamic_config::{
    BodyConfig, DynamicConfig, RateLimitConfig, SiteConfig,
};
use reverse_proxy::proxy::body_limit::DEFAULT_BODY_LIMIT_BYTES;
use reverse_proxy::proxy::router_with_body_limit;

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
    let (addr, handle) = reverse_proxy::health::start_health_check_listener(0, None, None)
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
    let (addr, handle) = reverse_proxy::health::start_health_check_listener(0, None, None)
        .await
        .unwrap();

    assert!(addr.ip().is_loopback());
    assert_eq!(addr.ip().to_string(), "127.0.0.1");

    handle.abort();
}

#[tokio::test]
async fn test_health_check_binds_random_port_when_zero() {
    let result = reverse_proxy::health::start_health_check_listener(0, None, None).await;
    assert!(result.is_ok());
    let (addr, handle) = result.unwrap();
    assert_ne!(addr.port(), 0);
    handle.abort();
}

fn make_rate_limit_app(
    limiter: Arc<reverse_proxy::rate_limit::RateLimiter>,
) -> axum::extract::connect_info::IntoMakeServiceWithConnectInfo<Router, std::net::SocketAddr> {
    Router::new()
        .route("/", get(|| async { "ok" }))
        .layer(axum::middleware::from_fn_with_state(
            limiter,
            reverse_proxy::rate_limit::rate_limit_middleware,
        ))
        .into_make_service_with_connect_info::<std::net::SocketAddr>()
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
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
    }

    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
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
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
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
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp2 = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn test_rate_limit_without_connect_info_rejected_with_429() {
    let mut config = reverse_proxy::config::test_fixtures::test_dynamic_config();
    config.rate_limit = reverse_proxy::config::RateLimitConfig {
        requests_per_second: 10,
        burst: 20,
    };
    let config_arc = Arc::new(ArcSwap::from_pointee(config));
    let limiter = Arc::new(reverse_proxy::rate_limit::RateLimiter::new(config_arc));

    let app = Router::new().route("/", get(|| async { "ok" })).layer(
        axum::middleware::from_fn_with_state(
            limiter,
            reverse_proxy::rate_limit::rate_limit_middleware,
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async { axum::serve(listener, app).await.unwrap() });

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "Too Many Requests");
}

#[tokio::test]
async fn test_rate_limit_xff_header_ignored_same_bucket() {
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

    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .header("X-Forwarded-For", "10.0.0.1")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .header("X-Forwarded-For", "10.0.0.2")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let resp = client
        .get(format!("http://127.0.0.1:{}/", addr.port()))
        .header("X-Forwarded-For", "10.0.0.3")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
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

    let shutdown = Arc::new(reverse_proxy::shutdown::GracefulShutdown::new(30));
    let handle = reverse_proxy::rate_limit::start_eviction_task(
        limiter.clone(),
        Duration::from_millis(50),
        Duration::from_millis(100),
        shutdown.subscribe(),
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
            acme_contact: String::new(),
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

fn write_valid_config(dir: &Path) -> std::path::PathBuf {
    let config_path = dir.join("config.toml");
    let config = r#"
health_check_port = 9900
admin_key_path = "/etc/reverse-proxy/admin-key"

[logging]
level = "info"
format = "text"

[[listeners]]
bind_addr = "127.0.0.1"
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme-cache"
acme_contact = "mailto:admin@test.local"

[[listeners.sites]]
host = "test.local"
upstream = "127.0.0.1:8080"

[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600
"#;
    std::fs::write(&config_path, config).unwrap();
    config_path
}

fn write_invalid_config(dir: &Path) -> std::path::PathBuf {
    let config_path = dir.join("config.toml");
    let config = r#"
health_check_port = 9900
"#;
    std::fs::write(&config_path, config).unwrap();
    config_path
}

fn binary_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_reverse-proxy"))
}

#[test]
fn test_validate_valid_config_exits_0() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_valid_config(dir.path());
    let output = Command::new(binary_path())
        .arg("--config")
        .arg(config_path.to_str().unwrap())
        .arg("--validate")
        .output()
        .expect("failed to run binary");
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0 with valid config, got {}: stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_validate_invalid_config_exits_1() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_invalid_config(dir.path());
    let output = Command::new(binary_path())
        .arg("--config")
        .arg(config_path.to_str().unwrap())
        .arg("--validate")
        .output()
        .expect("failed to run binary");
    assert!(
        output.status.code() == Some(1) || output.status.code() == Some(2),
        "expected non-zero exit with invalid config, got {}: stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_validate_missing_config_file_exits_1() {
    let output = Command::new(binary_path())
        .arg("--config")
        .arg("/nonexistent/path/config.toml")
        .arg("--validate")
        .output()
        .expect("failed to run binary");
    assert_ne!(
        output.status.code(),
        Some(0),
        "expected non-zero exit for missing config"
    );
}

#[test]
fn test_validate_wildcard_bind_via_cli_flag() {
    let dir = tempfile::tempdir().unwrap();
    let config_path = write_valid_config(dir.path());
    let output = Command::new(binary_path())
        .arg("--config")
        .arg(config_path.to_str().unwrap())
        .arg("--validate")
        .arg("--allow-wildcard-bind")
        .output()
        .expect("failed to run binary");
    assert_eq!(
        output.status.code(),
        Some(0),
        "expected exit 0 with --allow-wildcard-bind, got {}: stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

fn test_dynamic_config_with_limit(limit_bytes: u64) -> Arc<ArcSwap<DynamicConfig>> {
    let sites = vec![SiteConfig {
        host: "test.local".to_string(),
        upstream: "127.0.0.1:8080".to_string(),
        upstream_scheme: "http".to_string(),
        upstream_connect_timeout_secs: 5,
        upstream_request_timeout_secs: 60,
    }];
    let config = DynamicConfig::from_sites(
        sites,
        RateLimitConfig {
            requests_per_second: 10,
            burst: 20,
        },
        BodyConfig { limit_bytes },
    );
    Arc::new(ArcSwap::from_pointee(config))
}

async fn spawn_server_with_limit(limit_bytes: u64) -> helpers::http_test_helper::TestUpstream {
    let config = test_dynamic_config_with_limit(limit_bytes);
    helpers::http_test_helper::TestUpstream::spawn(|| {
        let app = Router::new().route(
            "/",
            post(|body: axum::body::Body| async move {
                let _ = body;
                "ok"
            }),
        );
        router_with_body_limit(app, config.clone())
    })
    .await
}

#[tokio::test]
async fn test_body_limit_rejects_oversized_request() {
    let server = spawn_server_with_limit(100).await;
    let client = reqwest::Client::new();

    let large_body = vec![0u8; 200];
    let resp = client
        .post(format!("http://127.0.0.1:{}/", server.addr.port()))
        .body(large_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "Payload Too Large");

    let _ = server.shutdown_tx.send(());
}

#[tokio::test]
async fn test_body_limit_allows_request_within_limit() {
    let server = spawn_server_with_limit(100).await;
    let client = reqwest::Client::new();

    let small_body = vec![0u8; 50];
    let resp = client
        .post(format!("http://127.0.0.1:{}/", server.addr.port()))
        .body(small_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let _ = server.shutdown_tx.send(());
}

#[tokio::test]
async fn test_body_limit_allows_request_at_exact_limit() {
    let server = spawn_server_with_limit(100).await;
    let client = reqwest::Client::new();

    let exact_body = vec![0u8; 100];
    let resp = client
        .post(format!("http://127.0.0.1:{}/", server.addr.port()))
        .body(exact_body)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let _ = server.shutdown_tx.send(());
}

#[tokio::test]
async fn test_body_limit_content_length_header_rejection() {
    let server = spawn_server_with_limit(100).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/", server.addr.port()))
        .header("content-length", "200")
        .body(vec![0u8; 200])
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);
    let body = resp.text().await.unwrap();
    assert_eq!(body, "Payload Too Large");

    let _ = server.shutdown_tx.send(());
}

#[tokio::test]
async fn test_body_limit_default_is_100mb() {
    assert_eq!(DEFAULT_BODY_LIMIT_BYTES, 104_857_600);
}

#[tokio::test]
async fn test_body_limit_empty_body_request_succeeds() {
    let server = spawn_server_with_limit(100).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("http://127.0.0.1:{}/", server.addr.port()))
        .body("")
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let _ = server.shutdown_tx.send(());
}

#[tokio::test]
async fn test_graceful_shutdown_trigger() {
    let shutdown = Arc::new(reverse_proxy::shutdown::GracefulShutdown::new(30));

    assert!(!shutdown.is_shutdown_requested());

    let mut rx = shutdown.subscribe();
    assert!(!*rx.borrow_and_update());

    shutdown.trigger_shutdown();

    assert!(shutdown.is_shutdown_requested());
    assert!(rx.has_changed().unwrap());
    assert!(*rx.borrow_and_update());
}

#[tokio::test]
async fn test_graceful_shutdown_custom_timeout() {
    let shutdown = reverse_proxy::shutdown::GracefulShutdown::new(60);
    assert_eq!(shutdown.shutdown_timeout(), Duration::from_secs(60));
}

#[tokio::test]
async fn test_graceful_shutdown_subscribe_multiple_receivers() {
    let shutdown = Arc::new(reverse_proxy::shutdown::GracefulShutdown::new(10));

    let mut rx1 = shutdown.subscribe();
    let mut rx2 = shutdown.subscribe();

    assert!(!*rx1.borrow_and_update());
    assert!(!*rx2.borrow_and_update());

    shutdown.trigger_shutdown();

    assert!(rx1.has_changed().unwrap());
    assert!(rx2.has_changed().unwrap());
}

#[tokio::test]
async fn test_sighup_config_reload_valid_config() {
    let config_arc = Arc::new(ArcSwap::from_pointee(
        reverse_proxy::config::test_fixtures::test_dynamic_config(),
    ));
    let static_config = reverse_proxy::config::test_fixtures::test_static_config();
    let reload_handle = Arc::new(reverse_proxy::config::ConfigReloadHandle::new(
        config_arc.clone(),
        static_config,
        false,
    ));

    let dir = tempfile::tempdir().unwrap();
    let config_content = r#"
health_check_port = 9900
admin_key_path = "/tmp/test-admin-key"

[logging]
level = "info"
format = "text"

[rate_limit]
requests_per_second = 20
burst = 40

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "127.0.0.1"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme-cache"
acme_directory = "staging"
acme_contact = "mailto:admin@test.local"

[[listeners.sites]]
host = "test.local"
upstream = "127.0.0.1:8080"
"#;
    let config_path = dir.path().join("config.toml");
    tokio::fs::write(&config_path, config_content)
        .await
        .unwrap();

    let config_path_str = config_path.to_str().unwrap().to_string();
    reverse_proxy::shutdown::handle_sighup_reload(&reload_handle, &config_path_str).await;

    let loaded = reload_handle.load();
    assert_eq!(loaded.rate_limit.requests_per_second, 20);
    assert_eq!(loaded.rate_limit.burst, 40);
}

#[tokio::test]
async fn test_sighup_config_reload_invalid_config_keeps_old() {
    let config_arc = Arc::new(ArcSwap::from_pointee(
        reverse_proxy::config::test_fixtures::test_dynamic_config(),
    ));
    let static_config = reverse_proxy::config::test_fixtures::test_static_config();
    let reload_handle = Arc::new(reverse_proxy::config::ConfigReloadHandle::new(
        config_arc.clone(),
        static_config,
        false,
    ));

    let dir = tempfile::tempdir().unwrap();
    let config_content = "invalid toml {{{";
    let config_path = dir.path().join("config.toml");
    tokio::fs::write(&config_path, config_content)
        .await
        .unwrap();

    let config_path_str = config_path.to_str().unwrap().to_string();
    let _ = reverse_proxy::shutdown::handle_sighup_reload(&reload_handle, &config_path_str).await;

    let loaded = reload_handle.load();
    assert_eq!(loaded.rate_limit.requests_per_second, 10);
}

#[tokio::test]
async fn test_graceful_shutdown_with_health_check() {
    let (addr, handle) = reverse_proxy::health::start_health_check_listener(0, None, None)
        .await
        .unwrap();

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://127.0.0.1:{}/health", addr.port()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let shutdown = Arc::new(reverse_proxy::shutdown::GracefulShutdown::new(5));
    let rx = shutdown.subscribe();

    assert!(!shutdown.is_shutdown_requested());

    shutdown.trigger_shutdown();
    assert!(shutdown.is_shutdown_requested());
    assert!(rx.has_changed().unwrap());

    handle.abort();
}
