//! Shared state types for the basic example.

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
