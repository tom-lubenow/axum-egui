//! Multi-frontend example demonstrating serving multiple egui apps from one backend.
//!
//! This example shows how to serve two separate frontends:
//! - User frontend at `/` - A simple counter app
//! - Admin frontend at `/admin` - An admin dashboard
//!
//! Each frontend has its own:
//! - RustEmbed type pointing to its dist/ folder
//! - App state type
//! - Routes

use axum::{Router, extract::Request, http::Uri, response::IntoResponse, routing::get};
use axum_egui::prelude::*;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

// ============================================================================
// User Frontend (served at /)
// ============================================================================

/// Embedded assets for the user frontend.
#[derive(RustEmbed)]
#[folder = "user-frontend/dist/"]
struct UserAssets;

/// State for the user app - a simple counter.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserApp {
    pub counter: i32,
    pub username: Option<String>,
}

/// Handler that returns the user app with initial state.
async fn user_app() -> axum_egui::App<UserApp, UserAssets> {
    axum_egui::App::new(UserApp {
        counter: 0,
        username: None,
    })
}

/// Static file handler for user frontend assets.
async fn user_static(uri: Uri) -> impl IntoResponse {
    axum_egui::static_handler::<UserAssets>(uri).await
}

// ============================================================================
// Admin Frontend (served at /admin)
// ============================================================================

/// Embedded assets for the admin frontend.
#[derive(RustEmbed)]
#[folder = "admin-frontend/dist/"]
struct AdminAssets;

/// State for the admin app - dashboard with stats.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdminApp {
    pub total_users: i32,
    pub active_sessions: i32,
    pub server_uptime_secs: u64,
}

/// Handler that returns the admin app with initial state.
async fn admin_app() -> axum_egui::App<AdminApp, AdminAssets> {
    axum_egui::App::new(AdminApp {
        total_users: 42,
        active_sessions: 7,
        server_uptime_secs: 3600,
    })
}

/// Static file handler for admin frontend assets.
/// Strips the /admin prefix before looking up the file.
async fn admin_static(request: Request) -> impl IntoResponse {
    // Strip /admin prefix from the path
    let path = request.uri().path();
    let stripped = path.strip_prefix("/admin").unwrap_or(path);
    let new_uri: Uri = stripped.parse().unwrap_or_else(|_| "/".parse().unwrap());

    axum_egui::static_handler::<AdminAssets>(new_uri).await
}

// ============================================================================
// Shared API (both frontends can use these)
// ============================================================================

#[server]
pub async fn get_server_time() -> Result<u64, ServerFnError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    Ok(now)
}

#[server]
pub async fn increment_counter(current: i32) -> Result<i32, ServerFnError> {
    Ok(current + 1)
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // User frontend routes
    let user_routes = Router::new()
        .route("/", get(user_app))
        .fallback(get(user_static));

    // Admin frontend routes (nested under /admin)
    let admin_routes = Router::new()
        .route("/", get(admin_app))
        .fallback(get(admin_static));

    // Combine everything
    let app = Router::new()
        // Mount admin frontend under /admin
        .nest("/admin", admin_routes)
        // API routes (shared by both frontends)
        .route("/api/server_time", get(get_server_time))
        .route("/api/increment", axum::routing::post(increment_counter))
        // User frontend at root (must be last due to fallback)
        .merge(user_routes);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server running on http://{addr}");
    println!();
    println!("Frontends:");
    println!("  User app:  http://{addr}/");
    println!("  Admin app: http://{addr}/admin");
    println!();
    println!("Shared API:");
    println!("  GET  /api/server_time   - Get current server time");
    println!("  POST /api/increment     - Increment a counter");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
