//! Test that double slashes in paths are rejected.

use axum_egui_macro::server;

#[server("/api//double")]
pub async fn double_slash() -> Result<(), ServerFnError> {
    Ok(())
}

fn main() {}

// Stub type for the test
pub struct ServerFnError;
