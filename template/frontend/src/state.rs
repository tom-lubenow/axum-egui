//! Shared state types.

use serde::{Deserialize, Serialize};

/// The app state.
///
/// This is serialized by the server and sent to the client as initial state.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AppState {
    pub counter: i32,
    pub message: String,
}
