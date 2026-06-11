mod helpers;

use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::routing::post;
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

fn test_dynamic_config_with_limit(limit_bytes: u64) -> Arc<ArcSwap<DynamicConfig>> {
    let config = DynamicConfig {
        sites: vec![SiteConfig {
            host: "test.local".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }],
        rate_limit: RateLimitConfig {
            requests_per_second: 10,
            burst: 20,
        },
        body: BodyConfig { limit_bytes },
    };
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
async fn test_body_limit_config_reload_changes_limit() {
    let config = test_dynamic_config_with_limit(100);
    let config_clone = config.clone();

    let server = helpers::http_test_helper::TestUpstream::spawn(|| {
        let app = Router::new().route(
            "/",
            post(|body: axum::body::Body| async move {
                let _ = body;
                "ok"
            }),
        );
        router_with_body_limit(app, config_clone.clone())
    })
    .await;

    let client = reqwest::Client::new();

    let small_body = vec![0u8; 50];
    let resp = client
        .post(format!("http://127.0.0.1:{}/", server.addr.port()))
        .body(small_body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let medium_body = vec![0u8; 150];
    let resp = client
        .post(format!("http://127.0.0.1:{}/", server.addr.port()))
        .body(medium_body.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

    let new_config = DynamicConfig {
        sites: vec![SiteConfig {
            host: "test.local".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
            upstream_scheme: "http".to_string(),
            upstream_connect_timeout_secs: 5,
            upstream_request_timeout_secs: 60,
        }],
        rate_limit: RateLimitConfig {
            requests_per_second: 10,
            burst: 20,
        },
        body: BodyConfig { limit_bytes: 200 },
    };
    config.store(Arc::new(new_config));

    let resp = client
        .post(format!("http://127.0.0.1:{}/", server.addr.port()))
        .body(medium_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let large_body = vec![0u8; 300];
    let resp = client
        .post(format!("http://127.0.0.1:{}/", server.addr.port()))
        .body(large_body)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::PAYLOAD_TOO_LARGE);

    let _ = server.shutdown_tx.send(());
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
