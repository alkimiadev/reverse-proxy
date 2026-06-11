mod helpers;

use std::process::Command;
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

fn write_valid_config(dir: &std::path::Path) -> std::path::PathBuf {
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, "cert").unwrap();
    std::fs::write(&key_path, "key").unwrap();

    let toml = format!(
        r#"
[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "127.0.0.1"
http_port = 80
https_port = 443

[listeners.tls]
mode = "manual"
cert_path = "{}"
key_path = "{}"

[[listeners.sites]]
host = "test.local"
upstream = "127.0.0.1:8080"
"#,
        cert_path.to_str().unwrap(),
        key_path.to_str().unwrap()
    );
    let config_path = dir.join("valid_config.toml");
    std::fs::write(&config_path, toml).unwrap();
    config_path
}

fn write_invalid_config(dir: &std::path::Path) -> std::path::PathBuf {
    let toml = r#"
[rate_limit]
requests_per_second = 0
burst = 20

[body]
limit_bytes = 0
"#;
    let config_path = dir.join("invalid_config.toml");
    std::fs::write(&config_path, toml).unwrap();
    config_path
}

fn binary_path() -> std::path::PathBuf {
    let bin = env!("CARGO_BIN_EXE_reverse-proxy");
    std::path::PathBuf::from(bin)
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
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0, got {}: stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("valid"));
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
        .unwrap();
    assert!(!output.status.success(), "expected exit 1, got success");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("validation failed") || stderr.contains("error"));
}

#[test]
fn test_validate_missing_config_file_exits_1() {
    let output = Command::new(binary_path())
        .arg("--config")
        .arg("/nonexistent/path/config.toml")
        .arg("--validate")
        .output()
        .unwrap();
    assert!(!output.status.success(), "expected exit 1, got success");
}

#[test]
fn test_validate_wildcard_bind_via_cli_flag() {
    let dir = tempfile::tempdir().unwrap();
    let toml = r#"
[rate_limit]
requests_per_second = 10
burst = 20

[body]
limit_bytes = 104857600

[[listeners]]
bind_addr = "0.0.0.0"
http_port = 80
https_port = 443

[listeners.tls]
mode = "acme"
acme_domains = ["test.local"]
acme_cache_dir = "/tmp/acme"
"#;
    let config_path = dir.path().join("wildcard.toml");
    std::fs::write(&config_path, toml).unwrap();

    let output = Command::new(binary_path())
        .arg("--config")
        .arg(config_path.to_str().unwrap())
        .arg("--validate")
        .arg("--allow-wildcard-bind")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "expected exit 0 with --allow-wildcard-bind, got {}: stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}
