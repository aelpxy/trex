use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::{Request, StatusCode},
    routing::get,
};
use serde::Serialize;
use tower_http::{
    LatencyUnit,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use trex_store::Store;

pub fn router(store: Arc<Store>) -> Router {
    let trace = TraceLayer::new_for_http()
        .make_span_with(|req: &Request<_>| {
            let request_id = req
                .headers()
                .get("x-request-id")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();

            tracing::info_span!(
                "request",
                method = %req.method(),
                path = req.uri().path(),
                request_id,
            )
        })
        .on_response(
            DefaultOnResponse::new()
                .level(Level::INFO)
                .latency_unit(LatencyUnit::Millis),
        );

    // layers wrap bottom-up: the id is set before tracing and copied to the response after
    Router::new()
        .route("/health", get(health))
        .with_state(store)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(trace)
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

async fn health(State(store): State<Arc<Store>>) -> (StatusCode, Json<Health>) {
    let (code, status) = match store.ping().await {
        Ok(()) => (StatusCode::OK, "ok"),
        Err(error) => {
            tracing::error!(error = format!("{error:#}"), "health check failed");
            (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
        }
    };
    let health = Health {
        status,
        version: env!("CARGO_PKG_VERSION"),
    };
    (code, Json(health))
}
