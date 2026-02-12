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
//! - Reactive shared state synchronization (`SharedState` / `StateSync`)
//! - Simple RPC helpers for client-server communication
//! - Typed error boundaries via [`ServerFnError<E>`](rpc::ServerFnError)
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

// ============================================================================
// RPC support
// ============================================================================

pub mod rpc;

// Re-export the server macro
pub use axum_egui_macro::server;

// ============================================================================
// Server-only: App wrapper and static file serving
// ============================================================================

#[cfg(feature = "server")]
mod app {
    use axum::{
        body::Body,
        http::{HeaderMap, StatusCode, Uri, header},
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

    /// Check if the `Accept-Encoding` header includes the given encoding.
    fn accepts_encoding(headers: &HeaderMap, encoding: &str) -> bool {
        headers
            .get(header::ACCEPT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.split(',').any(|part| part.trim().starts_with(encoding)))
    }

    /// Check if a filename contains a content hash (e.g., `app.abc12345.js`).
    ///
    /// A hashed filename has the form `stem.XXXXXXXX.ext` where the hash part
    /// is exactly 8 hex characters.
    fn has_content_hash(filename: &str) -> bool {
        let parts: Vec<&str> = filename.splitn(3, '.').collect();
        if parts.len() == 3 {
            let hash_part = parts[1];
            hash_part.len() == 8 && hash_part.chars().all(|c| c.is_ascii_hexdigit())
        } else {
            false
        }
    }

    /// Handler for serving static assets from an embedded `RustEmbed` type.
    ///
    /// This handler supports:
    /// - **Pre-compressed assets**: Serves `.br` (brotli) or `.gz` (gzip) versions
    ///   when the client indicates support via `Accept-Encoding`.
    /// - **Cache busting**: Files with content hashes in their name (e.g., `app.abc123.js`)
    ///   are served with long-lived immutable cache headers. Non-hashed files like
    ///   `index.html` get `Cache-Control: no-cache`.
    ///
    /// Compatible with axum's extractor system:
    /// ```ignore
    /// let app = Router::new()
    ///     .fallback(axum_egui::static_handler::<Assets>);
    /// ```
    pub async fn static_handler<A: RustEmbed>(headers: HeaderMap, uri: Uri) -> impl IntoResponse {
        let path = uri.path().trim_start_matches('/');

        match A::get(path) {
            Some(content) => {
                let mime = mime_guess::from_path(path).first_or_octet_stream();
                let filename = path.rsplit('/').next().unwrap_or(path);

                // Determine cache control based on whether filename has a content hash
                let cache_control = if has_content_hash(filename) {
                    "public, max-age=31536000, immutable"
                } else {
                    "no-cache"
                };

                // Try to serve a pre-compressed version
                if accepts_encoding(&headers, "br") {
                    let br_path = format!("{}.br", path);
                    if let Some(br_content) = A::get(&br_path) {
                        return Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, mime.as_ref())
                            .header(header::CONTENT_ENCODING, "br")
                            .header(header::CACHE_CONTROL, cache_control)
                            .header(header::VARY, "Accept-Encoding")
                            .body(Body::from(br_content.data.to_vec()))
                            .unwrap();
                    }
                }

                if accepts_encoding(&headers, "gzip") {
                    let gz_path = format!("{}.gz", path);
                    if let Some(gz_content) = A::get(&gz_path) {
                        return Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, mime.as_ref())
                            .header(header::CONTENT_ENCODING, "gzip")
                            .header(header::CACHE_CONTROL, cache_control)
                            .header(header::VARY, "Accept-Encoding")
                            .body(Body::from(gz_content.data.to_vec()))
                            .unwrap();
                    }
                }

                // Serve uncompressed original
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, mime.as_ref())
                    .header(header::CACHE_CONTROL, cache_control)
                    .body(Body::from(content.data.to_vec()))
                    .unwrap()
            }
            None => match A::get("index.html") {
                Some(content) => Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/html")
                    .header(header::CACHE_CONTROL, "no-cache")
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

// ============================================================================
// Reactive shared state synchronization
// ============================================================================

#[cfg(any(feature = "server", feature = "client"))]
pub mod sync;

// Re-export commonly used items at the crate root
pub use rpc::{DefaultError, ServerFnError};

#[cfg(feature = "server")]
pub use rpc::ErrorStatusCode;

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::rpc::{DefaultError, ServerFnError};
    pub use crate::server;

    #[cfg(feature = "server")]
    pub use crate::{App, static_handler};

    #[cfg(feature = "server")]
    pub use crate::rpc::{ApiResponse, ErrorStatusCode, IntoApiResponse, json_handler};

    #[cfg(feature = "server")]
    pub use crate::sse::{Event, KeepAlive, Sse, SseExt};

    #[cfg(feature = "server")]
    pub use crate::ws::{JsonWebSocket, Message, WebSocket, WebSocketUpgrade, WebSocketUpgradeExt};

    #[cfg(feature = "server")]
    pub use crate::sync::{SharedState, SharedStateReceiver};

    #[cfg(feature = "client")]
    pub use crate::rpc::call;

    #[cfg(feature = "client")]
    pub use crate::ws::{WsClientReceiver, WsClientSender, WsError, WsStream};

    #[cfg(feature = "client")]
    pub use crate::sync::{StateSync, SyncError, SyncReceiver};
}

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
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

    async fn body_to_bytes(response: axum::response::Response) -> Vec<u8> {
        let body = response.into_body();
        let bytes = body.collect().await.unwrap().to_bytes();
        bytes.to_vec()
    }

    fn empty_headers() -> HeaderMap {
        HeaderMap::new()
    }

    fn headers_with_encoding(encoding: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_str(encoding).unwrap(),
        );
        headers
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
        let response = static_handler::<TestAssets>(empty_headers(), uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/javascript"
        );
    }

    #[tokio::test]
    async fn static_handler_serves_wasm_with_correct_mime() {
        let uri: Uri = "/app.wasm".parse().unwrap();
        let response = static_handler::<TestAssets>(empty_headers(), uri)
            .await
            .into_response();

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
        let response = static_handler::<TestAssets>(empty_headers(), uri)
            .await
            .into_response();

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
        let response = static_handler::<TestAssetsNoIndex>(empty_headers(), uri)
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

    // ========================================================================
    // Compression serving tests
    // ========================================================================

    #[tokio::test]
    async fn static_handler_serves_brotli_when_accepted() {
        let uri: Uri = "/app.js".parse().unwrap();
        let headers = headers_with_encoding("br, gzip, deflate");
        let response = static_handler::<TestAssets>(headers, uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/javascript"
        );
        assert_eq!(response.headers().get("content-encoding").unwrap(), "br");
        assert_eq!(response.headers().get("vary").unwrap(), "Accept-Encoding");

        // Body should be the brotli-compressed data, not the original
        let body = body_to_bytes(response).await;
        assert_ne!(body, b"// Test JavaScript file\nconsole.log(\"test\");");
    }

    #[tokio::test]
    async fn static_handler_serves_gzip_when_brotli_not_accepted() {
        let uri: Uri = "/app.js".parse().unwrap();
        let headers = headers_with_encoding("gzip, deflate");
        let response = static_handler::<TestAssets>(headers, uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/javascript"
        );
        assert_eq!(response.headers().get("content-encoding").unwrap(), "gzip");

        // Body should start with gzip magic bytes
        let body = body_to_bytes(response).await;
        assert!(body.len() >= 2);
        assert_eq!(body[0], 0x1f);
        assert_eq!(body[1], 0x8b);
    }

    #[tokio::test]
    async fn static_handler_serves_uncompressed_when_no_encoding_accepted() {
        let uri: Uri = "/app.js".parse().unwrap();
        let response = static_handler::<TestAssets>(empty_headers(), uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/javascript"
        );
        assert!(response.headers().get("content-encoding").is_none());

        let body = body_to_string(response).await;
        assert_eq!(body, "// Test JavaScript file\nconsole.log(\"test\");\n");
    }

    #[tokio::test]
    async fn static_handler_serves_compressed_wasm() {
        let uri: Uri = "/app.wasm".parse().unwrap();
        let headers = headers_with_encoding("br");
        let response = static_handler::<TestAssets>(headers, uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/wasm"
        );
        assert_eq!(response.headers().get("content-encoding").unwrap(), "br");
    }

    // ========================================================================
    // Cache control tests
    // ========================================================================

    #[tokio::test]
    async fn static_handler_cache_control_no_cache_for_index_html() {
        // Direct request to index.html
        let uri: Uri = "/index.html".parse().unwrap();
        let response = static_handler::<TestAssets>(empty_headers(), uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-cache");
    }

    #[tokio::test]
    async fn static_handler_cache_control_no_cache_for_fallback() {
        // Fallback to index.html for unknown path
        let uri: Uri = "/unknown/path".parse().unwrap();
        let response = static_handler::<TestAssets>(empty_headers(), uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-cache");
    }

    #[tokio::test]
    async fn static_handler_cache_control_immutable_for_hashed_files() {
        // Request a content-hashed file
        let uri: Uri = "/app.e55aa7a8.js".parse().unwrap();
        let response = static_handler::<TestAssets>(empty_headers(), uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "public, max-age=31536000, immutable"
        );
    }

    #[tokio::test]
    async fn static_handler_cache_control_no_cache_for_unhashed_files() {
        // Regular file without content hash
        let uri: Uri = "/app.js".parse().unwrap();
        let response = static_handler::<TestAssets>(empty_headers(), uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("cache-control").unwrap(), "no-cache");
    }

    #[tokio::test]
    async fn static_handler_compressed_hashed_file_has_immutable_cache() {
        let uri: Uri = "/app.e55aa7a8.js".parse().unwrap();
        let headers = headers_with_encoding("br");
        let response = static_handler::<TestAssets>(headers, uri)
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("content-encoding").unwrap(), "br");
        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "public, max-age=31536000, immutable"
        );
    }
}
