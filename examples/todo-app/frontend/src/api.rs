//! Server functions for the todo app example.
//!
//! These functions are defined in the frontend crate (co-located pattern).
//! The `#[server]` macro generates feature-gated code:
//! - On the server (ssr feature): executes directly
//! - On the client (hydrate feature): makes HTTP requests
//!
//! Since the #[server] macro does not yet support axum extractors,
//! authentication is handled via an explicit token parameter.
//! The client stores the token after login and sends it with each request.

use crate::state::Todo;
use axum_egui::{ServerFnError, server};
use serde::{Deserialize, Serialize};

/// Response from a successful login.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub username: String,
}

/// Log in with a username and password.
///
/// For this demo, any non-empty password is accepted.
/// Returns a session token that must be passed to subsequent API calls.
#[server]
pub async fn login(username: String, password: String) -> Result<LoginResponse, ServerFnError> {
    if username.is_empty() {
        return Err(ServerFnError::ServerError(
            "Username cannot be empty".into(),
        ));
    }
    if password.is_empty() {
        return Err(ServerFnError::ServerError(
            "Password cannot be empty".into(),
        ));
    }

    // In a real app, you'd verify credentials against a database.
    // For this demo, any non-empty password is accepted.
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let token = format!("tok_{}_{}", username, timestamp);

    // Store session in the global DB (injected via thread-local or similar)
    // The server's main.rs sets up the DB and the handler accesses it.
    // But since #[server] generates direct function bodies, we use a global.
    crate::api::db::with_db(|db| {
        db.sessions.insert(token.clone(), username.clone());
    });

    Ok(LoginResponse { token, username })
}

/// Get all todos for the authenticated user.
#[server]
pub async fn get_todos(token: String) -> Result<Vec<Todo>, ServerFnError> {
    let username = crate::api::db::authenticate(&token)?;
    Ok(crate::api::db::with_db(|db| {
        db.todos
            .iter()
            .filter(|t| t.owner == username)
            .map(|t| Todo {
                id: t.id,
                title: t.title.clone(),
                completed: t.completed,
            })
            .collect()
    }))
}

/// Add a new todo for the authenticated user.
#[server]
pub async fn add_todo(token: String, title: String) -> Result<Todo, ServerFnError> {
    if title.is_empty() {
        return Err(ServerFnError::ServerError(
            "Todo title cannot be empty".into(),
        ));
    }
    let username = crate::api::db::authenticate(&token)?;
    Ok(crate::api::db::with_db(|db| {
        let id = db.next_id;
        db.next_id += 1;
        let entry = crate::api::db::TodoEntry {
            id,
            owner: username,
            title: title.clone(),
            completed: false,
        };
        db.todos.push(entry);
        Todo {
            id,
            title,
            completed: false,
        }
    }))
}

/// Toggle the completed status of a todo.
#[server]
pub async fn toggle_todo(token: String, id: u64) -> Result<Todo, ServerFnError> {
    let username = crate::api::db::authenticate(&token)?;
    crate::api::db::with_db(|db| {
        let entry = db
            .todos
            .iter_mut()
            .find(|t| t.id == id && t.owner == username)
            .ok_or_else(|| ServerFnError::ServerError("Todo not found".into()))?;
        entry.completed = !entry.completed;
        Ok(Todo {
            id: entry.id,
            title: entry.title.clone(),
            completed: entry.completed,
        })
    })
}

/// Delete a todo.
#[server]
pub async fn delete_todo(token: String, id: u64) -> Result<(), ServerFnError> {
    let username = crate::api::db::authenticate(&token)?;
    crate::api::db::with_db(|db| {
        let len_before = db.todos.len();
        db.todos.retain(|t| !(t.id == id && t.owner == username));
        if db.todos.len() == len_before {
            Err(ServerFnError::ServerError("Todo not found".into()))
        } else {
            Ok(())
        }
    })
}

/// Logout: invalidate the session token.
#[server]
pub async fn logout(token: String) -> Result<(), ServerFnError> {
    crate::api::db::with_db(|db| {
        db.sessions.remove(&token);
    });
    Ok(())
}

// ============================================================================
// Server-side in-memory database (only compiled with ssr feature)
// ============================================================================

#[cfg(feature = "ssr")]
pub mod db {
    use crate::state::Todo;
    use axum_egui::ServerFnError;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    /// Internal todo entry with owner information.
    #[derive(Debug, Clone)]
    pub struct TodoEntry {
        pub id: u64,
        pub owner: String,
        pub title: String,
        pub completed: bool,
    }

    /// The in-memory database.
    #[derive(Debug)]
    pub struct AppDb {
        pub todos: Vec<TodoEntry>,
        pub next_id: u64,
        pub sessions: HashMap<String, String>, // token -> username
    }

    impl Default for AppDb {
        fn default() -> Self {
            Self {
                todos: Vec::new(),
                next_id: 1,
                sessions: HashMap::new(),
            }
        }
    }

    /// Global database instance.
    ///
    /// This is used because #[server] macro functions don't have access to
    /// axum state extractors. In a real application, you'd use axum's State
    /// extractor or a similar mechanism.
    static DB: OnceLock<Mutex<AppDb>> = OnceLock::new();

    /// Initialize the global database. Call this once at server startup.
    pub fn init_db() {
        DB.get_or_init(|| Mutex::new(AppDb::default()));
    }

    /// Access the database with a closure.
    pub fn with_db<F, R>(f: F) -> R
    where
        F: FnOnce(&mut AppDb) -> R,
    {
        let db = DB.get_or_init(|| Mutex::new(AppDb::default()));
        let mut lock = db.lock().expect("DB lock poisoned");
        f(&mut lock)
    }

    /// Validate a session token and return the associated username.
    pub fn authenticate(token: &str) -> Result<String, ServerFnError> {
        with_db(|db| {
            db.sessions
                .get(token)
                .cloned()
                .ok_or_else(|| ServerFnError::ServerError("Invalid or expired session".into()))
        })
    }

    /// Get the current todos for a user (used by the server to build initial state).
    pub fn get_user_todos(username: &str) -> Vec<Todo> {
        with_db(|db| {
            db.todos
                .iter()
                .filter(|t| t.owner == username)
                .map(|t| Todo {
                    id: t.id,
                    title: t.title.clone(),
                    completed: t.completed,
                })
                .collect()
        })
    }
}

// Stub module when not on the server (so crate::api::db paths compile)
#[cfg(not(feature = "ssr"))]
pub mod db {}
