use axum::{
    response::{IntoResponse, Response},
    routing::{get, MethodRouter},
    Router,
};
use eframe;
use std::{env, fs, path::PathBuf};

/// Core trait that provides the integration between axum and egui.
/// This trait is typically derived using the `AxumEguiApp` derive macro.
pub trait AxumEguiApp: Sized {
    /// The egui App type that this axum integration serves
    type App: eframe::App;
    
    /// Creates a new instance of the egui app
    fn create_app() -> Self::App;
    
    /// Returns the router that serves this app
    fn router() -> Router;
    
    /// Returns the fallback handler for serving static files
    fn fallback() -> MethodRouter;
}

pub struct AxumEguiHandler<A: 'static> {
    _app: A, // Mark as intentionally unused with underscore
}

impl<A: 'static> AxumEguiHandler<A> {
    pub fn new(app: A) -> Self {
        Self { _app: app }
    }

    pub fn router(&self) -> Router {
        Router::new()
            .route("/", get(Self::serve_static_file))
            .fallback(get(Self::serve_static_file))
    }

    pub async fn serve_static_file(uri: axum::http::Uri) -> Response {
        let path = uri.path().trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };

        // Get the file from the assets directory
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let file_path = manifest_dir.join("assets/dist").join(path);
        
        if !file_path.exists() {
            return (axum::http::StatusCode::NOT_FOUND, format!("File not found: {}", file_path.display())).into_response();
        }

        let contents = fs::read(&file_path).unwrap();
        let content_type = match path.split('.').last() {
            Some("html") => "text/html",
            Some("js") => "application/javascript",
            Some("wasm") => "application/wasm",
            _ => "application/octet-stream",
        };

        ([(axum::http::header::CONTENT_TYPE, content_type)], contents).into_response()
    }
} 