//! Server functions.
//!
//! These functions are defined in the frontend crate (co-located pattern).
//! The `#[server]` macro generates feature-gated code:
//! - On the server (ssr feature): executes directly
//! - On the client (hydrate feature): makes HTTP requests

use axum_egui::{ServerFnError, server};

/// Increment a counter value on the server.
#[server]
pub async fn increment(value: i32) -> Result<i32, ServerFnError> {
    Ok(value + 1)
}

/// Greet someone by name.
#[server]
pub async fn greet(name: String) -> Result<String, ServerFnError> {
    Ok(format!("Hello, {}!", name))
}
