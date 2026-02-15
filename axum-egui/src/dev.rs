//! Hot-reload development server for axum-egui.
//!
//! This module provides a development mode that:
//! - Serves frontend assets from the filesystem (not embedded via RustEmbed)
//! - Watches for file changes using the `notify` crate
//! - Auto-refreshes the browser via a WebSocket connection when files change
//!
//! # Overview
//!
//! During development, the normal workflow requires a full rebuild cycle:
//! compile WASM frontend -> wasm-bindgen -> embed in server -> recompile server.
//! This is slow and disruptive.
//!
//! With the `dev` feature enabled, you can instead:
//! 1. Rebuild only the WASM frontend (in a watch loop)
//! 2. Run the server with `--features dev`, serving assets from disk
//! 3. The browser automatically reloads when the frontend rebuild completes
//!
//! # Usage
//!
//! Enable the `dev` feature in your server's `Cargo.toml`:
//!
//! ```toml
//! [dependencies]
//! axum-egui = { version = "0.2", features = ["server", "dev"] }
//! ```
//!
//! Set up the dev server in your main.rs:
//!
//! ```ignore
//! use axum::{Router, routing::get};
//! use axum_egui::dev::{DevServer, dev_static_handler};
//!
//! #[tokio::main]
//! async fn main() {
//!     let dist_dir = "./target/dev-dist";
//!
//!     // Create the dev server with file watching
//!     let dev = DevServer::new(dist_dir);
//!     dev.start_watching().expect("Failed to start file watcher");
//!
//!     let app = Router::new()
//!         .route("/", get(index))
//!         .merge(dev.routes())   // adds /__dev/reload WebSocket endpoint
//!         .fallback({
//!             let dir = dev.watch_dir().to_path_buf();
//!             move |uri| dev_static_handler(dir.clone(), uri)
//!         });
//!
//!     // ...
//! }
//! ```
//!
//! Then run two terminals:
//!
//! ```bash
//! # Terminal 1: Watch and rebuild frontend WASM
//! cargo watch -w frontend/src -s "cargo build -p my-frontend --target wasm32-unknown-unknown && \
//!     wasm-bindgen target/wasm32-unknown-unknown/debug/my_frontend.wasm \
//!     --out-dir ./target/dev-dist --target web --no-typescript"
//!
//! # Terminal 2: Run the dev server
//! cargo run -p my-server --features dev
//! ```

use std::path::{Path, PathBuf};

use axum::{
    Router,
    body::Body,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::get,
};
use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use tokio::sync::broadcast;

// Debounce duration to avoid rapid consecutive reloads during multi-file writes.
const DEBOUNCE_MS: u64 = 200;

/// Development server that watches for file changes and notifies connected browsers.
///
/// The `DevServer` watches a directory (typically the frontend dist output) for
/// changes and sends reload notifications to connected browsers via WebSocket.
///
/// # Example
///
/// ```ignore
/// let dev = DevServer::new("./target/dev-dist");
/// dev.start_watching().unwrap();
///
/// let app = Router::new()
///     .merge(dev.routes())
///     .fallback(/* ... */);
/// ```
pub struct DevServer {
    watch_dir: PathBuf,
    tx: broadcast::Sender<()>,
}

impl DevServer {
    /// Create a new dev server that watches the given directory.
    ///
    /// The directory should point to where the frontend build outputs its
    /// artifacts (WASM, JS, HTML files).
    pub fn new(watch_dir: impl Into<PathBuf>) -> Self {
        let (tx, _) = broadcast::channel(16);
        Self {
            watch_dir: watch_dir.into(),
            tx,
        }
    }

    /// Get a reference to the watched directory path.
    pub fn watch_dir(&self) -> &Path {
        &self.watch_dir
    }

    /// Start watching for file changes in the configured directory.
    ///
    /// When a file change is detected, all connected browser clients will
    /// be notified to reload. Changes are debounced to avoid rapid reloads
    /// when multiple files are written in quick succession (e.g., wasm-bindgen
    /// writes both .wasm and .js files).
    ///
    /// # Errors
    ///
    /// Returns an error if the filesystem watcher cannot be created or if the
    /// watched directory does not exist.
    pub fn start_watching(&self) -> notify::Result<()> {
        let tx = self.tx.clone();
        let mut watcher =
            notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    // Only trigger reload on content changes, not metadata
                    use notify::EventKind;
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                            let _ = tx.send(());
                        }
                        _ => {}
                    }
                }
            })?;
        watcher.watch(&self.watch_dir, RecursiveMode::Recursive)?;
        // Keep the watcher alive for the lifetime of the process.
        // In a dev server context this is acceptable - the watcher should live
        // as long as the server is running.
        Box::leak(Box::new(watcher));
        Ok(())
    }

    /// Get a broadcast sender for manually triggering reloads.
    ///
    /// This can be useful for triggering a reload from custom build scripts
    /// or other tooling.
    pub fn reload_trigger(&self) -> broadcast::Sender<()> {
        self.tx.clone()
    }

    /// Create an axum Router with the hot-reload WebSocket endpoint.
    ///
    /// This adds a `/__dev/reload` route that browsers connect to for
    /// receiving reload notifications.
    pub fn routes(&self) -> Router {
        let tx = self.tx.clone();
        Router::new().route(
            "/__dev/reload",
            get(move |ws: WebSocketUpgrade| {
                let tx = tx.clone();
                async move { ws.on_upgrade(move |socket| handle_reload_ws(socket, tx)) }
            }),
        )
    }
}

/// Handle a single WebSocket connection for hot-reload notifications.
///
/// This keeps the connection open and sends a "reload" message whenever
/// a file change is detected. Includes debouncing to coalesce rapid changes.
async fn handle_reload_ws(mut socket: WebSocket, tx: broadcast::Sender<()>) {
    let mut rx = tx.subscribe();

    loop {
        match rx.recv().await {
            Ok(()) => {
                // Debounce: wait a short period and drain any additional notifications
                // that arrive during the window. This handles the common case where
                // wasm-bindgen writes multiple files in quick succession.
                tokio::time::sleep(std::time::Duration::from_millis(DEBOUNCE_MS)).await;
                // Drain any queued notifications
                while rx.try_recv().is_ok() {}

                if socket.send(Message::Text("reload".into())).await.is_err() {
                    // Client disconnected
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // Missed some notifications, send a reload anyway
                if socket.send(Message::Text("reload".into())).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Closed) => {
                break;
            }
        }
    }
}

/// JavaScript snippet that connects to the hot-reload WebSocket.
///
/// Inject this into your HTML during development to enable auto-refresh.
/// The script:
/// - Connects to `ws://{host}/__dev/reload`
/// - Reloads the page when a "reload" message is received
/// - Attempts to reconnect with exponential backoff if the connection drops
///   (e.g., when the server is restarting)
pub const HOT_RELOAD_SCRIPT: &str = r#"<script>
(function() {
    let reconnectDelay = 500;
    const maxDelay = 5000;

    function connect() {
        const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
        const ws = new WebSocket(protocol + '//' + location.host + '/__dev/reload');

        ws.onopen = function() {
            console.log('[axum-egui dev] Hot-reload connected');
            reconnectDelay = 500;
        };

        ws.onmessage = function(event) {
            if (event.data === 'reload') {
                console.log('[axum-egui dev] Reloading...');
                location.reload();
            }
        };

        ws.onclose = function() {
            console.log('[axum-egui dev] Connection lost, reconnecting in ' + reconnectDelay + 'ms');
            setTimeout(function() {
                reconnectDelay = Math.min(reconnectDelay * 2, maxDelay);
                connect();
            }, reconnectDelay);
        };

        ws.onerror = function() {
            ws.close();
        };
    }

    connect();
})();
</script>"#;

/// Serve static files from the filesystem for development.
///
/// Unlike `axum_egui::static_handler` which serves from embedded assets,
/// this handler reads files from disk on each request. This means changes
/// are picked up immediately without recompilation.
///
/// Falls back to serving `index.html` for unrecognized paths (SPA routing).
///
/// Uses blocking I/O on a Tokio blocking thread, which is appropriate for
/// development use where simplicity matters more than peak throughput.
///
/// # Example
///
/// ```ignore
/// use axum_egui::dev::dev_static_handler;
/// use std::path::PathBuf;
///
/// let dist = PathBuf::from("./target/dev-dist");
/// let app = Router::new()
///     .fallback(move |uri| dev_static_handler(dist.clone(), uri));
/// ```
pub async fn dev_static_handler(dir: PathBuf, uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/').to_string();
    let dir_clone = dir.clone();

    // Use spawn_blocking to avoid blocking the async runtime with filesystem I/O
    let result = tokio::task::spawn_blocking(move || {
        // Try the exact file first
        if !path.is_empty() {
            let file_path = dir_clone.join(&path);
            if let Ok(contents) = std::fs::read(&file_path) {
                let mime = mime_guess::from_path(&file_path).first_or_octet_stream();
                return Some((contents, mime.as_ref().to_string()));
            }
        }
        None
    })
    .await;

    if let Ok(Some((contents, mime))) = result {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            // Disable caching in dev mode so browsers always get fresh files
            .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
            .body(Body::from(contents))
            .unwrap();
    }

    // Fall back to index.html for SPA routing
    let index_result = tokio::task::spawn_blocking(move || {
        let index_path = dir.join("index.html");
        std::fs::read(&index_path).ok()
    })
    .await;

    match index_result {
        Ok(Some(contents)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html")
            .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
            .body(Body::from(contents))
            .unwrap(),
        _ => Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("404 Not Found (dev mode: no index.html found)"))
            .unwrap(),
    }
}

/// Development-mode App wrapper that injects hot-reload script into HTML.
///
/// This is the dev-mode equivalent of [`crate::App`]. Instead of reading
/// `index.html` from embedded assets, it reads from the filesystem and
/// injects both the initial state and the hot-reload client script.
///
/// # Example
///
/// ```ignore
/// use axum_egui::dev::DevApp;
///
/// async fn index() -> DevApp<MyState> {
///     DevApp::new(
///         MyState { counter: 42 },
///         "./target/dev-dist",
///     )
/// }
/// ```
pub struct DevApp<T> {
    state: T,
    dist_dir: PathBuf,
}

impl<T> DevApp<T> {
    /// Create a new DevApp response with the given state and dist directory.
    pub fn new(state: T, dist_dir: impl Into<PathBuf>) -> Self {
        Self {
            state,
            dist_dir: dist_dir.into(),
        }
    }
}

impl<T: Serialize> IntoResponse for DevApp<T> {
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

        // Read index.html from disk synchronously.
        // In dev mode this is acceptable since IntoResponse is not async,
        // and index.html is a small file read once per page load.
        let index_path = self.dist_dir.join("index.html");
        let html_str = match std::fs::read_to_string(&index_path) {
            Ok(html) => html,
            Err(e) => {
                return Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::from(format!(
                        "Failed to read index.html from {}: {e}\n\
                         Make sure the frontend has been built at least once.",
                        index_path.display()
                    )))
                    .unwrap();
            }
        };

        let state_script = format!(
            r#"<script id="axum-egui-state" type="application/json">{}</script>"#,
            state_json.replace("</", "<\\/")
        );

        // Inject state and hot-reload script
        let html = html_str
            .replace("<!--AXUM_EGUI_INITIAL_STATE-->", &state_script)
            .replace("</body>", &format!("{}\n</body>", HOT_RELOAD_SCRIPT));

        Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html")
            .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
            .body(Body::from(html))
            .unwrap()
    }
}

/// Convenience function to set up a complete dev-mode router.
///
/// This creates a `DevServer`, starts file watching, and returns a `Router`
/// with the hot-reload WebSocket endpoint and filesystem-based static serving.
///
/// # Arguments
///
/// * `dist_dir` - Path to the frontend dist directory (where wasm-bindgen outputs)
///
/// # Panics
///
/// Panics if the file watcher cannot be started.
///
/// # Example
///
/// ```ignore
/// use axum_egui::dev::dev_router;
///
/// let dev_routes = dev_router("./target/dev-dist");
///
/// let app = Router::new()
///     .route("/", get(index))
///     .route("/api/data", post(data_handler))
///     .merge(dev_routes);
/// ```
pub fn dev_router(dist_dir: impl Into<PathBuf>) -> Router {
    let dist_dir = dist_dir.into();
    let dev = DevServer::new(&dist_dir);
    dev.start_watching()
        .expect("Failed to start file watcher for dev mode");

    let fallback_dir = dist_dir.clone();
    Router::new()
        .merge(dev.routes())
        .fallback(move |uri: Uri| dev_static_handler(fallback_dir.clone(), uri))
}

/// Helper to create a dev-mode `Router` with an index route that injects state.
///
/// This is the highest-level convenience function. It sets up:
/// - File watching on the dist directory
/// - Hot-reload WebSocket endpoint at `/__dev/reload`
/// - An index route at `/` that serves `index.html` with state + hot-reload script
/// - A fallback that serves static files from the dist directory
///
/// # Type Parameters
///
/// * `T` - The state type to inject into the HTML
///
/// # Example
///
/// ```ignore
/// use axum_egui::dev::dev_app_router;
///
/// let dev_routes = dev_app_router("./target/dev-dist", MyState { counter: 42 });
///
/// let app = Router::new()
///     .route("/api/data", post(data_handler))
///     .merge(dev_routes);
/// ```
pub fn dev_app_router<T>(dist_dir: impl Into<PathBuf>, state: T) -> Router
where
    T: Serialize + Clone + Send + Sync + 'static,
{
    let dist_dir: PathBuf = dist_dir.into();
    let dev = DevServer::new(&dist_dir);
    dev.start_watching()
        .expect("Failed to start file watcher for dev mode");

    let index_dir = dist_dir.clone();
    let fallback_dir = dist_dir.clone();

    Router::new()
        .route(
            "/",
            get(move || async move { DevApp::new(state.clone(), index_dir.clone()) }),
        )
        .merge(dev.routes())
        .fallback(move |uri: Uri| dev_static_handler(fallback_dir.clone(), uri))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Uri;
    use http_body_util::BodyExt;
    use std::fs;
    use tempfile::TempDir;

    async fn body_to_string(response: Response) -> String {
        let body = response.into_body();
        let bytes = body.collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn dev_static_handler_serves_files() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("test.js"), "console.log('hello')").unwrap();

        let uri: Uri = "/test.js".parse().unwrap();
        let response = dev_static_handler(tmp.path().to_path_buf(), uri).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "text/javascript"
        );
        let body = body_to_string(response).await;
        assert_eq!(body, "console.log('hello')");
    }

    #[tokio::test]
    async fn dev_static_handler_falls_back_to_index() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("index.html"), "<html>Hello</html>").unwrap();

        let uri: Uri = "/nonexistent".parse().unwrap();
        let response = dev_static_handler(tmp.path().to_path_buf(), uri).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("content-type").unwrap(), "text/html");
        let body = body_to_string(response).await;
        assert_eq!(body, "<html>Hello</html>");
    }

    #[tokio::test]
    async fn dev_static_handler_returns_404_when_no_index() {
        let tmp = TempDir::new().unwrap();

        let uri: Uri = "/nonexistent".parse().unwrap();
        let response = dev_static_handler(tmp.path().to_path_buf(), uri).await;

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn dev_static_handler_sets_no_cache_headers() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("app.js"), "var x = 1;").unwrap();

        let uri: Uri = "/app.js".parse().unwrap();
        let response = dev_static_handler(tmp.path().to_path_buf(), uri).await;

        assert_eq!(
            response.headers().get("cache-control").unwrap(),
            "no-cache, no-store, must-revalidate"
        );
    }

    #[tokio::test]
    async fn dev_app_injects_state_and_reload_script() {
        let tmp = TempDir::new().unwrap();
        fs::write(
            tmp.path().join("index.html"),
            "<!DOCTYPE html><html><body><!--AXUM_EGUI_INITIAL_STATE--></body></html>",
        )
        .unwrap();

        #[derive(serde::Serialize, Clone)]
        struct TestState {
            value: i32,
        }

        let app = DevApp::new(TestState { value: 99 }, tmp.path());
        let response = app.into_response();

        assert_eq!(response.status(), StatusCode::OK);
        let body = body_to_string(response).await;

        // Should contain state
        assert!(body.contains(r#""value":99"#));
        assert!(body.contains(r#"<script id="axum-egui-state""#));

        // Should contain hot-reload script
        assert!(body.contains("/__dev/reload"));
        assert!(body.contains("location.reload()"));

        // Should not contain the placeholder
        assert!(!body.contains("<!--AXUM_EGUI_INITIAL_STATE-->"));
    }

    #[test]
    fn dev_server_creates_successfully() {
        let dev = DevServer::new("/tmp/test");
        assert_eq!(dev.watch_dir(), Path::new("/tmp/test"));
    }

    #[tokio::test]
    async fn dev_server_watching_nonexistent_dir_returns_error() {
        let dev = DevServer::new("/tmp/nonexistent_axum_egui_test_dir_12345");
        let result = dev.start_watching();
        assert!(result.is_err());
    }

    #[test]
    fn hot_reload_script_is_valid_html() {
        assert!(HOT_RELOAD_SCRIPT.starts_with("<script>"));
        assert!(HOT_RELOAD_SCRIPT.ends_with("</script>"));
        assert!(HOT_RELOAD_SCRIPT.contains("__dev/reload"));
    }

    #[tokio::test]
    async fn dev_server_routes_creates_router() {
        let dev = DevServer::new("/tmp");
        let _router = dev.routes();
        // Router creation should not panic
    }

    #[test]
    fn reload_trigger_can_send() {
        let dev = DevServer::new("/tmp");
        let tx = dev.reload_trigger();
        // Should not panic even with no receivers
        let _ = tx.send(());
    }
}
