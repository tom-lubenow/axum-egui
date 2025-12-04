//! Server function support for axum-egui.
//!
//! This crate provides the `#[server]` macro for defining functions that
//! run on the server but can be called from the client.
//!
//! # Example
//!
//! ```ignore
//! use server_fn::prelude::*;
//!
//! #[server]
//! pub async fn greet(name: String) -> Result<String, ServerFnError> {
//!     Ok(format!("Hello, {}!", name))
//! }
//! ```

pub use server_fn_macro::server;

/// Error type for server functions.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ServerFnError {
    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Deserialization error: {0}")]
    Deserialization(String),

    #[error("Request error: {0}")]
    Request(String),

    #[error("Server error: status {0}")]
    ServerError(u16),

    #[error("Custom error: {0}")]
    Custom(String),
}

#[cfg(target_arch = "wasm32")]
impl From<gloo_net::Error> for ServerFnError {
    fn from(e: gloo_net::Error) -> Self {
        ServerFnError::Request(e.to_string())
    }
}

/// Prelude for convenient imports.
pub mod prelude {
    pub use super::ServerFnError;
    pub use server_fn_macro::server;

    // Re-export serde for the generated code
    pub use serde;
    pub use serde_json;

    #[cfg(not(target_arch = "wasm32"))]
    pub use axum;

    #[cfg(not(target_arch = "wasm32"))]
    pub use tracing;

    #[cfg(target_arch = "wasm32")]
    pub use gloo_net;
}
