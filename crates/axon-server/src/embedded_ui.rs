//! Embedded admin UI — assets from `ui/build` compiled into the binary.
//!
//! The `UiAssets` struct embeds every file under `ui/build` at compile time.
//! The axum handler serves them at `/ui/*path` with correct `Content-Type`
//! headers, and falls back to `index.html` for any unrecognised path so that
//! SvelteKit client-side routing works correctly.
//!
//! **Build prerequisite**: `ui/build` must exist before `cargo build`.
//! Run `cd ui && bun run build` (or `npm run build`) first.

use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// All files under `ui/build`, embedded at compile time.
///
/// The `#[folder]` path is relative to the crate root (`crates/axon-server/`),
/// so `../../ui/build` resolves to `ui/build` in the workspace root.
#[derive(RustEmbed)]
#[folder = "../../ui/build"]
struct UiAssets;

/// Axum handler for `GET /ui` and `GET /ui/*path`.
///
/// Strips the `/ui` prefix, looks up the file in [`UiAssets`], and responds
/// with the correct `Content-Type`.  Unknown paths fall back to `index.html`
/// so SvelteKit's client-side router can handle them.
pub async fn embedded_ui_handler(uri: Uri) -> Response {
    let raw = uri.path();
    // Strip the /ui prefix that axum leaves on the URI when not using nest_service.
    let asset_path = raw
        .strip_prefix("/ui/")
        .or_else(|| raw.strip_prefix("/ui"))
        .unwrap_or(raw);
    let asset_path = if asset_path.is_empty() {
        "index.html"
    } else {
        asset_path
    };

    serve_asset(asset_path)
}

fn serve_asset(path: &str) -> Response {
    match UiAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        None => {
            // SPA fallback: let the client-side router handle unknown paths.
            match UiAssets::get("index.html") {
                Some(index) => Response::builder()
                    .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(Body::from(index.data))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}
