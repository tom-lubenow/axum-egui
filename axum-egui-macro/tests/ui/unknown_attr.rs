//! Test that unknown server function attributes are rejected.

use axum_egui_macro::server;

#[server(foobar)]
pub async fn bad_attr() -> Result<(), ServerFnError> {
    Ok(())
}

fn main() {}

// Stub type for the test
pub struct ServerFnError;
