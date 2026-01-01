//! Multi-frontend server demonstrating serving multiple egui apps.
//!
//! - User frontend at `/`
//! - Admin frontend at `/admin`

use axum::Router;
use axum::extract::Request;
use axum::http::Uri;
use axum::response::IntoResponse;
use axum::routing::get;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

// ============================================================================
// App State Types (must match frontend types for JSON serialization)
// ============================================================================

/// User app state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserApp {
    pub counter: i32,
    pub username: Option<String>,
}

/// Admin app state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdminApp {
    pub total_users: i32,
    pub active_sessions: i32,
    pub server_uptime_secs: u64,
}

// ============================================================================
// User Frontend Assets (from artifact dependency)
// ============================================================================

#[derive(RustEmbed)]
#[folder = "$USER_FRONTEND_DIST"]
struct UserAssets;

async fn user_app() -> axum_egui::App<UserApp, UserAssets> {
    axum_egui::App::new(UserApp {
        counter: 0,
        username: None,
    })
}

async fn user_static(uri: Uri) -> impl IntoResponse {
    axum_egui::static_handler::<UserAssets>(uri).await
}

// ============================================================================
// Admin Frontend Assets (from artifact dependency)
// ============================================================================

#[derive(RustEmbed)]
#[folder = "$ADMIN_FRONTEND_DIST"]
struct AdminAssets;

async fn admin_app() -> axum_egui::App<AdminApp, AdminAssets> {
    axum_egui::App::new(AdminApp {
        total_users: 42,
        active_sessions: 7,
        server_uptime_secs: 3600,
    })
}

async fn admin_static(request: Request) -> impl IntoResponse {
    // Strip /admin prefix before looking up the file
    let path = request.uri().path();
    let stripped = path.strip_prefix("/admin").unwrap_or(path);
    let new_uri: Uri = stripped.parse().unwrap_or_else(|_| "/".parse().unwrap());
    axum_egui::static_handler::<AdminAssets>(new_uri).await
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
        // User frontend at root (must be last due to fallback)
        .merge(user_routes);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Server running on http://{addr}");
    println!();
    println!("Frontends:");
    println!("  User app:  http://{addr}/");
    println!("  Admin app: http://{addr}/admin");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
