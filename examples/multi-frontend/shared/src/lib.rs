//! Shared server functions for multi-frontend example.
//!
//! Note: App state types are defined in each frontend crate to satisfy
//! Rust's orphan rules (they need to implement eframe::App locally).

use server_fn::prelude::*;

/// Get the current server time.
#[server]
pub async fn get_server_time() -> Result<u64, ServerFnError> {
    #[cfg(feature = "server")]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(now)
    }
    #[cfg(not(feature = "server"))]
    unreachable!()
}

/// Increment a counter value.
#[server]
pub async fn increment_counter(current: i32) -> Result<i32, ServerFnError> {
    Ok(current + 1)
}
