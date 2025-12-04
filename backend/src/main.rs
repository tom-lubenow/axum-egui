//! Backend server for axum-egui example.
//!
//! Demonstrates the `App<T>` response wrapper for serving egui apps,
//! and the `#[server]` macro for server functions.

use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use server_fn::prelude::*;
use std::net::SocketAddr;
use std::time::Duration;

// ============================================================================
// Server Functions (RPC)
// ============================================================================

#[server]
pub async fn add(a: i32, b: i32) -> Result<i32, ServerFnError> {
    println!("Server: adding {} + {}", a, b);
    Ok(a + b)
}

#[server]
pub async fn greet(name: String) -> Result<String, ServerFnError> {
    println!("Server: greeting {}", name);
    Ok(format!("Hello, {}! Greetings from the server.", name))
}

// ============================================================================
// Server Functions (SSE)
// ============================================================================

/// A counter that streams every second.
#[server(sse)]
pub async fn counter() -> impl Stream<Item = i32> {
    async_stream::stream! {
        for i in 0..100 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            println!("SSE: emitting {}", i);
            yield i;
        }
    }
}

/// Tick event with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickEvent {
    pub count: i32,
    pub timestamp: u64,
}

/// A more complex stream that emits structured events.
#[server(sse)]
pub async fn tick_stream() -> impl Stream<Item = TickEvent> {
    async_stream::stream! {
        let start = std::time::Instant::now();
        for count in 0..100 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            yield TickEvent {
                count,
                timestamp: start.elapsed().as_millis() as u64,
            };
        }
    }
}

// ============================================================================
// Frontend Serving
// ============================================================================

/// Embedded frontend assets (built by build.rs).
#[derive(RustEmbed)]
#[folder = "../frontend/dist/"]
struct Assets;

/// The example app state - must match the frontend's ExampleApp.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExampleApp {
    pub label: String,
    pub value: f32,
    pub server_message: Option<String>,
}

/// Axum response wrapper for serving egui apps with initial state.
pub struct App<T>(pub T);

impl<T: Serialize> IntoResponse for App<T> {
    fn into_response(self) -> Response {
        let state_json = match serde_json::to_string(&self.0) {
            Ok(json) => json,
            Err(e) => {
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(format!("Failed to serialize app state: {e}")))
                    .unwrap();
            }
        };

        let html = match Assets::get("index.html") {
            Some(content) => {
                let html_str = String::from_utf8_lossy(&content.data);
                let state_script = format!(
                    r#"<script id="axum-egui-state" type="application/json">{}</script>"#,
                    state_json.replace("</", "<\\/")
                );
                html_str.replace("<!--AXUM_EGUI_INITIAL_STATE-->", &state_script)
            }
            None => {
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from("Frontend assets not found."))
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
        server_message: None,
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
        None => match Assets::get("index.html") {
            Some(content) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "text/html")
                .body(Body::from(content.data.to_vec()))
                .unwrap(),
            None => Response::builder()
                .status(StatusCode::NOT_FOUND)
                .body(Body::from("404 Not Found"))
                .unwrap(),
        },
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(my_app))
        // RPC routes (generated by #[server] macro)
        .route("/api/add", post(add))
        .route("/api/greet", post(greet))
        // SSE routes (generated by #[server(sse)] macro)
        .route("/api/counter", get(counter))
        .route("/api/tick_stream", get(tick_stream))
        // Static assets
        .fallback(static_handler);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server running on http://{addr}");
    println!("API endpoints:");
    println!("  POST /api/add         - Add two numbers");
    println!("  POST /api/greet       - Get a greeting");
    println!("  GET  /api/counter     - SSE counter stream");
    println!("  GET  /api/tick_stream - SSE tick events");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
