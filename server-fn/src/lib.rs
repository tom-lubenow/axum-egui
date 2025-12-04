//! Server function support for axum-egui.
//!
//! This crate provides macros and runtime support for defining functions that
//! run on the server but can be called from the client.
//!
//! # RPC Functions
//!
//! Simple request/response pattern:
//!
//! ```ignore
//! use server_fn::prelude::*;
//!
//! #[server]
//! pub async fn greet(name: String) -> Result<String, ServerFnError> {
//!     Ok(format!("Hello, {}!", name))
//! }
//! ```
//!
//! # Request Context
//!
//! Access HTTP request details (headers, cookies, IP) within server functions:
//!
//! ```ignore
//! use server_fn::prelude::*;
//!
//! #[server]
//! pub async fn whoami() -> Result<String, ServerFnError> {
//!     let ctx = request_context();
//!     let user_agent = ctx.header("user-agent").unwrap_or("unknown");
//!     Ok(format!("Your browser: {}", user_agent))
//! }
//! ```
//!
//! # Server-Sent Events (SSE)
//!
//! For streaming data from server to client:
//!
//! ```ignore
//! use server_fn::prelude::*;
//!
//! // Server: returns a stream
//! #[server(sse)]
//! pub async fn counter() -> impl Stream<Item = i32> {
//!     async_stream::stream! {
//!         for i in 0..10 {
//!             tokio::time::sleep(Duration::from_secs(1)).await;
//!             yield i;
//!         }
//!     }
//! }
//!
//! // Client: receives events via SseStream
//! let stream = counter();
//! for event in stream.try_iter() {
//!     // handle event
//! }
//! ```

pub mod context;
pub mod sse;
pub mod ws;

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

    // Request context
    pub use super::context::{request_context, try_request_context, RequestContext};

    // Custom context (extractors)
    pub use super::context::{use_context, try_use_context};

    #[cfg(not(target_arch = "wasm32"))]
    pub use super::context::{provide_context, with_context, with_full_context};

    // SSE types
    pub use super::sse::ReconnectConfig;

    #[cfg(target_arch = "wasm32")]
    pub use super::sse::{ConnectionState, SseStream};

    #[cfg(not(target_arch = "wasm32"))]
    pub use super::sse::into_sse_response;

    // WebSocket types
    #[cfg(target_arch = "wasm32")]
    pub use super::ws::{WsConnectionState, WsStream};

    #[cfg(not(target_arch = "wasm32"))]
    pub use super::ws::{ws_upgrade, handle_socket, WsHandler};

    #[cfg(not(target_arch = "wasm32"))]
    pub use axum;

    #[cfg(not(target_arch = "wasm32"))]
    pub use tracing;

    #[cfg(not(target_arch = "wasm32"))]
    pub use async_stream;

    #[cfg(not(target_arch = "wasm32"))]
    pub use futures;

    #[cfg(not(target_arch = "wasm32"))]
    pub use futures::Stream;

    #[cfg(target_arch = "wasm32")]
    pub use gloo_net;
}
