use axum_egui::AxumEguiApp;
use std::{marker::PhantomData, net::SocketAddr};
use tokio::net::TcpListener;

pub mod gui;

#[derive(AxumEguiApp)]
struct TodoApp<T = gui::App>(PhantomData<T>);

#[tokio::main]
async fn main() {
    // Create the router using our derived implementation
    let app = TodoApp::<gui::App>::router();

    // Bind and serve
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Listening on {addr}");

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
