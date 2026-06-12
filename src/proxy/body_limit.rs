use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use http_body_util::Limited;

use crate::config::DynamicConfig;

pub const DEFAULT_BODY_LIMIT_BYTES: u64 = 104_857_600;

pub async fn body_limit_middleware(
    State(config): State<Arc<ArcSwap<DynamicConfig>>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let limit = config.load().body.limit_bytes;
    let limit = if limit == 0 {
        DEFAULT_BODY_LIMIT_BYTES
    } else {
        limit
    };

    // Early rejection: if Content-Length is present and exceeds the limit, reject
    // immediately without reading the body. For requests without Content-Length
    // (chunked, HTTP/2), the Limited body wrapper below enforces the limit during
    // streaming. This is a two-layer defense.
    if let Some(content_length) = request.headers().get("content-length") {
        if let Ok(length_str) = content_length.to_str() {
            if let Ok(length) = length_str.parse::<u64>() {
                if length > limit {
                    return (StatusCode::PAYLOAD_TOO_LARGE, "Payload Too Large").into_response();
                }
            }
        }
    }

    let (parts, body) = request.into_parts();
    let limited_body = Limited::new(body, limit as usize);
    let request = axum::extract::Request::from_parts(parts, Body::new(limited_body));

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_body_limit_is_100mb() {
        assert_eq!(DEFAULT_BODY_LIMIT_BYTES, 104_857_600);
    }
}
