//! Test that self parameters are rejected.

use axum_egui_macro::server;

struct MyService;

impl MyService {
    #[server]
    pub async fn with_self(&self) -> Result<(), ServerFnError> {
        Ok(())
    }
}

fn main() {}

// Stub type for the test
pub struct ServerFnError;
