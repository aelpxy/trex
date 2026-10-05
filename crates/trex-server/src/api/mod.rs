mod account;
mod admin;
mod attachments;
mod auth;
mod credits;
pub mod error;
pub mod events;
mod files;
mod ids;
mod library;
mod previews;
mod projects;
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
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};
use utoipa_axum::{router::OpenApiRouter, routes};

use crate::{config::PreviewUrl, credits::Plans, runs::Runs};

const MAX_UPLOAD_BYTES: usize = 100 * 1024 * 1024;
const MAX_SANDBOX_FILE_BYTES: usize = trex_harness::files::MAX_FILE_BYTES as usize;
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const DOCS_PAGE: &str = include_str!("docs.html");

pub struct AppState {
    pub store: Store,
    pub openshell: OpenShell,
    pub models: Models,
    pub plans: Plans,
    pub tools: Tools,
    pub library: Library,
    pub sandbox_image: String,
    pub sandbox_policy: Policy,
    pub runs: Runs,
    pub preview_url: PreviewUrl,
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
        .routes(routes!(account::signup))
        .routes(routes!(account::login))
        .routes(routes!(account::logout))
        .routes(routes!(account::me, account::update_me))
        .routes(routes!(account::change_password))
        .routes(routes!(credits::plans))
        .routes(routes!(credits::get))
        .routes(routes!(credits::ledger))
        .routes(routes!(admin::list_workspaces))
        .routes(routes!(admin::adjust_credits))
        .routes(routes!(admin::set_plan))
        .routes(routes!(models))
        .routes(routes!(projects::create, projects::list))
        .routes(routes!(projects::get, projects::update, projects::delete))
        .routes(routes!(sessions::create, sessions::list))
        .routes(routes!(sessions::get, sessions::update, sessions::delete))
        .routes(routes!(sessions::items))
        .routes(routes!(sessions::usage))
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
        .routes(routes!(sessions::retry))
        .routes(routes!(sessions::branch))
        .routes(routes!(events::stream_events))
        .routes(routes!(sessions::list_access))
        .routes(routes!(sessions::approve_access))
        .routes(routes!(sessions::reject_access))
        .routes(routes!(previews::create))
        .routes(routes!(files::list))
        .routes(routes!(files::move_file))
        .route(
            "/sessions/{id}/files/{*path}",
            get(files::download)
                .put(files::upload)
                .delete(files::delete)
                .layer(DefaultBodyLimit::max(MAX_SANDBOX_FILE_BYTES)),
        )
        .routes(routes!(library::list))
        .routes(routes!(library::move_file))
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
    spec.merge(SandboxFiles::openapi());
    (router, spec)
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "trex",
        description = "Sessions, agent runs and sandboxes for the trex web UI. Sign up or log in for a bearer token and send it as `Authorization: Bearer <token>`; requests act in the workspace named by the `Trex-Workspace` header, or the user's first workspace. Errors always have the shape `{\"error\": {\"type\", \"message\", \"param\"}}`; every response carries an `x-request-id` header.",
    ),
    components(schemas(error::ErrorType)),
    modifiers(&BearerToken),
    security(("token" = [])),
)]
struct ApiDoc;

#[derive(OpenApi)]
#[openapi(paths(library::download, library::upload, library::delete))]
struct LibraryFiles;

#[derive(OpenApi)]
#[openapi(paths(files::download, files::upload, files::delete))]
struct SandboxFiles;

struct BearerToken;

impl Modify for BearerToken {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_default();
        let scheme = HttpBuilder::new()
            .scheme(HttpAuthScheme::Bearer)
            .description(Some(
                "A token from POST /v1/auth/signup or /v1/auth/login; /v1/admin needs a user with the admin role.",
            ))
            .build();
        components.add_security_scheme("token", SecurityScheme::Http(scheme));
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
    price: Price,
}

/// Credits per million tokens.
#[derive(Serialize, ToSchema)]
struct Price {
    input: u64,
    cached_input: u64,
    output: u64,
    /// Fast mode multiplies the price by this.
    #[schema(example = 2.0)]
    fast_multiplier: f64,
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
            price: {
                let price = model.price();
                Price {
                    input: price.input,
                    cached_input: price.cached_input,
                    output: price.output,
                    fast_multiplier: price.fast_multiplier,
                }
            },
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
                "/v1/auth/signup",
                "/v1/auth/login",
                "/v1/auth/logout",
                "/v1/me",
                "/v1/me/password",
                "/v1/plans",
                "/v1/credits",
                "/v1/credits/ledger",
                "/v1/admin/workspaces",
                "/v1/admin/workspaces/{id}/credits",
                "/v1/admin/workspaces/{id}/plan",
                "/v1/models",
                "/v1/projects",
                "/v1/projects/{id}",
                "/v1/sessions",
                "/v1/sessions/{id}",
                "/v1/sessions/{id}/items",
                "/v1/sessions/{id}/usage",
                "/v1/sessions/{id}/messages",
                "/v1/sessions/{id}/answers",
                "/v1/sessions/{id}/cancel",
                "/v1/sessions/{id}/retry",
                "/v1/sessions/{id}/branch",
                "/v1/sessions/{id}/events",
                "/v1/sessions/{id}/access_requests",
                "/v1/sessions/{id}/access_requests/{request_id}/approve",
                "/v1/sessions/{id}/access_requests/{request_id}/reject",
                "/v1/sessions/{id}/previews",
                "/v1/sessions/{id}/files",
                "/v1/sessions/{id}/files/move",
                "/v1/library",
                "/v1/library/move",
                "/v1/attachments/{id}",
                "/v1/library/files/{path}",
                "/v1/sessions/{id}/files/{path}",
            ]
        );
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(
            json["components"]["securitySchemes"]["token"]["scheme"],
            "bearer"
        );
        assert!(json["components"]["schemas"]["SessionEvent"]["oneOf"].is_array());
    }
}
