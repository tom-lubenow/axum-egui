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
///
/// This enum provides typed error handling that works across the server/client boundary.
/// Custom application errors can be serialized and sent to the client.
///
/// # Example: Custom Error Types
///
/// ```ignore
/// use server_fn::prelude::*;
///
/// #[derive(Debug, Clone, Serialize, Deserialize)]
/// pub enum MyError {
///     NotFound { id: i32 },
///     Unauthorized,
///     ValidationFailed { field: String, message: String },
/// }
///
/// #[server]
/// pub async fn get_user(id: i32) -> Result<User, ServerFnError<MyError>> {
///     if id < 0 {
///         return Err(ServerFnError::app_error(MyError::ValidationFailed {
///             field: "id".into(),
///             message: "ID must be positive".into(),
///         }));
///     }
///     // ...
/// }
///
/// // On the client:
/// match get_user(-1).await {
///     Err(ServerFnError::AppError(MyError::ValidationFailed { field, message })) => {
///         // Handle validation error
///     }
///     // ...
/// }
/// ```
#[derive(Debug, Clone, thiserror::Error, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum ServerFnError<E = ()> {
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

    #[error("Application error")]
    AppError(E),
}

impl<E> ServerFnError<E> {
    /// Create an application-level error.
    ///
    /// This wraps a custom error type that will be serialized and sent to the client.
    pub fn app_error(error: E) -> Self {
        ServerFnError::AppError(error)
    }
}

impl ServerFnError<()> {
    /// Convert a unit-typed error to any other error type.
    ///
    /// This is useful for error propagation with `?`.
    pub fn into_any<E>(self) -> ServerFnError<E> {
        match self {
            ServerFnError::Serialization(s) => ServerFnError::Serialization(s),
            ServerFnError::Deserialization(s) => ServerFnError::Deserialization(s),
            ServerFnError::Request(s) => ServerFnError::Request(s),
            ServerFnError::ServerError(s) => ServerFnError::ServerError(s),
            ServerFnError::Custom(s) => ServerFnError::Custom(s),
            ServerFnError::AppError(()) => ServerFnError::Custom("unknown error".to_string()),
        }
    }
}

impl<E> From<String> for ServerFnError<E> {
    fn from(s: String) -> Self {
        ServerFnError::Custom(s)
    }
}

impl<E> From<&str> for ServerFnError<E> {
    fn from(s: &str) -> Self {
        ServerFnError::Custom(s.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
impl<E> From<gloo_net::Error> for ServerFnError<E> {
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

    // MessagePack serialization (when enabled)
    #[cfg(feature = "msgpack")]
    pub use rmp_serde;

    // Request context
    pub use super::context::{RequestContext, request_context, try_request_context};

    // Custom context (extractors)
    pub use super::context::{try_use_context, use_context};

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
    pub use super::ws::{WsHandler, handle_socket, ws_upgrade};

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
