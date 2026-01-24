//! Simple RPC helpers for client-server communication.
//!
//! This module provides utilities for making HTTP API calls from WASM clients
//! and handling JSON requests on the server.
//!
//! # Example
//!
//! Define an API function that works on both server and client:
//!
//! ```ignore
//! use axum_egui::rpc::ServerFnError;
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Serialize, Deserialize)]
//! struct AddArgs { a: i32, b: i32 }
//!
//! pub async fn add(a: i32, b: i32) -> Result<i32, ServerFnError> {
//!     #[cfg(feature = "ssr")]
//!     {
//!         Ok(a + b)
//!     }
//!     #[cfg(feature = "hydrate")]
//!     {
//!         axum_egui::rpc::call("/api/add", &AddArgs { a, b }).await
//!     }
//! }
//! ```

use serde::{Deserialize, Serialize};

#[cfg(feature = "client")]
use serde::de::DeserializeOwned;

/// Error type for server function calls.
#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
pub enum ServerFnError {
    /// Failed to serialize request data.
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Failed to deserialize response data.
    #[error("Deserialization error: {0}")]
    Deserialization(String),

    /// HTTP request failed.
    #[error("Request error: {0}")]
    Request(String),

    /// Server returned an error response.
    #[error("Server error: {0}")]
    ServerError(String),
}

/// Client-side function to call a server API endpoint.
///
/// This makes a POST request to the given path with JSON-serialized arguments,
/// and deserializes the JSON response.
#[cfg(feature = "client")]
pub async fn call<Args, Resp>(path: &str, args: &Args) -> Result<Resp, ServerFnError>
where
    Args: Serialize,
    Resp: DeserializeOwned,
{
    use gloo_net::http::Request;

    let response = Request::post(path)
        .header("Content-Type", "application/json")
        .json(args)
        .map_err(|e| ServerFnError::Serialization(e.to_string()))?
        .send()
        .await
        .map_err(|e| ServerFnError::Request(e.to_string()))?;

    if !response.ok() {
        let status = response.status();
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        return Err(ServerFnError::ServerError(format!(
            "HTTP {}: {}",
            status, text
        )));
    }

    response
        .json()
        .await
        .map_err(|e| ServerFnError::Deserialization(e.to_string()))
}

/// Server-side helper to extract JSON and call a handler.
///
/// This is a convenience wrapper for axum handlers that take JSON input.
#[cfg(feature = "server")]
pub mod server {
    use super::ServerFnError;
    use axum::{Json, http::StatusCode, response::IntoResponse};
    use serde::{Deserialize, Serialize};

    /// Response wrapper that serializes errors as JSON.
    pub struct ApiResponse<T>(pub Result<T, ServerFnError>);

    impl<T: Serialize> IntoResponse for ApiResponse<T> {
        fn into_response(self) -> axum::response::Response {
            match self.0 {
                Ok(value) => Json(value).into_response(),
                Err(e) => {
                    let body = serde_json::json!({
                        "error": e.to_string()
                    });
                    (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
                }
            }
        }
    }

    /// Helper trait for converting function results to API responses.
    pub trait IntoApiResponse<T> {
        fn into_api_response(self) -> ApiResponse<T>;
    }

    impl<T> IntoApiResponse<T> for Result<T, ServerFnError> {
        fn into_api_response(self) -> ApiResponse<T> {
            ApiResponse(self)
        }
    }

    /// Create an axum handler from a function that takes deserialized JSON args.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use axum_egui::rpc::{ServerFnError, server::json_handler};
    /// use serde::{Deserialize, Serialize};
    ///
    /// #[derive(Deserialize)]
    /// struct AddArgs { a: i32, b: i32 }
    ///
    /// async fn add_impl(args: AddArgs) -> Result<i32, ServerFnError> {
    ///     Ok(args.a + args.b)
    /// }
    ///
    /// // In router:
    /// // .route("/api/add", post(json_handler(add_impl)))
    /// ```
    pub fn json_handler<Args, Resp, F, Fut>(
        f: F,
    ) -> impl Fn(
        Json<Args>,
    )
        -> std::pin::Pin<Box<dyn std::future::Future<Output = ApiResponse<Resp>> + Send>>
    + Clone
    + Send
    where
        Args: for<'de> Deserialize<'de> + Send + 'static,
        Resp: Serialize + Send + 'static,
        F: Fn(Args) -> Fut + Clone + Send + 'static,
        Fut: std::future::Future<Output = Result<Resp, ServerFnError>> + Send + 'static,
    {
        move |Json(args): Json<Args>| {
            let f = f.clone();
            Box::pin(async move { ApiResponse(f(args).await) })
        }
    }
}

#[cfg(feature = "server")]
pub use server::{ApiResponse, IntoApiResponse, json_handler};
