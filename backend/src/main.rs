//! Backend server for axum-egui example.
//!
//! Demonstrates the `App<T>` response wrapper for serving egui apps.

use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Embedded frontend assets (built by build.rs).
#[derive(RustEmbed)]
#[folder = "../frontend/dist/"]
struct Assets;

/// The example app state - must match the frontend's ExampleApp.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExampleApp {
    pub label: String,
    pub value: f32,
}

/// Axum response wrapper for serving egui apps with initial state.
///
/// Usage:
/// ```rust
/// async fn handler() -> App<MyApp> {
///     App(MyApp { label: "Hello!".into(), value: 42.0 })
/// }
/// ```
pub struct App<T>(pub T);

impl<T: Serialize> IntoResponse for App<T> {
    fn into_response(self) -> Response {
        // Serialize the app state to JSON
        let state_json = match serde_json::to_string(&self.0) {
            Ok(json) => json,
            Err(e) => {
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(format!("Failed to serialize app state: {e}")))
                    .unwrap();
            }
        };

        // Get the index.html template
        let html = match Assets::get("index.html") {
            Some(content) => {
                let html_str = String::from_utf8_lossy(&content.data);
                // Inject the initial state as a script tag
                let state_script = format!(
                    r#"<script id="axum-egui-state" type="application/json">{}</script>"#,
                    state_json.replace("</", "<\\/") // Escape for HTML
                );
                html_str.replace("<!--AXUM_EGUI_INITIAL_STATE-->", &state_script)
            }
            None => {
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(
                        "Frontend assets not found. Did the build complete?",
                    ))
                    .unwrap();
            }
        };

        Html(html).into_response()
    }
}

/// Handler that returns an egui app with server-provided initial state.
async fn my_app() -> App<ExampleApp> {
    App(ExampleApp {
        label: "Hello from the server!".into(),
        value: 42.0,
    })
}

/// Handler for static assets (JS, WASM, etc).
async fn static_handler(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();

            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref())
                .body(Body::from(content.data.to_vec()))
                .unwrap()
        }
        None => {
            // For SPA routing, return index.html for unknown paths
            match Assets::get("index.html") {
                Some(content) => Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(Body::from(content.data.to_vec()))
                    .unwrap(),
                None => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("404 Not Found"))
                    .unwrap(),
            }
        }
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(my_app))
        .fallback(static_handler);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server running on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
