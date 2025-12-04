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

/// Server function that demonstrates request context access.
#[server]
pub async fn whoami() -> Result<WhoamiResponse, ServerFnError> {
    use server_fn::prelude::request_context;

    let ctx = request_context();

    let user_agent = ctx.header("user-agent").unwrap_or("unknown").to_string();
    let ip = ctx.client_ip().map(|ip| ip.to_string()).unwrap_or_else(|| "unknown".to_string());
    let path = ctx.path().to_string();

    println!("Server: whoami from {} ({})", ip, user_agent);

    Ok(WhoamiResponse {
        user_agent,
        ip,
        path,
    })
}

/// Response type for whoami endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhoamiResponse {
    pub user_agent: String,
    pub ip: String,
    pub path: String,
}

/// Application config - can be provided via use_context.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_name: String,
    pub version: String,
}

/// Server function that demonstrates use_context for dependency injection.
#[server]
pub async fn get_app_info() -> Result<AppInfoResponse, ServerFnError> {
    use server_fn::prelude::{provide_context, use_context};

    // Provide the config (in real app, this would come from middleware/state)
    provide_context(AppConfig {
        app_name: "axum-egui".to_string(),
        version: "0.1.0".to_string(),
    });

    // Now use it
    let config: AppConfig = use_context()?;

    println!("Server: get_app_info - {} v{}", config.app_name, config.version);

    Ok(AppInfoResponse {
        app_name: config.app_name,
        version: config.version,
    })
}

/// Response type for app info endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfoResponse {
    pub app_name: String,
    pub version: String,
}

// ============================================================================
// Typed Error Example
// ============================================================================

/// Custom error type for the divide function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MathError {
    DivisionByZero,
    Overflow { a: i32, b: i32 },
    NegativeResult,
}

/// Server function that demonstrates typed errors.
///
/// This shows how custom error types can be used with `ServerFnError<E>`.
/// The error type is serialized as JSON and can be matched on the client.
#[server]
pub async fn divide(a: i32, b: i32) -> Result<i32, ServerFnError<MathError>> {
    println!("Server: dividing {} / {}", a, b);

    if b == 0 {
        return Err(ServerFnError::app_error(MathError::DivisionByZero));
    }

    // Simulate overflow check
    if a == i32::MIN && b == -1 {
        return Err(ServerFnError::app_error(MathError::Overflow { a, b }));
    }

    let result = a / b;

    if result < 0 {
        return Err(ServerFnError::app_error(MathError::NegativeResult));
    }

    Ok(result)
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
// Server Functions (WebSocket)
// ============================================================================

/// Echo WebSocket - sends back any message received with "Echo: " prefix.
#[server(ws)]
pub async fn echo(incoming: impl Stream<Item = String>) -> impl Stream<Item = String> {
    use futures::StreamExt;
    incoming.map(|msg| {
        println!("WS: received {}", msg);
        format!("Echo: {}", msg)
    })
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
        .route("/api/whoami", post(whoami))
        .route("/api/app_info", post(get_app_info))
        .route("/api/divide", post(divide))
        // SSE routes (generated by #[server(sse)] macro)
        .route("/api/counter", get(counter))
        .route("/api/tick_stream", get(tick_stream))
        // WebSocket routes (generated by #[server(ws)] macro)
        .route("/api/echo", get(echo))
        // Static assets
        .fallback(static_handler);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server running on http://{addr}");
    println!("API endpoints:");
    println!("  POST /api/add         - Add two numbers");
    println!("  POST /api/greet       - Get a greeting");
    println!("  POST /api/whoami      - Get request context info");
    println!("  GET  /api/counter     - SSE counter stream");
    println!("  GET  /api/tick_stream - SSE tick events");
    println!("  WS   /api/echo        - WebSocket echo");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
