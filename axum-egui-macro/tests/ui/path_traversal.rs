//! Test that path traversal in API paths is rejected.

use axum_egui_macro::server;

#[server("/api/../../dangerous")]
pub async fn bad_path() -> Result<(), ServerFnError> {
    Ok(())
}

fn main() {}

// Stub type for the test
pub struct ServerFnError;
