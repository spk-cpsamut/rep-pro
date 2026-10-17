pub mod orchestrator;
pub mod resource;
pub mod resource_map;
pub mod sanitizer;
pub mod state;

mod http;

pub use state::AppState;

use std::time::Duration;

use axum::extract::MatchedPath;
use axum::http::{HeaderName, Request, Response};
use tower_http::request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer};
use tower_http::trace::TraceLayer;

pub fn router(state: AppState) -> axum::Router {
    let request_id = HeaderName::from_static("x-request-id");
    axum::Router::new()
        .merge(resource::handlers::router())
        .merge(resource_map::handlers::router())
        .with_state(state)
        .layer(PropagateRequestIdLayer::new(request_id.clone()))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(http_span)
                .on_response(http_response),
        )
        .layer(SetRequestIdLayer::new(request_id, MakeRequestUuid))
}

fn http_span(request: &Request<axum::body::Body>) -> tracing::Span {
    let request_id = request
        .headers()
        .get("x-request-id")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let matched_path = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str)
        .unwrap_or_else(|| request.uri().path());
    tracing::info_span!(
        "http",
        method = %request.method(),
        matched_path,
        request_id = %request_id,
        status = tracing::field::Empty,
    )
}

fn http_response(response: &Response<axum::body::Body>, latency: Duration, span: &tracing::Span) {
    let status = response.status().as_u16();
    span.record("status", status);
    let latency_ms = u64::try_from(latency.as_millis()).unwrap_or(u64::MAX);
    match status {
        500.. => tracing::error!(status, latency_ms, "response"),
        400..=499 => tracing::warn!(status, latency_ms, "response"),
        _ => tracing::info!(status, latency_ms, "response"),
    }
}
