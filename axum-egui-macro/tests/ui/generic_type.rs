//! Test that generic type parameters are rejected with a helpful message.

use axum_egui_macro::server;

#[server]
pub async fn generic_fn<T>(value: T) -> Result<T, ServerFnError> {
    Ok(value)
}

fn main() {}

// Stub type for the test
pub struct ServerFnError;
