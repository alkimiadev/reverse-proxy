use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

#[derive(Debug, thiserror::Error)]
pub enum ProxyError {
    #[error("Bad Gateway")]
    BadGateway { host: String, upstream: String },
    #[error("Gateway Timeout")]
    GatewayTimeout { host: String, upstream: String },
    #[error("Payload Too Large")]
    PayloadTooLarge,
    #[error("Too Many Requests")]
    TooManyRequests {
        client_ip: String,
        host: String,
        path: String,
    },
    #[error("Not Found")]
    NotFound,
    #[error("Bad Request")]
    BadRequest,
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

impl ProxyError {
    fn status_code(&self) -> StatusCode {
        match self {
            Self::BadGateway { .. } => StatusCode::BAD_GATEWAY,
            Self::GatewayTimeout { .. } => StatusCode::GATEWAY_TIMEOUT,
            Self::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            Self::TooManyRequests { .. } => StatusCode::TOO_MANY_REQUESTS,
            Self::NotFound | Self::UnknownHost => StatusCode::NOT_FOUND,
            Self::BadRequest | Self::MissingHost => StatusCode::BAD_REQUEST,
            Self::UpstreamConnection(_) => StatusCode::BAD_GATEWAY,
            Self::UpstreamTimeout => StatusCode::GATEWAY_TIMEOUT,
            Self::UpstreamTls(_) => StatusCode::BAD_GATEWAY,
        }
    }

    fn body(&self) -> &'static str {
        match self {
            Self::BadGateway { .. } => "Bad Gateway",
            Self::GatewayTimeout { .. } => "Gateway Timeout",
            Self::PayloadTooLarge => "Payload Too Large",
            Self::TooManyRequests { .. } => "Too Many Requests",
            Self::NotFound | Self::UnknownHost => "Not Found",
            Self::BadRequest | Self::MissingHost => "Bad Request",
            Self::UpstreamConnection(_) => "Bad Gateway",
            Self::UpstreamTimeout => "Gateway Timeout",
            Self::UpstreamTls(_) => "Bad Gateway",
        }
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        match &self {
            Self::BadGateway { host, upstream } => {
                tracing::warn!(
                    host = %host,
                    upstream = %upstream,
                    status = 502,
                    "Bad Gateway"
                );
            }
            Self::GatewayTimeout { host, upstream } => {
                tracing::warn!(
                    host = %host,
                    upstream = %upstream,
                    status = 504,
                    "Gateway Timeout"
                );
            }
            Self::TooManyRequests {
                client_ip,
                host,
                path,
            } => {
                tracing::info!(
                    "RATE_LIMIT client_ip={} host={} path={} status=429",
                    client_ip,
                    host,
                    path
                );
            }
            Self::UpstreamConnection(e) => {
                tracing::warn!(error = %e, status = 502, "upstream connection failed");
            }
            Self::UpstreamTimeout => {
                tracing::warn!(status = 504, "upstream timeout");
            }
            Self::UpstreamTls(e) => {
                tracing::warn!(error = %e, status = 502, "upstream TLS error");
            }
            _ => {}
        }

        (
            [(
                axum::http::header::CONTENT_TYPE,
                "text/plain; charset=utf-8",
            )],
            (self.status_code(), self.body()),
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Response, StatusCode};

    fn into_response(error: ProxyError) -> Response<Body> {
        let _guard = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .try_init();
        error.into_response()
    }

    #[tokio::test]
    async fn bad_gateway_response() {
        let resp = into_response(ProxyError::BadGateway {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
        });
        assert_eq!(resp.status(), StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"Bad Gateway");
    }

    #[tokio::test]
    async fn bad_gateway_content_type() {
        let resp = into_response(ProxyError::BadGateway {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
        });
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
    }

    #[tokio::test]
    async fn gateway_timeout_response() {
        let resp = into_response(ProxyError::GatewayTimeout {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
        });
        assert_eq!(resp.status(), StatusCode::GATEWAY_TIMEOUT);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"Gateway Timeout");
    }

    #[tokio::test]
    async fn gateway_timeout_content_type() {
        let resp = into_response(ProxyError::GatewayTimeout {
            host: "example.com".to_string(),
            upstream: "127.0.0.1:8080".to_string(),
        });
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
    }

    #[tokio::test]
    async fn payload_too_large_response() {
        let resp = into_response(ProxyError::PayloadTooLarge);
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"Payload Too Large");
    }

    #[tokio::test]
    async fn payload_too_large_content_type() {
        let resp = into_response(ProxyError::PayloadTooLarge);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
    }

    #[tokio::test]
    async fn too_many_requests_response() {
        let resp = into_response(ProxyError::TooManyRequests {
            client_ip: "192.168.1.1".to_string(),
            host: "example.com".to_string(),
            path: "/api".to_string(),
        });
        assert_eq!(resp.status(), StatusCode::TOO_MANY_REQUESTS);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"Too Many Requests");
    }

    #[tokio::test]
    async fn too_many_requests_content_type() {
        let resp = into_response(ProxyError::TooManyRequests {
            client_ip: "192.168.1.1".to_string(),
            host: "example.com".to_string(),
            path: "/api".to_string(),
        });
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
    }

    #[tokio::test]
    async fn not_found_response() {
        let resp = into_response(ProxyError::NotFound);
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"Not Found");
    }

    #[tokio::test]
    async fn not_found_content_type() {
        let resp = into_response(ProxyError::NotFound);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
    }

    #[tokio::test]
    async fn bad_request_response() {
        let resp = into_response(ProxyError::BadRequest);
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], b"Bad Request");
    }

    #[tokio::test]
    async fn bad_request_content_type() {
        let resp = into_response(ProxyError::BadRequest);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/plain; charset=utf-8"
        );
    }

    #[test]
    fn error_display_matches_body() {
        assert_eq!(
            ProxyError::BadGateway {
                host: String::new(),
                upstream: String::new()
            }
            .to_string(),
            "Bad Gateway"
        );
        assert_eq!(
            ProxyError::GatewayTimeout {
                host: String::new(),
                upstream: String::new()
            }
            .to_string(),
            "Gateway Timeout"
        );
        assert_eq!(ProxyError::PayloadTooLarge.to_string(), "Payload Too Large");
        assert_eq!(
            ProxyError::TooManyRequests {
                client_ip: String::new(),
                host: String::new(),
                path: String::new()
            }
            .to_string(),
            "Too Many Requests"
        );
        assert_eq!(ProxyError::NotFound.to_string(), "Not Found");
        assert_eq!(ProxyError::BadRequest.to_string(), "Bad Request");
    }
}
