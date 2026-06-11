use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProxyError {
    #[error("upstream connection failed")]
    UpstreamConnection(#[source] hyper_util::client::legacy::Error),
    #[error("upstream timeout")]
    UpstreamTimeout,
    #[error("upstream tls certificate validation failed")]
    UpstreamTls(#[source] std::io::Error),
    #[error("no matching site for host")]
    UnknownHost,
    #[error("missing host header")]
    MissingHost,
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let (status, body) = match &self {
            ProxyError::UpstreamConnection(_) => (StatusCode::BAD_GATEWAY, "Bad Gateway"),
            ProxyError::UpstreamTimeout => (StatusCode::GATEWAY_TIMEOUT, "Gateway Timeout"),
            ProxyError::UpstreamTls(_) => (StatusCode::BAD_GATEWAY, "Bad Gateway"),
            ProxyError::UnknownHost => (StatusCode::NOT_FOUND, "Not Found"),
            ProxyError::MissingHost => (StatusCode::BAD_REQUEST, "Bad Request"),
        };

        tracing::warn!(
            error = %self,
            status = status.as_u16(),
            "proxy error"
        );

        (status, body).into_response()
    }
}
