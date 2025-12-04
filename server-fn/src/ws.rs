//! WebSocket support for bidirectional communication.
//!
//! # Server Usage
//! ```ignore
//! #[server(ws)]
//! pub async fn echo(incoming: impl Stream<Item = String>) -> impl Stream<Item = String> {
//!     incoming.map(|msg| format!("Echo: {}", msg))
//! }
//! ```
//!
//! # Client Usage
//! ```ignore
//! let ws = echo();
//! ws.send("Hello".to_string());
//! for msg in ws.try_iter() {
//!     // Handle message
//! }
//! ```

pub use crate::sse::ReconnectConfig;

// ============================================================================
// Client-side WebSocket Stream (WASM only)
// ============================================================================

#[cfg(target_arch = "wasm32")]
mod client {
    use super::*;
    use gloo_net::websocket::{futures::WebSocket, Message};
    use std::marker::PhantomData;
    use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
    use wasm_bindgen_futures::spawn_local;

    /// Connection state for WebSocket stream.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum WsConnectionState {
        Connecting,
        Connected,
        Reconnecting { attempt: u32 },
        Disconnected,
        Failed,
    }

    /// A handle to a WebSocket connection for bidirectional communication.
    ///
    /// This type manages the underlying WebSocket connection, handles automatic
    /// reconnection, and provides a non-blocking interface for sending and receiving.
    pub struct WsStream<Send, Recv> {
        /// Sender for outgoing messages (to the background task)
        outgoing_tx: Sender<Send>,
        /// Receiver for incoming messages (from the background task)
        incoming_rx: Receiver<Recv>,
        /// Receiver for connection state updates
        state_rx: Receiver<WsConnectionState>,
        /// Current connection state (cached)
        state: WsConnectionState,
        /// Marker for types
        _marker: PhantomData<(Send, Recv)>,
    }

    impl<Send, Recv> std::fmt::Debug for WsStream<Send, Recv> {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WsStream")
                .field("state", &self.state)
                .finish_non_exhaustive()
        }
    }

    impl<Send, Recv> WsStream<Send, Recv>
    where
        Send: serde::Serialize + 'static,
        Recv: serde::de::DeserializeOwned + 'static,
    {
        /// Create a new WebSocket connection to the given endpoint.
        ///
        /// The endpoint should be a path like "/api/echo" - the ws:// or wss://
        /// prefix will be added automatically based on the current page protocol.
        pub fn connect(endpoint: &str) -> Self {
            Self::connect_with_config(endpoint, ReconnectConfig::default())
        }

        /// Create a new WebSocket connection with custom reconnection configuration.
        pub fn connect_with_config(endpoint: &str, config: ReconnectConfig) -> Self {
            let (outgoing_tx, outgoing_rx) = channel();
            let (incoming_tx, incoming_rx) = channel();
            let (state_tx, state_rx) = channel();

            // Build the full WebSocket URL
            let url = build_ws_url(endpoint);

            // Spawn the connection manager
            spawn_local(async move {
                connection_loop::<Send, Recv>(url, config, outgoing_rx, incoming_tx, state_tx).await;
            });

            Self {
                outgoing_tx,
                incoming_rx,
                state_rx,
                state: WsConnectionState::Connecting,
                _marker: PhantomData,
            }
        }

        /// Send a message to the server.
        ///
        /// This is non-blocking and will queue the message for sending.
        /// Returns `true` if the message was queued, `false` if the connection is closed.
        pub fn send(&self, msg: Send) -> bool {
            self.outgoing_tx.send(msg).is_ok()
        }

        /// Try to receive the next message without blocking.
        ///
        /// Returns `Some(message)` if a message is available, `None` otherwise.
        pub fn try_recv(&self) -> Option<Recv> {
            match self.incoming_rx.try_recv() {
                Ok(msg) => Some(msg),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => None,
            }
        }

        /// Returns an iterator over all currently available messages.
        ///
        /// This is useful for processing all pending messages in an egui update loop:
        /// ```ignore
        /// for msg in ws.try_iter() {
        ///     // handle message
        /// }
        /// ```
        pub fn try_iter(&self) -> impl Iterator<Item = Recv> + '_ {
            std::iter::from_fn(|| self.try_recv())
        }

        /// Get the current connection state.
        pub fn state(&mut self) -> WsConnectionState {
            // Update cached state from state channel
            while let Ok(new_state) = self.state_rx.try_recv() {
                self.state = new_state;
            }
            self.state
        }

        /// Check if the WebSocket is currently connected.
        pub fn is_connected(&mut self) -> bool {
            matches!(self.state(), WsConnectionState::Connected)
        }
    }

    /// Build the full WebSocket URL from an endpoint path.
    fn build_ws_url(endpoint: &str) -> String {
        let window = web_sys::window().expect("no window");
        let location = window.location();

        let protocol = location.protocol().unwrap_or_else(|_| "http:".to_string());
        let host = location.host().unwrap_or_else(|_| "localhost".to_string());

        let ws_protocol = if protocol == "https:" { "wss:" } else { "ws:" };

        format!("{}//{}{}", ws_protocol, host, endpoint)
    }

    /// Background task that manages the WebSocket connection with reconnection.
    async fn connection_loop<Send, Recv>(
        url: String,
        config: ReconnectConfig,
        outgoing_rx: Receiver<Send>,
        incoming_tx: Sender<Recv>,
        state_tx: Sender<WsConnectionState>,
    ) where
        Send: serde::Serialize + 'static,
        Recv: serde::de::DeserializeOwned + 'static,
    {
        use futures::{SinkExt, StreamExt};

        let mut attempt = 0u32;

        loop {
            // Update state
            if attempt == 0 {
                let _ = state_tx.send(WsConnectionState::Connecting);
            } else {
                let _ = state_tx.send(WsConnectionState::Reconnecting { attempt });
            }

            // Try to connect
            let ws = match WebSocket::open(&url) {
                Ok(ws) => ws,
                Err(e) => {
                    web_sys::console::error_1(
                        &format!("WebSocket connection failed: {:?}", e).into(),
                    );

                    // Check if we've exceeded max attempts
                    if let Some(max) = config.max_attempts {
                        if attempt >= max {
                            let _ = state_tx.send(WsConnectionState::Failed);
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
            let _ = state_tx.send(WsConnectionState::Connected);
            attempt = 0;

            // Split the WebSocket into sender and receiver
            let (mut ws_tx, mut ws_rx) = ws.split();

            // Process messages until disconnection
            'connection: loop {
                // First, send any pending outgoing messages (non-blocking)
                while let Ok(msg) = outgoing_rx.try_recv() {
                    match serde_json::to_string(&msg) {
                        Ok(json) => {
                            if ws_tx.send(Message::Text(json.into())).await.is_err() {
                                break 'connection;
                            }
                        }
                        Err(e) => {
                            web_sys::console::warn_1(
                                &format!("WebSocket serialize error: {}", e).into(),
                            );
                        }
                    }
                }

                // Wait for next message with timeout using select
                use futures::future::{select, Either};
                let recv_future = Box::pin(ws_rx.next());
                let timeout_future = Box::pin(gloo_timers::future::TimeoutFuture::new(50));

                match select(recv_future, timeout_future).await {
                    Either::Left((Some(result), _)) => {
                        match result {
                            Ok(Message::Text(text)) => {
                                match serde_json::from_str::<Recv>(&text) {
                                    Ok(msg) => {
                                        if incoming_tx.send(msg).is_err() {
                                            return; // Receiver dropped
                                        }
                                    }
                                    Err(e) => {
                                        web_sys::console::warn_1(
                                            &format!("WebSocket parse error: {}", e).into(),
                                        );
                                    }
                                }
                            }
                            Ok(Message::Bytes(bytes)) => {
                                if let Ok(text) = String::from_utf8(bytes) {
                                    if let Ok(msg) = serde_json::from_str::<Recv>(&text) {
                                        if incoming_tx.send(msg).is_err() {
                                            return;
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                web_sys::console::error_1(
                                    &format!("WebSocket error: {:?}", e).into(),
                                );
                                break 'connection;
                            }
                        }
                    }
                    Either::Left((None, _)) => {
                        // Connection closed
                        break 'connection;
                    }
                    Either::Right((_, recv_future)) => {
                        // Timeout - continue loop to check outgoing and retry
                        drop(recv_future);
                    }
                }
            }

            // Connection lost, will reconnect
            let _ = state_tx.send(WsConnectionState::Disconnected);

            // Check max attempts
            if let Some(max) = config.max_attempts {
                if attempt >= max {
                    let _ = state_tx.send(WsConnectionState::Failed);
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
// Server-side WebSocket helpers (non-WASM only)
// ============================================================================

#[cfg(not(target_arch = "wasm32"))]
mod server {
    use axum::{
        extract::ws::{Message, WebSocket, WebSocketUpgrade},
        response::Response,
    };
    use futures::{Stream, StreamExt, SinkExt};
    use std::future::Future;

    /// Trait for WebSocket handlers that transform incoming messages to outgoing messages.
    pub trait WsHandler<In, Out>: Send + 'static {
        /// The stream type returned by the handler.
        type OutStream: Stream<Item = Out> + Send + 'static;

        /// Handle the incoming stream and return an outgoing stream.
        fn handle(self, incoming: impl Stream<Item = In> + Send + 'static) -> Self::OutStream;
    }

    /// Implement WsHandler for closures that return a Stream.
    impl<F, In, Out, S> WsHandler<In, Out> for F
    where
        F: FnOnce(Box<dyn Stream<Item = In> + Send + Unpin>) -> S + Send + 'static,
        S: Stream<Item = Out> + Send + 'static,
        In: Send + 'static,
        Out: Send + 'static,
    {
        type OutStream = S;

        fn handle(self, incoming: impl Stream<Item = In> + Send + 'static) -> Self::OutStream {
            // Box the incoming stream to make it Unpin
            let boxed: Box<dyn Stream<Item = In> + Send + Unpin> = Box::new(Box::pin(incoming));
            self(boxed)
        }
    }

    /// Create a WebSocket upgrade response with a handler.
    pub fn ws_upgrade<In, Out, H, Fut>(
        ws: WebSocketUpgrade,
        handler_fn: impl FnOnce() -> Fut + Send + 'static,
    ) -> Response
    where
        In: serde::de::DeserializeOwned + Send + 'static,
        Out: serde::Serialize + Send + 'static,
        H: WsHandler<In, Out>,
        Fut: Future<Output = H> + Send + 'static,
    {
        ws.on_upgrade(move |socket| async move {
            let handler = handler_fn().await;
            handle_socket::<In, Out, H>(socket, handler).await;
        })
    }

    /// Handle a WebSocket connection with a stream transformer.
    pub async fn handle_socket<In, Out, H>(socket: WebSocket, handler: H)
    where
        In: serde::de::DeserializeOwned + Send + 'static,
        Out: serde::Serialize + Send + 'static,
        H: WsHandler<In, Out>,
    {
        let (mut ws_tx, ws_rx) = socket.split();

        // Create a stream of parsed incoming messages
        let incoming = ws_rx.filter_map(|result| async move {
            match result {
                Ok(Message::Text(text)) => {
                    serde_json::from_str::<In>(&text).ok()
                }
                Ok(Message::Binary(bytes)) => {
                    serde_json::from_slice::<In>(&bytes).ok()
                }
                _ => None,
            }
        });

        // Run the handler to get the outgoing stream
        let outgoing = handler.handle(incoming);
        futures::pin_mut!(outgoing);

        // Send outgoing messages
        while let Some(msg) = outgoing.next().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if ws_tx.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use server::*;
