mod auth;
pub mod error;
pub mod events;
mod ids;
mod library;
mod sessions;

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{Request, StatusCode},
    routing::{get, post},
};
use serde::Serialize;
use serde_json::{Value, json};
use tower_http::{
    LatencyUnit,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use trex_harness::{model::Models, tool::Tools};
use trex_sandbox::{OpenShell, Policy};
use trex_store::{Store, library::Library};

use crate::runs::Runs;

const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;

pub struct AppState {
    pub store: Store,
    pub openshell: OpenShell,
    pub models: Models,
    pub tools: Tools,
    pub library: Library,
    pub sandbox_image: String,
    pub sandbox_policy: Policy,
    pub runs: Runs,
}

pub fn router(state: Arc<AppState>) -> Router {
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
    let v1 = Router::new()
        .route("/models", get(models))
        .route("/sessions", post(sessions::create).get(sessions::list))
        .route(
            "/sessions/{id}",
            get(sessions::get).delete(sessions::delete),
        )
        .route("/sessions/{id}/items", get(sessions::items))
        .route("/sessions/{id}/messages", post(sessions::create_message))
        .route("/sessions/{id}/answers", post(sessions::create_answers))
        .route("/sessions/{id}/cancel", post(sessions::cancel))
        .route("/sessions/{id}/events", get(events::stream_events))
        .route("/sessions/{id}/access_requests", get(sessions::list_access))
        .route(
            "/sessions/{id}/access_requests/{request_id}/approve",
            post(sessions::approve_access),
        )
        .route(
            "/sessions/{id}/access_requests/{request_id}/reject",
            post(sessions::reject_access),
        )
        .route("/library", get(library::list))
        .route(
            "/library/files/{*path}",
            get(library::download)
                .put(library::upload)
                .delete(library::delete)
                .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        );

    Router::new()
        .route("/health", get(health))
        .nest("/v1", v1)
        .with_state(state)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(trace)
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    version: &'static str,
}

async fn models(State(state): State<Arc<AppState>>) -> Json<Value> {
    let mut ids: Vec<&str> = state.models.ids().collect();
    ids.sort_unstable();
    let data: Vec<Value> = ids
        .into_iter()
        .map(|id| json!({"id": id, "object": "model"}))
        .collect();
    Json(json!({"object": "list", "data": data, "has_more": false}))
}

async fn health(State(state): State<Arc<AppState>>) -> (StatusCode, Json<Health>) {
    let (code, status) = match state.store.ping().await {
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
