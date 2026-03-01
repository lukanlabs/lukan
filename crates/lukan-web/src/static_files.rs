use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../desktop-client/dist"]
struct DesktopAssets;

/// Serve static files from the embedded desktop-client build.
/// Falls back to index.html for SPA routing.
pub async fn serve_static(req: Request) -> Response {
    let path = req.uri().path().trim_start_matches('/');

    // Try the exact path first
    if let Some(file) = DesktopAssets::get(path) {
        return file_response(path, &file);
    }

    // SPA fallback: serve index.html for non-file paths
    if let Some(file) = DesktopAssets::get("index.html") {
        return file_response("index.html", &file);
    }

    (StatusCode::NOT_FOUND, "Not found").into_response()
}

fn file_response(path: &str, file: &rust_embed::EmbeddedFile) -> Response {
    let mime = mime_guess::from_path(path).first_or_octet_stream();

    let mut res = (StatusCode::OK, file.data.to_vec()).into_response();

    res.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref())
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );

    // Cache static assets (hashed filenames) aggressively, html short-lived
    if path.ends_with(".html") || path == "index.html" {
        res.headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    } else {
        res.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("public, max-age=31536000, immutable"),
        );
    }

    res
}
