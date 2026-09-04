use axum::{
    body::Body,
    extract::State,
    http::{header, Request, Response, StatusCode},
};
use std::sync::Arc;
use tracing::error;

use crate::cluster::DemoClusterManager;

pub async fn handle_proxy_admin(
    State(manager): State<Arc<DemoClusterManager>>,
    req: Request<Body>,
) -> Response<Body> {
    let uri = req.uri().clone();
    let path = uri.path();

    // 1. Extract session_id from cookie, header, or URL path
    let mut session_id = None;

    // Check URL path: /demo/:session_id/dashboard
    if path.starts_with("/demo/") {
        let segments: Vec<&str> = path.trim_start_matches("/demo/").split('/').collect();
        if !segments.is_empty() && !segments[0].is_empty() {
            session_id = Some(segments[0].to_string());
        }
    }

    // Check header
    if session_id.is_none() {
        if let Some(val) = req.headers().get("x-aaron-session") {
            if let Ok(s) = val.to_str() {
                session_id = Some(s.to_string());
            }
        }
    }

    // Check Cookie: aaron_demo_session=...
    if session_id.is_none() {
        if let Some(cookie_hdr) = req.headers().get(header::COOKIE) {
            if let Ok(cookie_str) = cookie_hdr.to_str() {
                for cookie in cookie_str.split(';') {
                    let parts: Vec<&str> = cookie.trim().split('=').collect();
                    if parts.len() == 2 && parts[0] == "aaron_demo_session" {
                        session_id = Some(parts[1].to_string());
                        break;
                    }
                }
            }
        }
    }

    // If still no session found, and this is /assets/*, try to proxy to any active cluster
    let cluster = match session_id {
        Some(ref sid) => manager.get_cluster(sid).await,
        None => {
            if path.starts_with("/assets/") || path == "/favicon.svg" {
                // Borrow any active cluster for static assets
                manager.get_any_cluster().await
            } else {
                None
            }
        }
    };

    let cluster = match cluster {
        Some(c) => c,
        None => {
            return Response::builder()
                .status(StatusCode::NOT_FOUND)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "error": "No active Aaron demo session found. Please launch a cluster on the landing page."
                    })
                    .to_string(),
                ))
                .unwrap();
        }
    };

    // Calculate target path on internal admin server
    let target_subpath = if path.starts_with("/demo/") {
        let parts: Vec<&str> = path.trim_start_matches("/demo/").splitn(2, '/').collect();
        if parts.len() == 2 {
            format!("/{}", parts[1])
        } else {
            "/".to_string()
        }
    } else {
        path.to_string()
    };

    let query_str = uri.query().map(|q| format!("?{q}")).unwrap_or_default();
    let target_url = format!("http://127.0.0.1:{}{}{}", cluster.admin_port, target_subpath, query_str);

    // Forward request
    let client = reqwest::Client::new();
    let method = req.method().clone();
    let mut builder = client.request(method, &target_url);

    // Copy safe headers
    for (name, val) in req.headers() {
        if name != header::HOST && name != header::CONNECTION && name != "transfer-encoding" {
            builder = builder.header(name, val);
        }
    }

    let body_bytes = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(e) => {
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::from(format!("Failed to read request body: {e}")))
                .unwrap();
        }
    };

    if !body_bytes.is_empty() {
        builder = builder.body(body_bytes);
    }

    match builder.send().await {
        Ok(resp) => {
            let status = resp.status();
            let mut response_builder = Response::builder().status(status.as_u16());

            // If this was a navigation to /demo/:session_id, set the session cookie
            if path.starts_with("/demo/") {
                response_builder = response_builder.header(
                    header::SET_COOKIE,
                    format!("aaron_demo_session={}; Path=/; SameSite=Lax", cluster.session_id),
                );
            }

            for (name, val) in resp.headers() {
                if name != header::CONNECTION && name != "transfer-encoding" {
                    response_builder = response_builder.header(name, val);
                }
            }

            let stream = resp.bytes_stream();
            response_builder.body(Body::from_stream(stream)).unwrap_or_else(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Failed to build response"))
                    .unwrap()
            })
        }
        Err(err) => {
            error!(url = %target_url, "Failed to proxy request to internal cluster admin: {err}");
            Response::builder()
                .status(StatusCode::BAD_GATEWAY)
                .body(Body::from(format!("Bad Gateway: {err}")))
                .unwrap()
        }
    }
}
