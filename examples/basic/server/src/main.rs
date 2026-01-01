//! Basic example server.
//!
//! This server:
//! - Serves the embedded frontend WASM app
//! - Provides API endpoints for the frontend
//! - Demonstrates simple RPC patterns
//! - Provides SSE streaming for real-time updates

use axum::routing::{get, post};
use axum::{Json, Router};
use axum_egui::sse::{Event, KeepAlive, Sse};
use basic_shared::{AppState, api};
use futures_util::stream::{self, Stream};
use rust_embed::RustEmbed;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Embed the frontend assets built by build.rs
// Convention: {CRATE_NAME}_DIST
#[derive(RustEmbed)]
#[folder = "$BASIC_FRONTEND_DIST"]
struct Assets;

/// Handler that serves the app with initial state.
async fn index() -> axum_egui::App<AppState, Assets> {
    axum_egui::App::new(AppState {
        label: "Hello from the server!".into(),
        value: 42.0,
        server_message: None,
    })
}

// ============================================================================
// API Handlers
// ============================================================================

async fn add_handler(Json(req): Json<api::AddRequest>) -> Json<api::AddResponse> {
    Json(api::AddResponse {
        result: req.a + req.b,
    })
}

async fn greet_handler(Json(req): Json<api::GreetRequest>) -> Json<api::GreetResponse> {
    Json(api::GreetResponse {
        message: format!("Hello, {}!", req.name),
    })
}

async fn whoami_handler() -> Json<api::WhoamiResponse> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    Json(api::WhoamiResponse {
        message: "I am axum-egui server".into(),
        timestamp,
    })
}

// ============================================================================
// SSE Handlers
// ============================================================================

/// SSE endpoint that streams a counter every second.
async fn counter_sse() -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let stream = stream::unfold(0, |count| async move {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let event = Event::new()
            .json_data(count)
            .unwrap_or_else(|_| Event::new().data("error"))
            .into();
        Some((Ok(event), count + 1))
    });

    Sse::new(stream).keep_alive(KeepAlive::default())
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(index))
        // API endpoints
        .route("/api/add", post(add_handler))
        .route("/api/greet", post(greet_handler))
        .route("/api/whoami", get(whoami_handler))
        // SSE endpoint for real-time updates
        .route("/api/counter", get(counter_sse))
        // Serve static assets
        .fallback(axum_egui::static_handler::<Assets>);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Server running on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
