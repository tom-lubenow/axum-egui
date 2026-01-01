//! Shared API types and client functions.
//!
//! This module defines the request/response types for server endpoints.
//! When the `web` feature is enabled, it also provides client functions
//! that use fetch to call the server.

use serde::{Deserialize, Serialize};

// ============================================================================
// Error Type
// ============================================================================

/// API error type for client-side calls.
#[derive(Debug, Clone)]
pub struct ApiError(pub String);

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ApiError {}

// ============================================================================
// API Types
// ============================================================================

/// Request for the add endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddRequest {
    pub a: i32,
    pub b: i32,
}

/// Response from the add endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddResponse {
    pub result: i32,
}

/// Request for the greet endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GreetRequest {
    pub name: String,
}

/// Response from the greet endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GreetResponse {
    pub message: String,
}

/// Response from the whoami endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhoamiResponse {
    pub message: String,
    pub timestamp: u64,
}

// ============================================================================
// Client Functions (web feature only)
// ============================================================================

#[cfg(feature = "web")]
mod client {
    use super::*;
    use gloo_net::http::Request;

    /// Call the greet endpoint.
    pub async fn greet(name: String) -> Result<String, ApiError> {
        let request = GreetRequest { name };
        let response: GreetResponse = Request::post("/api/greet")
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&request).map_err(|e| ApiError(e.to_string()))?)
            .map_err(|e| ApiError(e.to_string()))?
            .send()
            .await
            .map_err(|e| ApiError(e.to_string()))?
            .json()
            .await
            .map_err(|e| ApiError(e.to_string()))?;
        Ok(response.message)
    }

    /// Call the add endpoint.
    pub async fn add(a: i32, b: i32) -> Result<i32, ApiError> {
        let request = AddRequest { a, b };
        let response: AddResponse = Request::post("/api/add")
            .header("Content-Type", "application/json")
            .body(serde_json::to_string(&request).map_err(|e| ApiError(e.to_string()))?)
            .map_err(|e| ApiError(e.to_string()))?
            .send()
            .await
            .map_err(|e| ApiError(e.to_string()))?
            .json()
            .await
            .map_err(|e| ApiError(e.to_string()))?;
        Ok(response.result)
    }

    /// Call the whoami endpoint.
    pub async fn whoami() -> Result<WhoamiResponse, ApiError> {
        let response: WhoamiResponse = Request::get("/api/whoami")
            .send()
            .await
            .map_err(|e| ApiError(e.to_string()))?
            .json()
            .await
            .map_err(|e| ApiError(e.to_string()))?;
        Ok(response)
    }
}

#[cfg(feature = "web")]
pub use client::*;
