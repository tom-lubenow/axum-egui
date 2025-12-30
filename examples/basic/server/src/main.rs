//! Basic example server.
//!
//! This server:
//! - Serves the embedded frontend WASM app
//! - Auto-registers all server functions from basic-shared
//! - Demonstrates RPC, SSE, and WebSocket patterns

use axum::Router;
use axum::routing::get;
use axum_egui::prelude::*;
use basic_shared::AppState;
use rust_embed::RustEmbed;
use std::net::SocketAddr;

// Import shared module to ensure server functions are linked and registered
use basic_shared::api;

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

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // Touch api module to ensure server functions are linked
    let _ = &api::add;

    let app = Router::new()
        .route("/", get(index))
        // Auto-register all server functions
        .register_server_fns()
        // Serve static assets
        .fallback(axum_egui::static_handler::<Assets>);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server running on http://{addr}");
    println!("Server functions auto-registered via inventory.");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
