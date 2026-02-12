//! Simple RPC helpers for client-server communication.
//!
//! This module provides utilities for making HTTP API calls from WASM clients
//! and handling JSON requests on the server.
//!
//! # Error Boundaries
//!
//! [`ServerFnError<E>`] supports typed application errors that work across
//! the client/server boundary. The default type parameter is [`DefaultError`],
//! so existing code using `ServerFnError` (without a type parameter) continues
//! to work unchanged.
//!
//! ## Custom Errors
//!
//! ```rust
//! use axum_egui::rpc::{ServerFnError, ErrorStatusCode};
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Clone, Serialize, Deserialize)]
//! pub enum MyError {
//!     NotFound,
//!     Unauthorized,
//!     DatabaseError(String),
//! }
//!
//! #[cfg(feature = "server")]
//! impl ErrorStatusCode for MyError {
//!     fn status_code(&self) -> axum::http::StatusCode {
//!         match self {
//!             MyError::NotFound => axum::http::StatusCode::NOT_FOUND,
//!             MyError::Unauthorized => axum::http::StatusCode::UNAUTHORIZED,
//!             MyError::DatabaseError(_) => axum::http::StatusCode::INTERNAL_SERVER_ERROR,
//!         }
//!     }
//! }
//! ```
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

/// Default error type used when no custom error is specified.
///
/// This preserves backwards compatibility: `ServerFnError` (without a type
/// parameter) is equivalent to `ServerFnError<DefaultError>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DefaultError(pub String);

impl std::fmt::Display for DefaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Trait for mapping application errors to HTTP status codes.
///
/// Implement this for your custom error types to control which HTTP status
/// code is returned when the error is sent across the wire.
///
/// This trait is only available on the server (it depends on `axum::http::StatusCode`).
#[cfg(feature = "server")]
pub trait ErrorStatusCode {
    /// Returns the HTTP status code for this error.
    fn status_code(&self) -> axum::http::StatusCode;
}

#[cfg(feature = "server")]
impl ErrorStatusCode for DefaultError {
    fn status_code(&self) -> axum::http::StatusCode {
        axum::http::StatusCode::INTERNAL_SERVER_ERROR
    }
}

/// Error type for server function calls.
///
/// `ServerFnError` is generic over a custom application error type `E`.
/// The default type parameter is [`DefaultError`], so existing code using
/// `ServerFnError` without a type parameter continues to work.
///
/// # Variants
///
/// - `AppError(E)` — a typed application error (e.g. `NotFound`, `Unauthorized`)
/// - `Serialization(String)` — failed to serialize request data
/// - `Deserialization(String)` — failed to deserialize response data
/// - `Request(String)` — HTTP request failed (client-side)
/// - `ServerError(String)` — server returned an untyped error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerFnError<E = DefaultError>
where
    E: Clone + std::fmt::Debug,
{
    /// A typed application error.
    AppError(E),

    /// Failed to serialize request data.
    Serialization(String),

    /// Failed to deserialize response data.
    Deserialization(String),

    /// HTTP request failed.
    Request(String),

    /// Server returned an error response.
    ServerError(String),
}

impl<E> ServerFnError<E>
where
    E: Clone + std::fmt::Debug,
{
    /// Create an `AppError` variant from a custom error value.
    pub fn app(error: E) -> Self {
        Self::AppError(error)
    }

    /// Create a `ServerError` variant from a string message.
    pub fn server(msg: impl Into<String>) -> Self {
        Self::ServerError(msg.into())
    }
}

impl<E> std::fmt::Display for ServerFnError<E>
where
    E: Clone + std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerFnError::AppError(e) => write!(f, "Application error: {:?}", e),
            ServerFnError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            ServerFnError::Deserialization(msg) => write!(f, "Deserialization error: {}", msg),
            ServerFnError::Request(msg) => write!(f, "Request error: {}", msg),
            ServerFnError::ServerError(msg) => write!(f, "Server error: {}", msg),
        }
    }
}

impl<E> std::error::Error for ServerFnError<E> where E: Clone + std::fmt::Debug {}

// ---------------------------------------------------------------------------
// Convenient `From` implementations
// ---------------------------------------------------------------------------

impl<E> From<String> for ServerFnError<E>
where
    E: Clone + std::fmt::Debug,
{
    fn from(msg: String) -> Self {
        ServerFnError::ServerError(msg)
    }
}

impl<E> From<&str> for ServerFnError<E>
where
    E: Clone + std::fmt::Debug,
{
    fn from(msg: &str) -> Self {
        ServerFnError::ServerError(msg.to_string())
    }
}

// ---------------------------------------------------------------------------
// Client-side RPC call
// ---------------------------------------------------------------------------

/// Client-side function to call a server API endpoint.
///
/// This makes a POST request to the given path with JSON-serialized arguments,
/// and deserializes the JSON response.
///
/// On error the HTTP status code is included in the error message for
/// debugging. If the server responds with a serialized `ServerFnError<E>`,
/// the typed error is preserved across the boundary.
#[cfg(feature = "client")]
pub async fn call<Args, Resp, E>(path: &str, args: &Args) -> Result<Resp, ServerFnError<E>>
where
    Args: Serialize,
    Resp: DeserializeOwned,
    E: Clone + std::fmt::Debug + DeserializeOwned,
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

        // Try to deserialize a typed ServerFnError from the response body.
        let text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());

        if let Ok(typed_err) = serde_json::from_str::<ServerFnError<E>>(&text) {
            return Err(typed_err);
        }

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

// ---------------------------------------------------------------------------
// Server-side helpers
// ---------------------------------------------------------------------------

/// Server-side helper to extract JSON and call a handler.
///
/// This is a convenience wrapper for axum handlers that take JSON input.
#[cfg(feature = "server")]
pub mod server {
    use super::{DefaultError, ErrorStatusCode, ServerFnError};
    use axum::{Json, http::StatusCode, response::IntoResponse};
    use serde::{Deserialize, Serialize};

    // `DefaultError` always maps to 500, but we need a blanket-friendly way
    // for `ApiResponse` to pick the right status. We require `ErrorStatusCode`
    // on the `E` parameter.

    /// Response wrapper that serializes errors as JSON.
    ///
    /// The HTTP status code is derived from the error via [`ErrorStatusCode`].
    /// Infrastructure variants (`Serialization`, `Deserialization`, etc.)
    /// always produce `500 Internal Server Error`.
    pub struct ApiResponse<T, E = DefaultError>(pub Result<T, ServerFnError<E>>)
    where
        E: Clone + std::fmt::Debug;

    impl<T, E> IntoResponse for ApiResponse<T, E>
    where
        T: Serialize,
        E: Clone + std::fmt::Debug + Serialize + ErrorStatusCode,
    {
        fn into_response(self) -> axum::response::Response {
            match self.0 {
                Ok(value) => Json(value).into_response(),
                Err(ref err) => {
                    let status = match err {
                        ServerFnError::AppError(e) => e.status_code(),
                        _ => StatusCode::INTERNAL_SERVER_ERROR,
                    };
                    // Serialize the full ServerFnError<E> so the client can
                    // reconstruct the typed error.
                    (status, Json(err)).into_response()
                }
            }
        }
    }

    /// Helper trait for converting function results to API responses.
    pub trait IntoApiResponse<T, E = DefaultError>
    where
        E: Clone + std::fmt::Debug,
    {
        fn into_api_response(self) -> ApiResponse<T, E>;
    }

    impl<T, E> IntoApiResponse<T, E> for Result<T, ServerFnError<E>>
    where
        E: Clone + std::fmt::Debug,
    {
        fn into_api_response(self) -> ApiResponse<T, E> {
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
    #[allow(clippy::type_complexity)]
    pub fn json_handler<Args, Resp, E, F, Fut>(
        f: F,
    ) -> impl Fn(
        Json<Args>,
    )
        -> std::pin::Pin<Box<dyn std::future::Future<Output = ApiResponse<Resp, E>> + Send>>
    + Clone
    + Send
    where
        Args: for<'de> Deserialize<'de> + Send + 'static,
        Resp: Serialize + Send + 'static,
        E: Clone + std::fmt::Debug + Serialize + ErrorStatusCode + Send + 'static,
        F: Fn(Args) -> Fut + Clone + Send + 'static,
        Fut: std::future::Future<Output = Result<Resp, ServerFnError<E>>> + Send + 'static,
    {
        move |Json(args): Json<Args>| {
            let f = f.clone();
            Box::pin(async move { ApiResponse(f(args).await) })
        }
    }
}

#[cfg(feature = "server")]
pub use server::{ApiResponse, IntoApiResponse, json_handler};

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- DefaultError backwards compatibility ---------------------------------

    #[test]
    fn server_fn_error_without_type_param_compiles() {
        // This is the critical backwards compatibility test: `ServerFnError`
        // without a generic parameter should still work.
        let err: ServerFnError = ServerFnError::ServerError("boom".into());
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn server_fn_error_display() {
        let err: ServerFnError = ServerFnError::Serialization("bad json".into());
        assert_eq!(err.to_string(), "Serialization error: bad json");

        let err: ServerFnError = ServerFnError::Deserialization("unexpected token".into());
        assert_eq!(err.to_string(), "Deserialization error: unexpected token");

        let err: ServerFnError = ServerFnError::Request("timeout".into());
        assert_eq!(err.to_string(), "Request error: timeout");

        let err: ServerFnError = ServerFnError::ServerError("internal".into());
        assert_eq!(err.to_string(), "Server error: internal");
    }

    #[test]
    fn default_error_app_variant() {
        let err: ServerFnError = ServerFnError::AppError(DefaultError("custom".into()));
        assert!(err.to_string().contains("custom"));
    }

    // -- Custom typed errors --------------------------------------------------

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    enum MyError {
        NotFound,
        Unauthorized,
        DatabaseError(String),
    }

    #[test]
    fn typed_error_app_variant() {
        let err: ServerFnError<MyError> = ServerFnError::app(MyError::NotFound);
        match &err {
            ServerFnError::AppError(MyError::NotFound) => {}
            other => panic!("expected AppError(NotFound), got {:?}", other),
        }
    }

    #[test]
    fn typed_error_display() {
        let err: ServerFnError<MyError> =
            ServerFnError::app(MyError::DatabaseError("timeout".into()));
        let display = err.to_string();
        assert!(display.contains("Application error"));
        assert!(display.contains("DatabaseError"));
    }

    #[test]
    fn typed_error_server_helper() {
        let err: ServerFnError<MyError> = ServerFnError::server("something went wrong");
        match &err {
            ServerFnError::ServerError(msg) => assert_eq!(msg, "something went wrong"),
            other => panic!("expected ServerError, got {:?}", other),
        }
    }

    // -- From conversions -----------------------------------------------------

    #[test]
    fn from_string() {
        let err: ServerFnError = ServerFnError::from("oops".to_string());
        match err {
            ServerFnError::ServerError(msg) => assert_eq!(msg, "oops"),
            other => panic!("expected ServerError, got {:?}", other),
        }
    }

    #[test]
    fn from_str() {
        let err: ServerFnError = ServerFnError::from("oops");
        match err {
            ServerFnError::ServerError(msg) => assert_eq!(msg, "oops"),
            other => panic!("expected ServerError, got {:?}", other),
        }
    }

    #[test]
    fn from_string_typed() {
        let err: ServerFnError<MyError> = "db down".into();
        match err {
            ServerFnError::ServerError(msg) => assert_eq!(msg, "db down"),
            other => panic!("expected ServerError, got {:?}", other),
        }
    }

    // -- Serialization round-trip ---------------------------------------------

    #[test]
    fn serde_round_trip_default() {
        let err: ServerFnError = ServerFnError::ServerError("test".into());
        let json = serde_json::to_string(&err).unwrap();
        let back: ServerFnError = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, ServerFnError::ServerError(ref m) if m == "test"));
    }

    #[test]
    fn serde_round_trip_typed() {
        let err: ServerFnError<MyError> = ServerFnError::app(MyError::NotFound);
        let json = serde_json::to_string(&err).unwrap();
        let back: ServerFnError<MyError> = serde_json::from_str(&json).unwrap();
        assert!(matches!(back, ServerFnError::AppError(MyError::NotFound)));
    }

    #[test]
    fn serde_round_trip_typed_with_data() {
        let err: ServerFnError<MyError> =
            ServerFnError::app(MyError::DatabaseError("connection refused".into()));
        let json = serde_json::to_string(&err).unwrap();
        let back: ServerFnError<MyError> = serde_json::from_str(&json).unwrap();
        match back {
            ServerFnError::AppError(MyError::DatabaseError(msg)) => {
                assert_eq!(msg, "connection refused");
            }
            other => panic!("unexpected: {:?}", other),
        }
    }

    #[test]
    fn serde_infrastructure_variants_typed() {
        // Infrastructure variants should serialize/deserialize regardless of E
        let cases: Vec<ServerFnError<MyError>> = vec![
            ServerFnError::Serialization("a".into()),
            ServerFnError::Deserialization("b".into()),
            ServerFnError::Request("c".into()),
            ServerFnError::ServerError("d".into()),
        ];
        for err in cases {
            let json = serde_json::to_string(&err).unwrap();
            let back: ServerFnError<MyError> = serde_json::from_str(&json).unwrap();
            assert_eq!(err.to_string(), back.to_string());
        }
    }

    // -- ErrorStatusCode (server feature) ------------------------------------

    #[cfg(feature = "server")]
    mod server_tests {
        use super::*;
        use crate::rpc::ErrorStatusCode;
        use crate::rpc::server::ApiResponse;
        use axum::http::StatusCode;
        use axum::response::IntoResponse;
        use http_body_util::BodyExt;

        impl ErrorStatusCode for MyError {
            fn status_code(&self) -> StatusCode {
                match self {
                    MyError::NotFound => StatusCode::NOT_FOUND,
                    MyError::Unauthorized => StatusCode::UNAUTHORIZED,
                    MyError::DatabaseError(_) => StatusCode::INTERNAL_SERVER_ERROR,
                }
            }
        }

        async fn body_to_string(response: axum::response::Response) -> String {
            let body = response.into_body();
            let bytes = body.collect().await.unwrap().to_bytes();
            String::from_utf8(bytes.to_vec()).unwrap()
        }

        #[tokio::test]
        async fn api_response_ok() {
            let resp: ApiResponse<i32> = ApiResponse(Ok(42));
            let response = resp.into_response();
            assert_eq!(response.status(), StatusCode::OK);
            let body = body_to_string(response).await;
            assert_eq!(body, "42");
        }

        #[tokio::test]
        async fn api_response_default_error_is_500() {
            let resp: ApiResponse<i32> =
                ApiResponse(Err(ServerFnError::ServerError("fail".into())));
            let response = resp.into_response();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn api_response_typed_error_status_code() {
            let resp: ApiResponse<String, MyError> =
                ApiResponse(Err(ServerFnError::app(MyError::NotFound)));
            let response = resp.into_response();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);

            let body = body_to_string(response).await;
            // The body should be a serialized ServerFnError<MyError>
            let deserialized: ServerFnError<MyError> = serde_json::from_str(&body).unwrap();
            assert!(matches!(
                deserialized,
                ServerFnError::AppError(MyError::NotFound)
            ));
        }

        #[tokio::test]
        async fn api_response_typed_error_unauthorized() {
            let resp: ApiResponse<String, MyError> =
                ApiResponse(Err(ServerFnError::app(MyError::Unauthorized)));
            let response = resp.into_response();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }

        #[tokio::test]
        async fn api_response_typed_infrastructure_error_is_500() {
            let resp: ApiResponse<String, MyError> =
                ApiResponse(Err(ServerFnError::Serialization("bad data".into())));
            let response = resp.into_response();
            assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        }

        #[tokio::test]
        async fn api_response_typed_error_body_round_trip() {
            let original_err = ServerFnError::app(MyError::DatabaseError("connection lost".into()));
            let resp: ApiResponse<String, MyError> = ApiResponse(Err(original_err));
            let response = resp.into_response();
            let body = body_to_string(response).await;
            let recovered: ServerFnError<MyError> = serde_json::from_str(&body).unwrap();
            match recovered {
                ServerFnError::AppError(MyError::DatabaseError(msg)) => {
                    assert_eq!(msg, "connection lost");
                }
                other => panic!("unexpected: {:?}", other),
            }
        }

        #[tokio::test]
        async fn json_handler_default_error() {
            use crate::rpc::json_handler;

            async fn add(args: (i32, i32)) -> Result<i32, ServerFnError> {
                Ok(args.0 + args.1)
            }

            let handler = json_handler(add);
            let resp = handler(axum::extract::Json((2, 3))).await;
            let response = resp.into_response();
            assert_eq!(response.status(), StatusCode::OK);
            let body = body_to_string(response).await;
            assert_eq!(body, "5");
        }

        #[tokio::test]
        async fn json_handler_typed_error() {
            use crate::rpc::json_handler;

            async fn fail(_args: ()) -> Result<String, ServerFnError<MyError>> {
                Err(ServerFnError::app(MyError::Unauthorized))
            }

            let handler = json_handler(fail);
            let resp = handler(axum::extract::Json(())).await;
            let response = resp.into_response();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        }
    }
}
