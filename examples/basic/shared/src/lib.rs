//! Shared types and server functions for the basic example.
//!
//! This crate compiles for both server and web targets.
//! Server functions defined here work on both sides:
//! - Server: executes the function body
//! - Web: makes HTTP calls to the server

pub mod api;
pub mod server_fns;

// Re-export for convenience
pub use api::*;

use serde::{Deserialize, Serialize};

/// The example app state.
///
/// This is serialized by the server and sent to the client as initial state.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub label: String,
    pub value: f32,
    pub server_message: Option<String>,
}
