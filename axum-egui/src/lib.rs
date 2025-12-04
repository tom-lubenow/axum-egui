//! axum-egui: Seamlessly embed egui frontends in axum backends.
//!
//! This crate provides utilities for serving egui WASM applications from axum,
//! with support for server-side initial state injection.
//!
//! # Features
//!
//! - `App<T>` response wrapper for serving egui apps with initial state
//! - Static file serving utilities for embedded assets
//! - Re-exports of `server-fn` for RPC, SSE, and WebSocket support
//!
//! # Example
//!
//! ```ignore
//! use axum::{Router, routing::get};
//! use axum_egui::{App, create_static_handler};
//! use serde::{Serialize, Deserialize};
//! use rust_embed::RustEmbed;
//!
//! #[derive(RustEmbed)]
//! #[folder = "frontend/dist/"]
//! struct Assets;
//!
//! #[derive(Serialize, Deserialize, Default)]
//! struct MyApp {
//!     counter: i32,
//! }
//!
//! async fn index() -> App<MyApp, Assets> {
//!     App::new(MyApp { counter: 42 })
//! }
//!
//! let router = Router::new()
//!     .route("/", get(index))
//!     .fallback(axum_egui::static_handler::<Assets>);
//! ```

use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
};
use rust_embed::RustEmbed;
use serde::Serialize;
use std::marker::PhantomData;

// Re-export server-fn for convenient access
pub use server_fn;
pub use server_fn::prelude::*;

/// Axum response wrapper for serving egui apps with initial state.
///
/// This wrapper injects serialized state into the HTML template, allowing
/// the frontend to hydrate with server-provided data.
///
/// # Type Parameters
///
/// - `T`: The application state type (must implement `Serialize`)
/// - `A`: The `RustEmbed` asset type containing the frontend files
///
/// # Example
///
/// ```ignore
/// use axum_egui::App;
/// use rust_embed::RustEmbed;
///
/// #[derive(RustEmbed)]
/// #[folder = "frontend/dist/"]
/// struct Assets;
///
/// #[derive(serde::Serialize)]
/// struct MyState {
///     message: String,
/// }
///
/// async fn handler() -> App<MyState, Assets> {
///     App::new(MyState {
///         message: "Hello from server!".into(),
///     })
/// }
/// ```
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
///
/// This handler serves files from the embedded assets, with fallback to
/// `index.html` for SPA-style routing.
///
/// # Example
///
/// ```ignore
/// use axum::Router;
/// use axum_egui::static_handler;
/// use rust_embed::RustEmbed;
///
/// #[derive(RustEmbed)]
/// #[folder = "frontend/dist/"]
/// struct Assets;
///
/// let router = Router::new()
///     .fallback(static_handler::<Assets>);
/// ```
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

/// Prelude module for convenient imports.
pub mod prelude {
    pub use super::{App, static_handler};
    pub use server_fn::prelude::*;
}
