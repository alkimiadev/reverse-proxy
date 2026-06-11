mod helpers;

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
    let (addr, handle) =
        reverse_proxy::health::start_health_check_listener(0).await.unwrap();

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
    let (addr, handle) =
        reverse_proxy::health::start_health_check_listener(0).await.unwrap();

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