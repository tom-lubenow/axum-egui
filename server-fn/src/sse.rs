//! Server-Sent Events (SSE) support.
//!
//! This module provides types for both server and client sides of SSE connections.
//!
//! # Server Usage
//! ```ignore
//! #[server(sse)]
//! pub async fn counter() -> impl Stream<Item = i32> {
//!     async_stream::stream! {
//!         for i in 0..10 {
//!             tokio::time::sleep(Duration::from_secs(1)).await;
//!             yield i;
//!         }
//!     }
//! }
//! ```
//!
//! # Client Usage
//! ```ignore
//! let stream = counter();
//! // In your egui update loop:
//! for event in stream.try_iter() {
//!     // Handle event
//! }
//! ```

/// Configuration for automatic reconnection with exponential backoff.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Base delay in milliseconds before first reconnect attempt.
    /// Default: 500ms
    pub base_delay_ms: u32,

    /// Maximum delay in milliseconds between reconnect attempts.
    /// Default: 30000ms (30 seconds)
    pub max_delay_ms: u32,

    /// Jitter factor (0.0 to 1.0) to randomize delays and prevent thundering herd.
    /// Default: 0.3
    pub jitter_factor: f32,

    /// Maximum number of reconnection attempts. None means infinite.
    /// Default: None
    pub max_attempts: Option<u32>,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            base_delay_ms: 500,
            max_delay_ms: 30_000,
            jitter_factor: 0.3,
            max_attempts: None,
        }
    }
}

impl ReconnectConfig {
    /// Create a new config with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the base delay.
    pub fn base_delay(mut self, ms: u32) -> Self {
        self.base_delay_ms = ms;
        self
    }

    /// Set the maximum delay.
    pub fn max_delay(mut self, ms: u32) -> Self {
        self.max_delay_ms = ms;
        self
    }

    /// Set the jitter factor.
    pub fn jitter(mut self, factor: f32) -> Self {
        self.jitter_factor = factor.clamp(0.0, 1.0);
        self
    }

    /// Set maximum reconnection attempts.
    pub fn max_attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = Some(attempts);
        self
    }

    /// Calculate delay for a given attempt number using exponential backoff with jitter.
    pub fn delay_for_attempt(&self, attempt: u32) -> u32 {
        let base = self.base_delay_ms as f64;
        let exponential = base * 2_f64.powi(attempt as i32);
        let capped = exponential.min(self.max_delay_ms as f64);

        // Add jitter
        let jitter_range = capped * self.jitter_factor as f64;
        let jitter = random_f64() * jitter_range;

        (capped + jitter) as u32
    }
}

/// Platform-specific random number generation
#[cfg(target_arch = "wasm32")]
fn random_f64() -> f64 {
    js_sys::Math::random()
}

#[cfg(not(target_arch = "wasm32"))]
fn random_f64() -> f64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let state = RandomState::new();
    let mut hasher = state.build_hasher();
    hasher.write_u64(std::time::Instant::now().elapsed().as_nanos() as u64);
    (hasher.finish() as f64) / (u64::MAX as f64)
}

// ============================================================================
// Client-side SSE Stream (WASM only)
// ============================================================================

#[cfg(target_arch = "wasm32")]
mod client {
    use super::*;
    use gloo_net::eventsource::futures::EventSource;
    use std::marker::PhantomData;
    use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
    use wasm_bindgen_futures::spawn_local;

    /// Connection state for SSE stream.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ConnectionState {
        Connecting,
        Connected,
        Reconnecting { attempt: u32 },
        Disconnected,
        Failed,
    }

    /// A handle to an SSE stream that can be polled in egui's update loop.
    ///
    /// This type manages the underlying EventSource connection, handles automatic
    /// reconnection, and provides a non-blocking interface for receiving events.
    pub struct SseStream<T> {
        /// Receiver for events from the background task
        rx: Receiver<T>,
        /// Receiver for connection state updates
        state_rx: Receiver<ConnectionState>,
        /// Current connection state (cached)
        state: ConnectionState,
        /// Marker for the event type
        _marker: PhantomData<T>,
    }

    impl<T> std::fmt::Debug for SseStream<T> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SseStream")
                .field("state", &self.state)
                .finish_non_exhaustive()
        }
    }

    impl<T> SseStream<T>
    where
        T: serde::de::DeserializeOwned + 'static,
    {
        /// Create a new SSE stream connecting to the given endpoint.
        ///
        /// The stream will automatically connect and handle reconnection.
        pub fn connect(endpoint: &str) -> Self {
            Self::connect_with_config(endpoint, ReconnectConfig::default())
        }

        /// Create a new SSE stream with custom reconnection configuration.
        pub fn connect_with_config(endpoint: &str, config: ReconnectConfig) -> Self {
            let (tx, rx) = channel();
            let (state_tx, state_rx) = channel();
            let endpoint = endpoint.to_string();

            // Spawn the connection manager
            spawn_local(async move {
                connection_loop::<T>(endpoint, config, tx, state_tx).await;
            });

            Self {
                rx,
                state_rx,
                state: ConnectionState::Connecting,
                _marker: PhantomData,
            }
        }

        /// Try to receive the next event without blocking.
        ///
        /// Returns `Some(event)` if an event is available, `None` otherwise.
        pub fn try_recv(&self) -> Option<T> {
            match self.rx.try_recv() {
                Ok(event) => Some(event),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => None,
            }
        }

        /// Returns an iterator over all currently available events.
        ///
        /// This is useful for processing all pending events in an egui update loop:
        /// ```ignore
        /// for event in stream.try_iter() {
        ///     // handle event
        /// }
        /// ```
        pub fn try_iter(&self) -> impl Iterator<Item = T> + '_ {
            std::iter::from_fn(|| self.try_recv())
        }

        /// Get the current connection state.
        pub fn state(&mut self) -> ConnectionState {
            // Update cached state from state channel
            while let Ok(new_state) = self.state_rx.try_recv() {
                self.state = new_state;
            }
            self.state
        }

        /// Check if the stream is currently connected.
        pub fn is_connected(&mut self) -> bool {
            matches!(self.state(), ConnectionState::Connected)
        }
    }

    /// Background task that manages the SSE connection with reconnection.
    async fn connection_loop<T>(
        endpoint: String,
        config: ReconnectConfig,
        tx: Sender<T>,
        state_tx: Sender<ConnectionState>,
    ) where
        T: serde::de::DeserializeOwned + 'static,
    {
        use futures::StreamExt;

        let mut attempt = 0u32;

        loop {
            // Update state
            if attempt == 0 {
                let _ = state_tx.send(ConnectionState::Connecting);
            } else {
                let _ = state_tx.send(ConnectionState::Reconnecting { attempt });
            }

            // Try to connect
            let mut es = match EventSource::new(&endpoint) {
                Ok(es) => es,
                Err(e) => {
                    web_sys::console::error_1(&format!("SSE connection failed: {:?}", e).into());

                    // Check if we've exceeded max attempts
                    if let Some(max) = config.max_attempts {
                        if attempt >= max {
                            let _ = state_tx.send(ConnectionState::Failed);
                            return;
                        }
                    }

                    // Wait and retry
                    let delay = config.delay_for_attempt(attempt);
                    gloo_timers::future::TimeoutFuture::new(delay).await;
                    attempt += 1;
                    continue;
                }
            };

            // Connected successfully
            let _ = state_tx.send(ConnectionState::Connected);
            attempt = 0;

            // Subscribe to messages
            let mut subscription = es.subscribe("message").unwrap();

            // Process events until disconnection
            while let Some(result) = subscription.next().await {
                match result {
                    Ok((_, msg)) => {
                        // Parse the event data
                        if let Some(data) = msg.data().as_string() {
                            match serde_json::from_str::<T>(&data) {
                                Ok(event) => {
                                    if tx.send(event).is_err() {
                                        // Receiver dropped, stop the loop
                                        return;
                                    }
                                }
                                Err(e) => {
                                    web_sys::console::warn_1(
                                        &format!("SSE parse error: {}", e).into(),
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        web_sys::console::error_1(&format!("SSE error: {:?}", e).into());
                        break;
                    }
                }
            }

            // Connection lost, will reconnect
            let _ = state_tx.send(ConnectionState::Disconnected);

            // Check max attempts
            if let Some(max) = config.max_attempts {
                if attempt >= max {
                    let _ = state_tx.send(ConnectionState::Failed);
                    return;
                }
            }

            // Wait before reconnecting
            let delay = config.delay_for_attempt(attempt);
            gloo_timers::future::TimeoutFuture::new(delay).await;
            attempt += 1;
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use client::*;

// ============================================================================
// Server-side SSE helpers (non-WASM only)
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
mod server {
    use axum::response::sse::{Event, KeepAlive, Sse};
    use futures::Stream;
    use std::convert::Infallible;

    /// Convert a stream of serializable items into an SSE response.
    ///
    /// This is a helper for the `#[server(sse)]` macro.
    pub fn into_sse_response<S, T>(stream: S) -> Sse<impl Stream<Item = Result<Event, Infallible>>>
    where
        S: Stream<Item = T> + Send + 'static,
        T: serde::Serialize,
    {
        use futures::StreamExt;

        let event_stream = stream.map(|item| {
            let data = serde_json::to_string(&item).unwrap_or_default();
            Ok(Event::default().data(data))
        });

        Sse::new(event_stream).keep_alive(KeepAlive::default())
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use server::*;
