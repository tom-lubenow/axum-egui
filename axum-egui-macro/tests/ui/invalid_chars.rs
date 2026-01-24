//! Test that invalid characters in paths are rejected.

use axum_egui_macro::server;

#[server("/api/bad@path")]
pub async fn bad_chars() -> Result<(), ServerFnError> {
    Ok(())
}

fn main() {}

// Stub type for the test
pub struct ServerFnError;
