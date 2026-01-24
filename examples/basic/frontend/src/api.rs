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
