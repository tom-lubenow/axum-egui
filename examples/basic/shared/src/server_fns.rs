//! Server functions using the #[server] macro.
//!
//! These functions work on both server and client:
//! - On the server, they execute directly
//! - On the client (WASM), they make HTTP requests

#[allow(unused_imports)]
use axum_egui::ServerFnError;
use axum_egui::server;
use serde::{Deserialize, Serialize};

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

/// Server info response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub message: String,
    pub timestamp: u64,
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
