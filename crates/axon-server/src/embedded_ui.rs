//! Embedded admin UI — assets from `ui/build` compiled into the binary.
//!
//! The `UiAssets` struct embeds every file under `ui/build` at compile time.
//! The axum handler serves them at `/ui/*path` with correct `Content-Type`
//! headers, and falls back to `index.html` for any unrecognised path so that
//! SvelteKit client-side routing works correctly.
//!
//! Release and installer builds should run `cd ui && bun run build` before
//! compiling the server. Plain Rust-only checks use a small compile-time
//! fallback so the server still builds from a fresh checkout.

use std::borrow::Cow;

use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
#[cfg(axon_embed_ui_bundle)]
use rust_embed::RustEmbed;

/// All files under `ui/build`, embedded at compile time.
///
/// The `#[folder]` path is relative to the crate root (`crates/axon-server/`),
/// so `../../ui/build` resolves to `ui/build` in the workspace root.
#[cfg(axon_embed_ui_bundle)]
#[derive(RustEmbed)]
#[folder = "../../ui/build"]
struct UiAssets;

#[cfg(axon_embed_ui_bundle)]
fn embedded_asset(path: &str) -> Option<Cow<'static, [u8]>> {
    UiAssets::get(path).map(|content| content.data)
}

#[cfg(not(axon_embed_ui_bundle))]
fn embedded_asset(path: &str) -> Option<Cow<'static, [u8]>> {
    const FALLBACK_INDEX_HTML: &[u8] = br#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>Axon Admin UI</title></head>
<body><main>Axon admin UI bundle was not built.</main><script type="module" src="/ui/_app/env.js"></script></body>
</html>
"#;
    const FALLBACK_ENV_JS: &[u8] = b"export {};\n";

    match path {
        "index.html" => Some(Cow::Borrowed(FALLBACK_INDEX_HTML)),
        "_app/env.js" => Some(Cow::Borrowed(FALLBACK_ENV_JS)),
        _ => None,
    }
}

/// Axum handler for `GET /ui` and `GET /ui/*path`.
///
/// Strips the `/ui` prefix, looks up the embedded file, and responds
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
    match embedded_asset(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        None => {
            // SPA fallback: let the client-side router handle unknown paths.
            match embedded_asset("index.html") {
                Some(index) => Response::builder()
                    .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(Body::from(index))
                    .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response()),
                None => StatusCode::NOT_FOUND.into_response(),
            }
        }
    }
}
