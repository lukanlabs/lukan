use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../desktop-client/dist/"]
struct Asset;

/// Serve embedded static files (the React SPA).
pub async fn serve_static(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try the exact path first
    if let Some(content) = Asset::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            content.data.to_vec(),
        )
            .into_response();
    }

    // SPA fallback: serve index.html for client-side routing
    if let Some(content) = Asset::get("index.html") {
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html")],
            content.data.to_vec(),
        )
            .into_response();
    }

    (StatusCode::NOT_FOUND, "Not Found").into_response()
}
