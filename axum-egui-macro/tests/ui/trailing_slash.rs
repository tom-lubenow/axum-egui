//! Test that trailing slashes in paths are rejected.

use axum_egui_macro::server;

#[server("/api/trailing/")]
pub async fn trailing() -> Result<(), ServerFnError> {
    Ok(())
}

fn main() {}

// Stub type for the test
pub struct ServerFnError;
