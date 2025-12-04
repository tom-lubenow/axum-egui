//! Request context and dependency injection for server functions.
//!
//! Provides access to HTTP request details (headers, cookies, IP) and
//! custom context values within server functions.
//!
//! # Request Context
//!
//! ```ignore
//! use server_fn::prelude::*;
//!
//! #[server]
//! pub async fn whoami() -> Result<String, ServerFnError> {
//!     let ctx = request_context();
//!     let user_agent = ctx.header("user-agent").unwrap_or("unknown");
//!     Ok(format!("Your browser: {}", user_agent))
//! }
//! ```
//!
//! # Custom Context (Extractors)
//!
//! You can provide custom context values (like database connections, config, etc.)
//! and access them in server functions using `use_context<T>()`:
//!
//! ```ignore
//! use server_fn::prelude::*;
//!
//! // In your router setup:
//! let app = Router::new()
//!     .route("/api/get_user", post(get_user))
//!     .layer(Extension(db_pool.clone()));
//!
//! // In your server function:
//! #[server]
//! pub async fn get_user(id: i32) -> Result<User, ServerFnError> {
//!     let pool: DbPool = use_context()?;
//!     // Use pool...
//! }
//! ```

#[cfg(not(target_arch = "wasm32"))]
mod server {
    use axum::http::HeaderMap;
    use std::any::{Any, TypeId};
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::net::IpAddr;
    use std::sync::Arc;

    use crate::ServerFnError;

    /// Request context available within server functions.
    ///
    /// This provides access to HTTP request details like headers, cookies,
    /// and client IP address.
    #[derive(Debug, Clone, Default)]
    pub struct RequestContext {
        headers: HeaderMap,
        cookies: HashMap<String, String>,
        client_ip: Option<IpAddr>,
        path: String,
        query: Option<String>,
    }

    impl RequestContext {
        /// Create a new request context.
        pub fn new() -> Self {
            Self::default()
        }

        /// Create context from axum request parts.
        pub fn from_parts(
            headers: HeaderMap,
            client_ip: Option<IpAddr>,
            path: String,
            query: Option<String>,
        ) -> Self {
            // Parse cookies from the Cookie header
            let cookies = headers
                .get("cookie")
                .and_then(|v| v.to_str().ok())
                .map(|cookie_str| {
                    cookie_str
                        .split(';')
                        .filter_map(|pair| {
                            let mut parts = pair.trim().splitn(2, '=');
                            let key = parts.next()?.to_string();
                            let value = parts.next()?.to_string();
                            Some((key, value))
                        })
                        .collect()
                })
                .unwrap_or_default();

            Self {
                headers,
                cookies,
                client_ip,
                path,
                query,
            }
        }

        /// Get a header value by name (case-insensitive).
        pub fn header(&self, name: &str) -> Option<&str> {
            self.headers.get(name).and_then(|v| v.to_str().ok())
        }

        /// Get all headers.
        pub fn headers(&self) -> &HeaderMap {
            &self.headers
        }

        /// Get a cookie value by name.
        pub fn cookie(&self, name: &str) -> Option<&str> {
            self.cookies.get(name).map(|s| s.as_str())
        }

        /// Get all cookies.
        pub fn cookies(&self) -> &HashMap<String, String> {
            &self.cookies
        }

        /// Get the client IP address.
        ///
        /// This checks common proxy headers (X-Forwarded-For, X-Real-IP)
        /// before falling back to the direct connection IP.
        pub fn client_ip(&self) -> Option<IpAddr> {
            // Check X-Forwarded-For first (common with proxies)
            if let Some(forwarded) = self.header("x-forwarded-for") {
                if let Some(first_ip) = forwarded.split(',').next() {
                    if let Ok(ip) = first_ip.trim().parse() {
                        return Some(ip);
                    }
                }
            }

            // Check X-Real-IP
            if let Some(real_ip) = self.header("x-real-ip") {
                if let Ok(ip) = real_ip.trim().parse() {
                    return Some(ip);
                }
            }

            // Fall back to direct connection IP
            self.client_ip
        }

        /// Get the raw connection IP (without checking proxy headers).
        pub fn connection_ip(&self) -> Option<IpAddr> {
            self.client_ip
        }

        /// Get the request path.
        pub fn path(&self) -> &str {
            &self.path
        }

        /// Get the query string (without the leading ?).
        pub fn query(&self) -> Option<&str> {
            self.query.as_deref()
        }

        /// Check if the request appears to be from a secure connection.
        pub fn is_secure(&self) -> bool {
            // Check X-Forwarded-Proto header (set by proxies)
            if let Some(proto) = self.header("x-forwarded-proto") {
                return proto.eq_ignore_ascii_case("https");
            }
            false
        }

        /// Get the Authorization header value.
        pub fn authorization(&self) -> Option<&str> {
            self.header("authorization")
        }

        /// Get bearer token from Authorization header.
        pub fn bearer_token(&self) -> Option<&str> {
            self.authorization()
                .filter(|auth| auth.starts_with("Bearer "))
                .map(|auth| &auth[7..])
        }
    }

    // Task-local storage for request context
    tokio::task_local! {
        static REQUEST_CONTEXT: RefCell<Option<RequestContext>>;
    }

    /// Get the current request context.
    ///
    /// # Panics
    ///
    /// Panics if called outside of a server function context.
    /// This should only be called within the body of a `#[server]` function.
    pub fn request_context() -> RequestContext {
        REQUEST_CONTEXT.with(|ctx| {
            ctx.borrow()
                .clone()
                .expect("request_context() called outside of server function context")
        })
    }

    /// Try to get the current request context.
    ///
    /// Returns `None` if called outside of a server function context.
    pub fn try_request_context() -> Option<RequestContext> {
        REQUEST_CONTEXT
            .try_with(|ctx| ctx.borrow().clone())
            .ok()
            .flatten()
    }

    /// Run a future with the given request context.
    ///
    /// This is used internally by the `#[server]` macro to set up the context.
    pub async fn with_context<F, T>(ctx: RequestContext, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        REQUEST_CONTEXT
            .scope(RefCell::new(Some(ctx)), f)
            .await
    }

    // ========================================================================
    // Custom Context (Extractors / Dependency Injection)
    // ========================================================================

    /// Type-erased storage for custom context values.
    type TypeMap = HashMap<TypeId, Arc<dyn Any + Send + Sync>>;

    // Task-local storage for custom context values
    tokio::task_local! {
        static CUSTOM_CONTEXT: RefCell<TypeMap>;
    }

    /// Provide a value in the current context.
    ///
    /// This makes the value available to `use_context::<T>()` calls within
    /// the current server function.
    ///
    /// # Example
    ///
    /// ```ignore
    /// // In a middleware or custom extractor:
    /// provide_context(my_database_pool.clone());
    ///
    /// // Later, in a server function:
    /// let pool: DbPool = use_context()?;
    /// ```
    pub fn provide_context<T: Clone + Send + Sync + 'static>(value: T) {
        let _ = CUSTOM_CONTEXT.try_with(|map| {
            map.borrow_mut().insert(TypeId::of::<T>(), Arc::new(value));
        });
    }

    /// Get a value from the current context.
    ///
    /// Returns `Err(ServerFnError::Custom)` if the value was not provided.
    ///
    /// # Example
    ///
    /// ```ignore
    /// #[server]
    /// pub async fn get_user(id: i32) -> Result<User, ServerFnError> {
    ///     let pool: DbPool = use_context()?;
    ///     // Use pool to query user...
    /// }
    /// ```
    pub fn use_context<T: Clone + Send + Sync + 'static>() -> Result<T, ServerFnError> {
        CUSTOM_CONTEXT
            .try_with(|map| {
                map.borrow()
                    .get(&TypeId::of::<T>())
                    .and_then(|arc| arc.downcast_ref::<T>())
                    .cloned()
            })
            .ok()
            .flatten()
            .ok_or_else(|| {
                ServerFnError::Custom(format!(
                    "Context value not found for type: {}",
                    std::any::type_name::<T>()
                ))
            })
    }

    /// Try to get a value from the current context.
    ///
    /// Returns `None` if the value was not provided, instead of an error.
    pub fn try_use_context<T: Clone + Send + Sync + 'static>() -> Option<T> {
        CUSTOM_CONTEXT
            .try_with(|map| {
                map.borrow()
                    .get(&TypeId::of::<T>())
                    .and_then(|arc| arc.downcast_ref::<T>())
                    .cloned()
            })
            .ok()
            .flatten()
    }

    /// Run a future with custom context values.
    ///
    /// This is used internally by the `#[server]` macro to set up context
    /// from axum Extensions.
    pub async fn with_custom_context<F, T>(f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        CUSTOM_CONTEXT
            .scope(RefCell::new(HashMap::new()), f)
            .await
    }

    /// Run a future with both request context and custom context.
    ///
    /// This combines `with_context` and `with_custom_context` for convenience.
    pub async fn with_full_context<F, T>(ctx: RequestContext, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        REQUEST_CONTEXT
            .scope(RefCell::new(Some(ctx)), async {
                CUSTOM_CONTEXT
                    .scope(RefCell::new(HashMap::new()), f)
                    .await
            })
            .await
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use server::*;

// On WASM, provide stub types that won't be used but allow code to compile
#[cfg(target_arch = "wasm32")]
mod client {
    use std::collections::HashMap;
    use std::net::IpAddr;

    /// Stub request context for WASM (not used on client).
    #[derive(Debug, Clone, Default)]
    pub struct RequestContext;

    impl RequestContext {
        pub fn header(&self, _name: &str) -> Option<&str> {
            None
        }
        pub fn cookie(&self, _name: &str) -> Option<&str> {
            None
        }
        pub fn cookies(&self) -> HashMap<String, String> {
            HashMap::new()
        }
        pub fn client_ip(&self) -> Option<IpAddr> {
            None
        }
        pub fn path(&self) -> &str {
            ""
        }
        pub fn query(&self) -> Option<&str> {
            None
        }
        pub fn is_secure(&self) -> bool {
            false
        }
        pub fn authorization(&self) -> Option<&str> {
            None
        }
        pub fn bearer_token(&self) -> Option<&str> {
            None
        }
    }

    /// Stub - always returns empty context on WASM.
    pub fn request_context() -> RequestContext {
        RequestContext
    }

    /// Stub - always returns None on WASM.
    pub fn try_request_context() -> Option<RequestContext> {
        None
    }

    /// Stub - always returns error on WASM.
    pub fn use_context<T>() -> Result<T, crate::ServerFnError> {
        Err(crate::ServerFnError::Custom(
            "use_context is not available on WASM".to_string(),
        ))
    }

    /// Stub - always returns None on WASM.
    pub fn try_use_context<T>() -> Option<T> {
        None
    }
}

#[cfg(target_arch = "wasm32")]
pub use client::*;
