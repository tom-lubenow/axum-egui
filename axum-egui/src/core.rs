use axum::{
    extract::{
        ws::{WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
    routing::{get, get_service},
    Router,
};
use eframe::App;
use futures_util::StreamExt;
use include_dir::Dir;
use std::sync::Arc;
use tower_http::services::ServeDir;

/// Handler for serving an egui app through axum
pub struct AxumEguiHandler<A: App + Send + Sync + 'static> {
    app: Arc<A>,
    assets_dir: Dir<'static>,
}

impl<A: App + Send + Sync + 'static> AxumEguiHandler<A> {
    /// Create a new handler for the given app and assets directory
    pub fn new(app: A, assets_dir: Dir<'static>) -> Self {
        Self {
            app: Arc::new(app),
            assets_dir,
        }
    }

    /// Create the router for serving this app
    pub fn router(self) -> Router {
        let app = self.app.clone();
        Router::new()
            .route("/ws", get(Self::handle_websocket))
            .with_state(app)
            .fallback_service(get_service(ServeDir::new("/")))
    }

    /// Handle WebSocket connections
    async fn handle_websocket(State(app): State<Arc<A>>, ws: WebSocketUpgrade) -> Response {
        ws.on_upgrade(move |socket| Self::handle_socket(app, socket))
    }

    /// Handle an individual WebSocket connection
    async fn handle_socket(app: Arc<A>, socket: WebSocket) {
        // TODO: Implement WebSocket handling
        let (_tx, _rx) = socket.split();
    }
}
