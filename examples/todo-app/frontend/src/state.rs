//! Shared state types for the todo app example.

use serde::{Deserialize, Serialize};

/// A single todo item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: u64,
    pub title: String,
    pub completed: bool,
}

/// The todo app state sent from server to client on initial load.
///
/// This is serialized by the server and injected into the DOM
/// via the `#axum-egui-state` script tag.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    /// The logged-in username, if any.
    pub username: Option<String>,
    /// The user's current todos.
    pub todos: Vec<Todo>,
}
