use std::{net::SocketAddr, sync::Arc};

use axum::{
    Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use trex_sandbox::Sandbox;

use crate::{api::AppState, runs};

const TUNNEL_BUFFER_BYTES: usize = 64 * 1024;
const PREVIEW_ID_LENGTH: usize = 32;

// serves apps running in sandboxes; the preview id is the host's first label, so each preview is
// its own origin, apart from the trex app and from other previews
pub async fn serve(state: Arc<AppState>, addr: SocketAddr) -> anyhow::Result<()> {
    let listener = TcpListener::bind(addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "serving previews");
    let shutdown = state.shutdown.clone();
    let router = Router::new().fallback(proxy).with_state(state);
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown.cancelled_owned())
        .await?;
    Ok(())
}

async fn proxy(State(state): State<Arc<AppState>>, request: Request) -> Response {
    let Some(id) = preview_id(&request) else {
        return page(StatusCode::NOT_FOUND, "This preview link isn't valid.");
    };
    let preview = match state.store.preview(&id).await {
        Ok(Some(preview)) => preview,
        Ok(None) => {
            return page(
                StatusCode::NOT_FOUND,
                "This preview link has expired. Open the preview again from the chat.",
            );
        }
        Err(error) => return failure(error),
    };
    let session = match state
        .store
        .session(preview.workspace, preview.session)
        .await
    {
        Ok(Some(session)) => session,
        Ok(None) => return page(StatusCode::NOT_FOUND, "This chat no longer exists."),
        Err(error) => return failure(error),
    };
    let sandbox = match runs::files_sandbox(&state, preview.workspace, &session, false).await {
        Ok(Some(sandbox)) => sandbox,
        Ok(None) => {
            return page(
                StatusCode::BAD_GATEWAY,
                "This chat's sandbox isn't available. Ask the agent to start the app again.",
            );
        }
        Err(error) => return failure(error),
    };
    match forward(&state, sandbox, preview.port, request).await {
        Ok(response) => response,
        Err(error) => {
            tracing::debug!(
                port = preview.port,
                error = format!("{error:#}"),
                "preview request failed"
            );
            page(
                StatusCode::BAD_GATEWAY,
                &format!(
                    "Nothing is answering on port {} in the sandbox yet. Start the server, then reload.",
                    preview.port
                ),
            )
        }
    }
}

// each request gets its own tunnel: the gateway only ends a tunnel once both sides close, so hyper
// reads exactly one response and then lets the tunnel go
async fn forward(
    state: &Arc<AppState>,
    sandbox: Sandbox,
    port: u16,
    mut request: Request,
) -> anyhow::Result<Response> {
    let (near, far) = tokio::io::duplex(TUNNEL_BUFFER_BYTES);
    let state_for_tunnel = state.clone();
    tokio::spawn(async move {
        if let Err(error) = state_for_tunnel
            .openshell
            .forward(&sandbox, port, Vec::new(), far)
            .await
        {
            tracing::debug!(port, error = format!("{error:#}"), "preview tunnel closed");
        }
    });
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(near)).await?;
    tokio::spawn(async move {
        if let Err(error) = connection.with_upgrades().await {
            tracing::debug!(%error, "preview connection ended");
        }
    });

    // dev servers check the host header, so they see the address they were started on
    let host = HeaderValue::from_str(&format!("localhost:{port}"))?;
    request.headers_mut().insert(header::HOST, host);
    let upgrading = request.headers().contains_key(header::UPGRADE);
    let client_upgrade = upgrading.then(|| hyper::upgrade::on(&mut request));

    let mut response = sender.send_request(request).await?;
    if response.status() == StatusCode::SWITCHING_PROTOCOLS
        && let Some(client_upgrade) = client_upgrade
    {
        // websockets, such as a dev server's hot reload, are piped both ways once both sides switch
        let server_upgrade = hyper::upgrade::on(&mut response);
        tokio::spawn(async move {
            let (Ok(client), Ok(server)) = (client_upgrade.await, server_upgrade.await) else {
                return;
            };
            let (mut client, mut server) = (TokioIo::new(client), TokioIo::new(server));
            let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
        });
    }
    Ok(response.map(Body::new))
}

fn preview_id(request: &Request) -> Option<String> {
    let host = request.headers().get(header::HOST)?.to_str().ok()?;
    let id = host.split('.').next()?.to_ascii_lowercase();
    (id.len() == PREVIEW_ID_LENGTH && id.chars().all(|char| char.is_ascii_hexdigit())).then_some(id)
}

pub fn new_preview_id() -> anyhow::Result<String> {
    let mut bytes = [0u8; PREVIEW_ID_LENGTH / 2];
    getrandom::fill(&mut bytes)
        .map_err(|error| anyhow::anyhow!("failed to generate a preview id: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn failure(error: anyhow::Error) -> Response {
    tracing::warn!(error = format!("{error:#}"), "failed to serve a preview");
    page(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Something went wrong opening this preview.",
    )
}

fn page(status: StatusCode, message: &str) -> Response {
    let body = format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content='width=device-width'><title>Preview</title>\
         <body style='font:14px system-ui,sans-serif;color:#666;display:grid;place-items:center;height:100vh;margin:0'>\
         <p style='max-width:28rem;text-align:center'>{}</p>",
        message.replace('&', "&amp;").replace('<', "&lt;")
    );
    (status, Html(body)).into_response()
}
