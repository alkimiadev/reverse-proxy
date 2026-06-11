pub mod body_limit;
pub mod error;
pub mod handler;
pub mod headers;

pub use crate::config::dynamic_config::normalize_host;
pub use handler::{create_http_client, create_https_client, proxy_router, ProxyState};

use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::config::DynamicConfig;
use crate::rate_limit::RateLimiter;

pub fn build_router(
    proxy_state: Arc<ProxyState>,
    config: Arc<ArcSwap<DynamicConfig>>,
    rate_limiter: Arc<RateLimiter>,
) -> axum::Router {
    let router = proxy_router(proxy_state);
    let router = router_with_body_limit(router, config);
    router_with_rate_limit(router, rate_limiter)
}

pub fn router_with_body_limit(
    router: axum::Router,
    config: Arc<ArcSwap<DynamicConfig>>,
) -> axum::Router {
    router.layer(axum::middleware::from_fn_with_state(
        config,
        body_limit::body_limit_middleware,
    ))
}

pub fn router_with_rate_limit(
    router: axum::Router,
    rate_limiter: Arc<RateLimiter>,
) -> axum::Router {
    router.layer(axum::middleware::from_fn_with_state(
        rate_limiter,
        crate::rate_limit::rate_limit_middleware,
    ))
}
