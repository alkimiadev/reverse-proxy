use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;

async fn health_handler() -> impl IntoResponse {
    StatusCode::OK
}

pub fn health_route() -> Router {
    Router::new().route("/health", get(health_handler))
}
