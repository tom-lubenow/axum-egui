//! axum-egui: Seamlessly embed egui frontends in axum backends.
//!
//! This crate provides utilities for serving egui WASM applications from axum,
//! with support for server-side initial state injection and real-time updates.
//!
//! # Features
//!
//! - `App<T>` response wrapper for serving egui apps with initial state
//! - Static file serving utilities for embedded assets
//! - Server-Sent Events (SSE) for real-time server-to-client updates
//! - WebSockets for bidirectional real-time communication
//! - `#[server]` macro for type-safe RPC between client and server
//!
//! # Server Example
//!
//! ```ignore
//! use axum::{Router, routing::get};
//! use axum_egui::sse::{Sse, Event, KeepAlive};
//! use rust_embed::RustEmbed;
//!
//! #[derive(RustEmbed)]
//! #[folder = "$MY_FRONTEND_DIST"]
//! struct Assets;
//!
//! #[derive(serde::Serialize, serde::Deserialize, Default)]
//! struct MyApp { counter: i32 }
//!
//! async fn index() -> axum_egui::App<MyApp, Assets> {
//!     axum_egui::App::new(MyApp { counter: 42 })
//! }
//!
//! // SSE endpoint for real-time updates
//! async fn events() -> Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>> {
//!     use futures_util::stream;
//!     let stream = stream::repeat_with(|| {
//!         Ok(Event::new().json_data(42).unwrap())
//!     });
//!     Sse::new(stream).keep_alive(KeepAlive::default())
//! }
//!
//! #[tokio::main]
//! async fn main() {
//!     let app = Router::new()
//!         .route("/", get(index))
//!         .route("/events", get(events))
//!         .fallback(axum_egui::static_handler::<Assets>);
//!     // ...
//! }
//! ```
//!
//! # Server Functions
//!
//! Use the `#[server]` macro to define functions that work on both server and client:
//!
//! ```ignore
//! use axum_egui::{server, ServerFnError};
//!
//! #[server]
//! pub async fn greet(name: String) -> Result<String, ServerFnError> {
//!     Ok(format!("Hello, {}!", name))
//! }
//! ```
//!
//! On the server, this generates an axum handler. On the client (WASM), it generates
//! a function that makes an HTTP request to the server.

// ============================================================================
// Server function support
// ============================================================================

/// Re-export the server macro.
pub use axum_egui_macro::server;

/// Error type for server functions.
#[derive(Debug, Clone)]
pub enum ServerFnError {
    /// Failed to serialize the request.
    Serialization(String),
    /// Failed to deserialize the response.
    Deserialization(String),
    /// HTTP request failed.
    Request(String),
    /// Server returned an error.
    ServerError(String),
}

impl std::fmt::Display for ServerFnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerFnError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            ServerFnError::Deserialization(msg) => write!(f, "Deserialization error: {}", msg),
            ServerFnError::Request(msg) => write!(f, "Request error: {}", msg),
            ServerFnError::ServerError(msg) => write!(f, "Server error: {}", msg),
        }
    }
}

impl std::error::Error for ServerFnError {}

/// Private module for macro-generated code.
/// Not part of the public API.
#[doc(hidden)]
pub mod __private {
    pub use serde_json;

    #[cfg(feature = "client")]
    pub use gloo_net;
}

// ============================================================================
// Server-only: App wrapper and static file serving
// ============================================================================

#[cfg(feature = "server")]
mod app {
    use axum::{
        body::Body,
        http::{StatusCode, Uri, header},
        response::{Html, IntoResponse, Response},
    };
    use rust_embed::RustEmbed;
    use serde::Serialize;
    use std::marker::PhantomData;

    /// Axum response wrapper for serving egui apps with initial state.
    ///
    /// This wrapper injects serialized state into the HTML template, allowing
    /// the frontend to hydrate with server-provided data.
    pub struct App<T, A: RustEmbed> {
        state: T,
        _assets: PhantomData<A>,
    }

    impl<T, A: RustEmbed> App<T, A> {
        /// Create a new App response with the given initial state.
        pub fn new(state: T) -> Self {
            Self {
                state,
                _assets: PhantomData,
            }
        }
    }

    impl<T: Serialize, A: RustEmbed> IntoResponse for App<T, A> {
        fn into_response(self) -> Response {
            let state_json = match serde_json::to_string(&self.state) {
                Ok(json) => json,
                Err(e) => {
                    return Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from(format!("Failed to serialize app state: {e}")))
                        .unwrap();
                }
            };

            let html = match A::get("index.html") {
                Some(content) => {
                    let html_str = String::from_utf8_lossy(&content.data);
                    let state_script = format!(
                        r#"<script id="axum-egui-state" type="application/json">{}</script>"#,
                        state_json.replace("</", "<\\/")
                    );
                    html_str.replace("<!--AXUM_EGUI_INITIAL_STATE-->", &state_script)
                }
                None => {
                    return Response::builder()
                        .status(StatusCode::INTERNAL_SERVER_ERROR)
                        .body(Body::from(
                            "Frontend assets not found. Did you build the frontend?",
                        ))
                        .unwrap();
                }
            };

            Html(html).into_response()
        }
    }

    /// Handler for serving static assets from an embedded `RustEmbed` type.
    pub async fn static_handler<A: RustEmbed>(uri: Uri) -> impl IntoResponse {
        let path = uri.path().trim_start_matches('/');

        match A::get(path) {
            Some(content) => {
                let mime = mime_guess::from_path(path).first_or_octet_stream();

                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, mime.as_ref())
                    .body(Body::from(content.data.to_vec()))
                    .unwrap()
            }
            None => match A::get("index.html") {
                Some(content) => Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html")
                    .body(Body::from(content.data.to_vec()))
                    .unwrap(),
                None => Response::builder()
                    .status(StatusCode::NOT_FOUND)
                    .body(Body::from("404 Not Found"))
                    .unwrap(),
            },
        }
    }
}

#[cfg(feature = "server")]
pub use app::{App, static_handler};

// ============================================================================
// SSE (Server-Sent Events) support
// ============================================================================

#[cfg(any(feature = "server", feature = "client"))]
pub mod sse;

// ============================================================================
// WebSocket support
// ============================================================================

#[cfg(any(feature = "server", feature = "client"))]
pub mod ws;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::{ServerFnError, server};

    #[cfg(feature = "server")]
    pub use crate::{App, static_handler};

    #[cfg(feature = "server")]
    pub use crate::sse::{Event, KeepAlive, Sse, SseExt};

    #[cfg(feature = "server")]
    pub use crate::ws::{JsonWebSocket, Message, WebSocket, WebSocketUpgrade, WebSocketUpgradeExt};

    #[cfg(feature = "client")]
    pub use crate::ws::{WsClientReceiver, WsClientSender, WsError, WsStream};
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use axum::http::{StatusCode, Uri};
    use axum::response::IntoResponse;
    use http_body_util::BodyExt;
    use rust_embed::RustEmbed;
    use serde::{Deserialize, Serialize};

    // Mock assets for testing
    #[derive(RustEmbed)]
    #[folder = "src/test_assets/"]
    struct TestAssets;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct TestState {
        counter: i32,
        message: String,
    }

    async fn body_to_string(response: axum::response::Response) -> String {
        let body = response.into_body();
        let bytes = body.collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn app_injects_state_into_html() {
        let state = TestState {
            counter: 42,
            message: "Hello".into(),
        };
        let app: App<TestState, TestAssets> = App::new(state.clone());
        let response = app.into_response();

        assert_eq!(response.status(), StatusCode::OK);

        let body = body_to_string(response).await;

        // Should contain the state script tag
        assert!(body.contains(r#"<script id="axum-egui-state" type="application/json">"#));
        assert!(body.contains(r#""counter":42"#));
        assert!(body.contains(r#""message":"Hello""#));

        // Should have replaced the placeholder
        assert!(!body.contains("<!--AXUM_EGUI_INITIAL_STATE-->"));
    }

    #[tokio::test]
    async fn app_escapes_script_closing_tag() {
        // Test that </script> in state is properly escaped
        let state = TestState {
            counter: 1,
            message: "</script><script>alert('xss')".into(),
        };
        let app: App<TestState, TestAssets> = App::new(state);
        let response = app.into_response();
        let body = body_to_string(response).await;

        // Should escape </ to <\/ to prevent script injection
        assert!(body.contains(r#"<\/script>"#));
        assert!(!body.contains(r#"</script><script>"#));
    }

    #[tokio::test]
    async fn static_handler_serves_js_with_correct_mime() {
        let uri: Uri = "/app.js".parse().unwrap();
        let response = static_handler::<TestAssets>(uri).await.into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/javascript"
        );
    }

    #[tokio::test]
    async fn static_handler_serves_wasm_with_correct_mime() {
        let uri: Uri = "/app.wasm".parse().unwrap();
        let response = static_handler::<TestAssets>(uri).await.into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/wasm"
        );
    }

    #[tokio::test]
    async fn static_handler_falls_back_to_index_html() {
        // Unknown path should return index.html for SPA routing
        let uri: Uri = "/some/unknown/path".parse().unwrap();
        let response = static_handler::<TestAssets>(uri).await.into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("content-type").unwrap(), "text/html");

        let body = body_to_string(response).await;
        assert!(body.contains("<!--AXUM_EGUI_INITIAL_STATE-->"));
    }

    // Test assets without index.html
    #[derive(RustEmbed)]
    #[folder = "src/test_assets_no_index/"]
    struct TestAssetsNoIndex;

    #[tokio::test]
    async fn static_handler_returns_404_when_no_index() {
        let uri: Uri = "/unknown".parse().unwrap();
        let response = static_handler::<TestAssetsNoIndex>(uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn app_returns_error_when_no_index_html() {
        let state = TestState {
            counter: 1,
            message: "test".into(),
        };
        let app: App<TestState, TestAssetsNoIndex> = App::new(state);
        let response = app.into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
