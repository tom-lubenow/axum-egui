//! Server functions for the basic example.
//!
//! These functions are defined here in the frontend crate (co-located pattern).
//! The `#[server]` macro generates feature-gated code:
//! - On the server (ssr feature): executes directly
//! - On the client (hydrate feature): makes HTTP requests

use axum_egui::{ServerFnError, server};
use serde::{Deserialize, Serialize};

/// Server info response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub message: String,
    pub timestamp: u64,
}

/// Add two numbers together.
#[server]
pub async fn add(a: i32, b: i32) -> Result<i32, ServerFnError> {
    Ok(a + b)
}

/// Greet someone by name.
#[server]
pub async fn greet(name: String) -> Result<String, ServerFnError> {
    Ok(format!("Hello, {}!", name))
}

/// Get information about the server.
#[server]
pub async fn whoami() -> Result<ServerInfo, ServerFnError> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    Ok(ServerInfo {
        message: "I am axum-egui server".into(),
        timestamp,
    })
}

/// Demonstrates axum extractor access in server functions.
///
/// The `#[extract]` attribute marks parameters that are injected by axum on the
/// server side. These parameters are omitted from the client-side function
/// signature and the serialized args struct.
///
/// Here `app_name` is an axum `State<String>` extractor -- on the server, axum
/// injects the shared state. On the client, the function only takes `user_name`.
#[cfg(feature = "ssr")]
use axum::extract::State;

#[server]
pub async fn greet_with_app(
    #[extract] State(app_name): State<String>,
    user_name: String,
) -> Result<String, ServerFnError> {
    Ok(format!("Hello {} from {}!", user_name, app_name))
}
