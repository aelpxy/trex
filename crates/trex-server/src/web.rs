use axum::{
    http::{HeaderValue, StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
};
use rust_embed::Embed;

use crate::api::error::ApiError;

// the built web app, embedded at compile time in release builds and read from disk in debug ones;
// a build without it still compiles and serves only the api
#[derive(Embed)]
#[folder = "../../frontend/build/client"]
#[allow_missing = true]
struct App;

const INDEX: &str = "index.html";
const API_PREFIX: &str = "v1/";
// vite names everything under assets/ by content hash, so a cached copy is never stale
const HASHED_ASSETS: &str = "assets/";
const IMMUTABLE: &str = "public, max-age=31536000, immutable";
const REVALIDATE: &str = "no-cache";
const NOT_BUILT: &str = "<!doctype html><meta charset=utf-8><title>trex</title>\
    <p style='font:14px system-ui,sans-serif;margin:3rem'>The web app isn't built into this server. \
    Run <code>pnpm --dir frontend build</code>, then build trex again.";

// serves the app's files, and index.html for every other path so client-side routes load on refresh
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(file) = App::get(path).filter(|_| !path.is_empty()) {
        let cache = if path.starts_with(HASHED_ASSETS) {
            IMMUTABLE
        } else {
            REVALIDATE
        };
        return with_headers(file.data.into_owned(), file.metadata.mimetype(), cache);
    }
    // unknown api paths get the api's own error, and missing assets a plain 404, not the app
    if path.starts_with(API_PREFIX) {
        return ApiError::NotFound(format!("no endpoint {}", uri.path())).into_response();
    }
    if path.starts_with(HASHED_ASSETS) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match App::get(INDEX) {
        Some(index) => with_headers(
            index.data.into_owned(),
            "text/html; charset=utf-8",
            REVALIDATE,
        ),
        None => (StatusCode::NOT_FOUND, Html(NOT_BUILT)).into_response(),
    }
}

fn with_headers(body: Vec<u8>, content_type: &str, cache: &'static str) -> Response {
    let content_type = HeaderValue::from_str(content_type)
        .unwrap_or(HeaderValue::from_static("application/octet-stream"));
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
        ],
        body,
    )
        .into_response()
}
