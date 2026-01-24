//! Test that non-Result return types are rejected.

use axum_egui_macro::server;

#[server]
pub async fn bad_return() -> String {
    "oops".into()
}

fn main() {}
