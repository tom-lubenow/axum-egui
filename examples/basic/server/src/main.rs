//! Basic example server.
//!
//! This server:
//! - Serves the embedded frontend WASM app
//! - Provides API endpoints for the frontend
//! - Demonstrates simple RPC patterns

use axum::routing::{get, post};
use axum::{Json, Router};
use basic_shared::{api, AppState};
use rust_embed::RustEmbed;
use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(index))
        // API endpoints
        .route("/api/add", post(add_handler))
        .route("/api/greet", post(greet_handler))
        .route("/api/whoami", get(whoami_handler))
        // Serve static assets
        .fallback(axum_egui::static_handler::<Assets>);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Server running on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
