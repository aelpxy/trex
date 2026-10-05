mod attachments;
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
    response::Html,
    routing::get,
};
use serde::Serialize;
use tower_http::{
    LatencyUnit,
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::Level;
use trex_harness::{model::Models, tool::Tools};
use trex_sandbox::{OpenShell, Policy};
use trex_store::{Store, library::Library};
use utoipa::{
    Modify, OpenApi, ToSchema,
    openapi::security::{ApiKey, ApiKeyValue, SecurityScheme},
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::runs::Runs;

const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const DOCS_PAGE: &str = include_str!("docs.html");

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

    let (router, spec) = routes();
    let spec = Arc::new(spec);

    // layers wrap bottom-up: the id is set before tracing and copied to the response after
    router
        .route("/openapi.json", get(move || async move { Json(spec) }))
        .route("/docs", get(|| async { Html(DOCS_PAGE) }))
        .with_state(state)
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(trace)
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
}

// the api routes and the openapi spec describing them, built from the same handler list
fn routes() -> (Router<Arc<AppState>>, utoipa::openapi::OpenApi) {
    let v1 = OpenApiRouter::new()
        .routes(routes!(models))
        .routes(routes!(sessions::create, sessions::list))
        .routes(routes!(sessions::get, sessions::delete))
        .routes(routes!(sessions::items))
        .routes({
            // attachments arrive inline as base64, far beyond the default json limit
            let (schemas, paths, method) = routes!(sessions::create_message);
            (
                schemas,
                paths,
                method.layer(DefaultBodyLimit::max(MAX_MESSAGE_BYTES)),
            )
        })
        .routes(routes!(sessions::create_answers))
        .routes(routes!(sessions::cancel))
        .routes(routes!(events::stream_events))
        .routes(routes!(sessions::list_access))
        .routes(routes!(sessions::approve_access))
        .routes(routes!(sessions::reject_access))
        .routes(routes!(library::list))
        .routes(routes!(attachments::download))
        .route(
            "/library/files/{*path}",
            get(library::download)
                .put(library::upload)
                .delete(library::delete)
                .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)),
        );

    let (router, mut spec) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health))
        .nest("/v1", v1)
        .split_for_parts();
    spec.merge(LibraryFiles::openapi());
    (router, spec)
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "trex",
        description = "Sessions, agent runs and sandboxes for the trex web UI. Errors always have the shape `{\"error\": {\"type\", \"message\", \"param\"}}`; every response carries an `x-request-id` header.",
    ),
    components(schemas(error::ErrorType)),
    modifiers(&UserHeader),
    security(("user" = [])),
)]
struct ApiDoc;

#[derive(OpenApi)]
#[openapi(paths(library::download, library::upload, library::delete))]
struct LibraryFiles;

struct UserHeader;

impl Modify for UserHeader {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_default();
        let scheme = ApiKey::Header(ApiKeyValue::with_description(
            "X-Trex-User",
            "Temporary: the user's uuid, until registration and api keys exist.",
        ));
        components.add_security_scheme("user", SecurityScheme::ApiKey(scheme));
    }
}

#[derive(Serialize, ToSchema)]
pub struct List<T> {
    #[schema(example = "list")]
    object: &'static str,
    data: Vec<T>,
    has_more: bool,
}

impl<T> List<T> {
    pub fn new(data: Vec<T>, has_more: bool) -> Self {
        Self {
            object: "list",
            data,
            has_more,
        }
    }
}

#[derive(Serialize, ToSchema)]
struct Model {
    #[schema(example = "gpt-6.1-sol")]
    id: String,
    #[schema(example = "model")]
    object: &'static str,
    /// Display name for model pickers.
    #[schema(example = "GPT 6.1 Sol")]
    name: String,
    /// Tokens; the conversation is compacted at 80% of it.
    context_window: u64,
    /// The reasoning effort levels a session may pick; null when the model accepts any.
    #[schema(example = json!(["low", "medium", "high", "xhigh", "max"]))]
    reasoning_efforts: Option<Vec<String>>,
    /// Whether sessions can turn on fast mode.
    fast: bool,
}

#[derive(Serialize, ToSchema)]
struct Health {
    #[schema(example = "ok")]
    status: &'static str,
    version: &'static str,
}

/// List models
///
/// The models a session can use, configured by the operator, in the operator's order.
#[utoipa::path(
    get,
    operation_id = "list_models",
    path = "/models",
    tag = "models",
    responses((status = 200, body = List<Model>)),
)]
async fn models(State(state): State<Arc<AppState>>) -> Json<List<Model>> {
    let data = state
        .models
        .all()
        .map(|model| Model {
            id: model.id().to_owned(),
            object: "model",
            name: model.name().to_owned(),
            context_window: model.context_window(),
            reasoning_efforts: model.reasoning_efforts().map(<[String]>::to_vec),
            fast: model.supports_fast(),
        })
        .collect();
    Json(List::new(data, false))
}

/// Health check
///
/// Reports whether Postgres and Redis are reachable.
#[utoipa::path(
    get,
    operation_id = "health",
    path = "/health",
    tag = "system",
    security(()),
    responses(
        (status = 200, body = Health),
        (status = 503, body = Health),
    ),
)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_documents_every_route() {
        let (_, spec) = routes();
        let paths: Vec<&str> = spec.paths.paths.keys().map(String::as_str).collect();
        assert_eq!(
            paths,
            [
                "/health",
                "/v1/models",
                "/v1/sessions",
                "/v1/sessions/{id}",
                "/v1/sessions/{id}/items",
                "/v1/sessions/{id}/messages",
                "/v1/sessions/{id}/answers",
                "/v1/sessions/{id}/cancel",
                "/v1/sessions/{id}/events",
                "/v1/sessions/{id}/access_requests",
                "/v1/sessions/{id}/access_requests/{request_id}/approve",
                "/v1/sessions/{id}/access_requests/{request_id}/reject",
                "/v1/library",
                "/v1/attachments/{id}",
                "/v1/library/files/{path}",
            ]
        );
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(
            json["components"]["securitySchemes"]["user"]["name"],
            "X-Trex-User"
        );
        assert!(json["components"]["schemas"]["SessionEvent"]["oneOf"].is_array());
    }
}
