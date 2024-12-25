use axum::Router;
use std::net::SocketAddr;
use tokio::net::TcpListener;

mod app;
use app::SimpleApp;

#[tokio::main]
async fn main() {
    // Create our handler
    let handler = axum_egui::AxumEguiHandler::new(SimpleApp::default());
    
    // Create the router
    let app = handler.router();
    
    // Bind and serve
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Listening on {addr}");
    
    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
} 