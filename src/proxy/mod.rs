pub mod body_limit;
pub mod error;
pub mod handler;
pub mod headers;

use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::config::DynamicConfig;

pub fn router_with_body_limit(
    router: axum::Router,
    config: Arc<ArcSwap<DynamicConfig>>,
) -> axum::Router {
    router.layer(axum::middleware::from_fn_with_state(
        config,
        body_limit::body_limit_middleware,
    ))
}
