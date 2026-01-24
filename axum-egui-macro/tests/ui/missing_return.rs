//! Test that functions without return types are rejected.

use axum_egui_macro::server;

#[server]
pub async fn no_return() {
    println!("no return type");
}

fn main() {}
