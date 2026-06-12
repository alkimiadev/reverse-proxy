pub mod bucket;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::IntoResponse;
use dashmap::DashMap;
use tracing::warn;

use crate::config::DynamicConfig;
use bucket::{normalize_ip, TokenBucket};

pub struct RateLimiter {
    buckets: DashMap<IpAddr, TokenBucket>,
    config: Arc<ArcSwap<DynamicConfig>>,
}

impl RateLimiter {
    pub fn new(config: Arc<ArcSwap<DynamicConfig>>) -> Self {
        Self {
            buckets: DashMap::new(),
            config,
        }
    }

    pub fn check_and_consume(&self, ip: IpAddr) -> bool {
        let normalized = normalize_ip(ip);
        let config = self.config.load();
        let rate = config.rate_limit.requests_per_second as f64;
        let burst = config.rate_limit.burst;

        let entry = self.buckets.entry(normalized);
        match entry {
            dashmap::mapref::entry::Entry::Occupied(mut occupied) => {
                occupied.get_mut().try_consume(rate, burst)
            }
            dashmap::mapref::entry::Entry::Vacant(vacant) => {
                let mut bucket = TokenBucket::new(rate, burst);
                let result = bucket.try_consume(rate, burst);
                vacant.insert(bucket);
                result
            }
        }
    }

    pub fn evict_stale(&self, max_age: Duration) {
        let cutoff = std::time::Instant::now() - max_age;
        self.buckets.retain(|_, bucket| bucket.last_access > cutoff);
    }

    pub fn contains_ip(&self, ip: IpAddr) -> bool {
        self.buckets.contains_key(&normalize_ip(ip))
    }
}

pub async fn rate_limit_middleware(
    axum::extract::State(limiter): axum::extract::State<Arc<RateLimiter>>,
    req: Request,
    next: Next,
) -> axum::response::Response {
    let client_ip = req
        .extensions()
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .map(|ci| ci.ip());

    let Some(ip) = client_ip else {
        return (StatusCode::TOO_MANY_REQUESTS, "Too Many Requests").into_response();
    };

    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");

    let path = req.uri().path();

    if !limiter.check_and_consume(ip) {
        warn!(
            "RATE_LIMIT client_ip={} host={} path={} status=429",
            ip, host, path
        );
        return (StatusCode::TOO_MANY_REQUESTS, "Too Many Requests").into_response();
    }

    next.run(req).await
}

pub fn start_eviction_task(
    limiter: Arc<RateLimiter>,
    interval: Duration,
    max_age: Duration,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval_timer = tokio::time::interval(interval);
        loop {
            tokio::select! {
                _ = interval_timer.tick() => {
                    limiter.evict_stale(max_age);
                }
                _ = shutdown_rx.changed() => {
                    tracing::info!("rate limiter eviction task shutting down");
                    break;
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::test_fixtures;
    use std::net::Ipv6Addr;
    use std::time::Duration;

    fn make_limiter(rps: u32, burst: u32) -> Arc<RateLimiter> {
        let mut config = test_fixtures::test_dynamic_config();
        config.rate_limit = crate::config::RateLimitConfig {
            requests_per_second: rps,
            burst,
        };
        let config_arc = Arc::new(ArcSwap::from_pointee(config));
        Arc::new(RateLimiter::new(config_arc))
    }

    #[test]
    fn check_and_consume_allows_within_burst() {
        let limiter = make_limiter(10, 5);
        for _ in 0..5 {
            assert!(limiter.check_and_consume(IpAddr::from([192, 168, 1, 1])));
        }
    }

    #[test]
    fn check_and_consume_rejects_above_burst() {
        let limiter = make_limiter(10, 5);
        for _ in 0..5 {
            assert!(limiter.check_and_consume(IpAddr::from([192, 168, 1, 1])));
        }
        assert!(!limiter.check_and_consume(IpAddr::from([192, 168, 1, 1])));
    }

    #[test]
    fn check_and_consume_per_ip_independent() {
        let limiter = make_limiter(10, 5);
        for _ in 0..5 {
            assert!(limiter.check_and_consume(IpAddr::from([192, 168, 1, 1])));
        }
        assert!(!limiter.check_and_consume(IpAddr::from([192, 168, 1, 1])));
        assert!(limiter.check_and_consume(IpAddr::from([192, 168, 1, 2])));
    }

    #[test]
    fn check_and_consume_ipv6_normalized_to_64() {
        let limiter = make_limiter(10, 5);
        let ip1 = IpAddr::from(Ipv6Addr::new(
            0x2001, 0x0db8, 0x85a3, 0x0001, 0x1111, 0x2222, 0x3333, 0x4444,
        ));
        let ip2 = IpAddr::from(Ipv6Addr::new(
            0x2001, 0x0db8, 0x85a3, 0x0001, 0x5555, 0x6666, 0x7777, 0x8888,
        ));
        for _ in 0..5 {
            assert!(limiter.check_and_consume(ip1));
        }
        assert!(!limiter.check_and_consume(ip2));
    }

    #[test]
    fn evict_removes_stale_entries() {
        let limiter = make_limiter(10, 20);
        let ip1 = IpAddr::from([192, 168, 1, 1]);
        let ip2 = IpAddr::from([192, 168, 1, 2]);

        limiter.check_and_consume(ip1);
        std::thread::sleep(Duration::from_millis(50));
        limiter.check_and_consume(ip2);

        limiter.evict_stale(Duration::from_millis(25));
        assert!(!limiter.buckets.contains_key(&normalize_ip(ip1)));
        assert!(limiter.buckets.contains_key(&normalize_ip(ip2)));
    }

    #[test]
    fn config_reload_adopted_on_next_request() {
        let config = test_fixtures::test_dynamic_config();
        let config_arc = Arc::new(ArcSwap::from_pointee(config));
        let limiter = Arc::new(RateLimiter::new(config_arc.clone()));

        assert!(limiter.check_and_consume(IpAddr::from([192, 168, 1, 1])));

        let mut new_config = test_fixtures::test_dynamic_config();
        new_config.rate_limit = crate::config::RateLimitConfig {
            requests_per_second: 10,
            burst: 2,
        };
        config_arc.store(Arc::new(new_config));

        assert!(limiter.check_and_consume(IpAddr::from([192, 168, 1, 1])));
        assert!(limiter.check_and_consume(IpAddr::from([192, 168, 1, 1])));
        assert!(!limiter.check_and_consume(IpAddr::from([192, 168, 1, 1])));
    }

    #[tokio::test]
    async fn middleware_uses_connect_info_without_xff_header() {
        let limiter = make_limiter(10, 5);

        let app = axum::Router::new()
            .route("/", axum::routing::get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                limiter,
                rate_limit_middleware,
            ))
            .into_make_service_with_connect_info::<std::net::SocketAddr>();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = reqwest::Client::new();
        for _ in 0..5 {
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
    }

    #[tokio::test]
    async fn middleware_rejects_without_connect_info() {
        let limiter = make_limiter(10, 20);

        let app = axum::Router::new()
            .route("/", axum::routing::get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                limiter,
                rate_limit_middleware,
            ));

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let client = reqwest::Client::new();
        let resp = client
            .get(format!("http://127.0.0.1:{}/", addr.port()))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), reqwest::StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn middleware_ignores_xff_header_same_bucket() {
        let limiter = make_limiter(10, 2);

        let app = axum::Router::new()
            .route("/", axum::routing::get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(
                limiter,
                rate_limit_middleware,
            ))
            .into_make_service_with_connect_info::<std::net::SocketAddr>();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

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
}
