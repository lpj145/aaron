use axum::{
    body::Body,
    http::{header, Response, StatusCode, Uri},
    response::IntoResponse,
};
use rust_embed::RustEmbed;
use std::fs;
use std::path::PathBuf;

#[derive(RustEmbed)]
#[folder = "ui/dist"]
pub struct Assets;

pub async fn serve_static(uri: Uri, custom_dir: Option<PathBuf>) -> impl IntoResponse {
    let raw_path = uri.path().trim_start_matches('/');

    // 1. If custom static directory was provided, serve from disk
    if let Some(ref dir) = custom_dir {
        let file_path = dir.join(raw_path);
        if file_path.is_file()
            && let Ok(contents) = fs::read(&file_path)
        {
            let mime = mime_guess::from_path(&file_path).first_or_octet_stream();
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(contents))
                .unwrap();
        }
        // SPA Fallback to index.html
        let index_path = dir.join("index.html");
        if let Ok(contents) = fs::read(&index_path) {
            return Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(Body::from(contents))
                .unwrap();
        }
    }

    // 2. Embedded assets
    let target_path = if raw_path.is_empty() {
        "index.html"
    } else {
        raw_path
    };

    if let Some(file) = Assets::get(target_path) {
        let mime = mime_guess::from_path(target_path).first_or_octet_stream();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime.as_ref())
            .body(Body::from(file.data))
            .unwrap();
    }

    // SPA fallback: return index.html for non-file routes
    if !target_path.contains('.')
        && !target_path.starts_with("api/")
        && let Some(index) = Assets::get("index.html")
    {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .body(Body::from(index.data))
            .unwrap();
    }

    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .header(header::CONTENT_TYPE, "text/plain")
        .body(Body::from("404 Not Found"))
        .unwrap()
}
