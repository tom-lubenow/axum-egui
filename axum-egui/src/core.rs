use axum::{
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Router,
};
use eframe::App;
use include_dir::{Dir, File};

/// Handler for serving an egui app through axum
pub struct AxumEguiHandler<A>
where
    A: App + 'static,
{
    app: A,
}

impl<A> AxumEguiHandler<A>
where
    A: App + 'static,
{
    /// Create a new handler for the given app
    pub fn new(app: A) -> Self {
        Self { app }
    }

    /// Create the router for serving this app
    pub fn router(self) -> Router {
        Router::new()
            .fallback(Self::serve_static_file)
    }

    /// Serve a static file from the embedded assets
    async fn serve_static_file(uri: axum::http::Uri) -> Response {
        let path = uri.path().trim_start_matches('/');
        let path = if path.is_empty() { "index.html" } else { path };

        match crate::assets::ASSETS.get_file(path) {
            Some(file) => Self::serve_file(file),
            None => (StatusCode::NOT_FOUND, "404 Not Found").into_response(),
        }
    }

    /// Serve a file with the appropriate content type
    fn serve_file(file: &'static File<'static>) -> Response {
        let content_type = match file.path().extension().and_then(|e| e.to_str()) {
            Some("html") => "text/html",
            Some("js") => "application/javascript",
            Some("wasm") => "application/wasm",
            _ => "application/octet-stream",
        };

        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, content_type)],
            file.contents(),
        ).into_response()
    }
} 