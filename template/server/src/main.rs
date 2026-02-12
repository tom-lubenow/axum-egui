//! Server for {{project-name}}.
//!
//! This server:
//! - Serves the embedded frontend WASM app
//! - Provides API endpoints for server functions

use axum::Router;
use axum::routing::{get, post};
use {{crate_name}}_frontend::AppState;
use {{crate_name}}_frontend::api::{greet_handler, increment_handler};
use rust_embed::RustEmbed;
use std::net::SocketAddr;

// Embed the frontend assets built by build.rs
// The env var is set by axum_egui_build::frontend() in build.rs
#[derive(RustEmbed)]
#[folder = "${{ crate_name | upcase }}_FRONTEND_DIST"]
struct Assets;

/// Handler that serves the app with initial state.
async fn index() -> axum_egui::App<AppState, Assets> {
    axum_egui::App::new(AppState {
        counter: 0,
        message: "Hello from the server!".into(),
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(index))
        .route("/api/increment", post(increment_handler))
        .route("/api/greet", post(greet_handler))
        .fallback(axum_egui::static_handler::<Assets>);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Server running on http://{addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
