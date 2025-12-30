//! Server function registry for automatic route registration.
//!
//! This module provides infrastructure for automatically registering server functions
//! at compile time using the `inventory` crate.
//!
//! # Usage
//!
//! Server functions marked with `#[server]` automatically register themselves.
//! To add them all to your axum router:
//!
//! ```ignore
//! use axum::Router;
//! use server_fn::registry::ServerFnRouter;
//!
//! let app = Router::new()
//!     .register_server_fns()
//!     .fallback(static_handler::<Assets>);
//! ```

use axum::http::Method;
use axum::routing::MethodRouter;

/// A registered server function.
///
/// This struct holds the metadata needed to register a server function
/// as an axum route.
pub struct ServerFunction {
    /// HTTP method (GET, POST, etc.)
    pub method: Method,
    /// URL path (e.g., "/api/greet")
    pub path: &'static str,
    /// Factory function that creates the axum MethodRouter
    pub handler_factory: fn() -> MethodRouter,
}

impl ServerFunction {
    /// Create a new server function registration.
    pub const fn new(
        method: Method,
        path: &'static str,
        handler_factory: fn() -> MethodRouter,
    ) -> Self {
        Self {
            method,
            path,
            handler_factory,
        }
    }

    /// Get all registered server functions.
    pub fn collect() -> impl Iterator<Item = &'static ServerFunction> {
        inventory::iter::<ServerFunction>()
    }
}

// Register the ServerFunction type with inventory
inventory::collect!(ServerFunction);

/// Extension trait for axum Router to add all registered server functions.
pub trait ServerFnRouter {
    /// Register all server functions that have been marked with `#[server]`.
    ///
    /// This method iterates over all server functions collected by the `inventory`
    /// crate and adds them to the router with their configured paths and methods.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use axum::Router;
    /// use server_fn::registry::ServerFnRouter;
    ///
    /// let app = Router::new()
    ///     .register_server_fns()
    ///     .fallback(static_handler::<Assets>);
    /// ```
    fn register_server_fns(self) -> Self;
}

// Implement for Router<()> since server functions use their own context mechanism
// rather than axum's state pattern.
impl ServerFnRouter for axum::Router<()> {
    fn register_server_fns(self) -> Self {
        let mut router = self;
        for func in ServerFunction::collect() {
            let method_router = (func.handler_factory)();
            router = router.route(func.path, method_router);
            tracing::debug!("Registered server function: {} {}", func.method, func.path);
        }
        router
    }
}
