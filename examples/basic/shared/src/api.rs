//! Server functions - defined once, work on both server and web.
//!
//! These functions are marked with `#[server]` which generates:
//! - Server: axum handlers that execute the function body
//! - Web: async functions that make HTTP calls to the server

use serde::{Deserialize, Serialize};
use server_fn::prelude::*;

// ============================================================================
// RPC Functions
// ============================================================================

/// Simple addition function.
#[server]
pub async fn add(a: i32, b: i32) -> Result<i32, ServerFnError> {
    #[cfg(feature = "server")]
    println!("Server: adding {} + {}", a, b);
    Ok(a + b)
}

/// Greeting function.
#[server]
pub async fn greet(name: String) -> Result<String, ServerFnError> {
    #[cfg(feature = "server")]
    println!("Server: greeting {}", name);
    Ok(format!("Hello, {}! Greetings from the server.", name))
}

/// Response type for whoami endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhoamiResponse {
    pub user_agent: String,
    pub ip: String,
    pub path: String,
}

/// Server function that demonstrates request context access.
#[server]
pub async fn whoami() -> Result<WhoamiResponse, ServerFnError> {
    #[cfg(feature = "server")]
    {
        let ctx = request_context();
        let user_agent = ctx.header("user-agent").unwrap_or("unknown").to_string();
        let ip = ctx
            .client_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let path = ctx.path().to_string();

        println!("Server: whoami from {} ({})", ip, user_agent);

        Ok(WhoamiResponse {
            user_agent,
            ip,
            path,
        })
    }

    #[cfg(not(feature = "server"))]
    unreachable!()
}

// ============================================================================
// SSE Streaming
// ============================================================================

/// Tick event with timestamp
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickEvent {
    pub count: i32,
    pub timestamp: u64,
}

/// A counter that streams every second.
#[server(sse)]
pub async fn counter() -> impl Stream<Item = i32> {
    use std::time::Duration;
    async_stream::stream! {
        for i in 0..100 {
            tokio::time::sleep(Duration::from_secs(1)).await;
            println!("SSE: emitting {}", i);
            yield i;
        }
    }
}

/// A more complex stream that emits structured events.
#[server(sse)]
pub async fn tick_stream() -> impl Stream<Item = TickEvent> {
    use std::time::Duration;
    async_stream::stream! {
        let start = std::time::Instant::now();
        for count in 0..100 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            yield TickEvent {
                count,
                timestamp: start.elapsed().as_millis() as u64,
            };
        }
    }
}

// ============================================================================
// WebSocket
// ============================================================================

/// Echo WebSocket - sends back any message received with "Echo: " prefix.
#[server(ws)]
pub async fn echo(incoming: impl Stream<Item = String>) -> impl Stream<Item = String> {
    use futures::StreamExt;
    incoming.map(|msg| {
        println!("WS: received {}", msg);
        format!("Echo: {}", msg)
    })
}
