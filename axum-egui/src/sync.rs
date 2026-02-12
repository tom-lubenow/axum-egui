//! Reactive shared state synchronization for axum-egui.
//!
//! This module provides a `SharedState<T>` (server) and `StateSync<T>` (client)
//! for automatically synchronizing server-side state changes to connected clients
//! over WebSockets.
//!
//! The server holds the authoritative state and broadcasts full snapshots on each
//! mutation. Clients maintain a local copy that is updated in the background.
//!
//! # Server Example
//!
//! ```ignore
//! use axum::{Router, routing::get};
//! use axum_egui::sync::SharedState;
//! use serde::{Serialize, Deserialize};
//!
//! #[derive(Clone, Serialize, Deserialize)]
//! struct AppState { counter: i32 }
//!
//! #[tokio::main]
//! async fn main() {
//!     let state = SharedState::new(AppState { counter: 0 });
//!
//!     // Mutate — automatically broadcasts to all clients
//!     state.update(|s| s.counter += 1);
//!
//!     // Read current value
//!     let current = state.read();
//!
//!     let app = Router::new()
//!         .route("/api/sync", get(state.sync_handler()));
//!     // ...
//! }
//! ```
//!
//! # Client Example (WASM)
//!
//! ```ignore
//! use axum_egui::sync::StateSync;
//! use futures_util::StreamExt;
//!
//! async fn connect() -> Result<(), axum_egui::sync::SyncError> {
//!     let sync = StateSync::<AppState>::connect("/api/sync").await?;
//!
//!     // Read latest state
//!     let current = sync.current();
//!
//!     // Subscribe to changes
//!     let mut rx = sync.subscribe();
//!     while let Some(new_state) = rx.next().await {
//!         // Handle updated state
//!     }
//!     Ok(())
//! }
//! ```

// ============================================================================
// Server-side shared state
// ============================================================================

#[cfg(feature = "server")]
mod server {
    use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
    use futures_util::{SinkExt, StreamExt};
    use serde::{Serialize, de::DeserializeOwned};
    use std::sync::{Arc, RwLock};
    use tokio::sync::broadcast;

    /// Reactive shared state that broadcasts changes to connected clients.
    ///
    /// `SharedState<T>` wraps a value of type `T` and provides:
    /// - Thread-safe read/write access via `read()` and `update()`
    /// - Automatic broadcasting of state snapshots on mutation
    /// - A WebSocket handler for client synchronization
    ///
    /// It is cheaply cloneable (backed by `Arc`) and can be shared across
    /// axum handlers and background tasks.
    pub struct SharedState<T> {
        inner: Arc<SharedStateInner<T>>,
    }

    impl<T> Clone for SharedState<T> {
        fn clone(&self) -> Self {
            Self {
                inner: self.inner.clone(),
            }
        }
    }

    struct SharedStateInner<T> {
        state: RwLock<T>,
        /// Broadcast channel for serialized state snapshots.
        tx: broadcast::Sender<Vec<u8>>,
    }

    /// A receiver for serialized state broadcasts from a `SharedState`.
    ///
    /// This is a thin wrapper around `tokio::sync::broadcast::Receiver<Vec<u8>>`
    /// that deserializes incoming state snapshots.
    pub struct SharedStateReceiver<T> {
        rx: broadcast::Receiver<Vec<u8>>,
        _phantom: std::marker::PhantomData<T>,
    }

    impl<T: DeserializeOwned> SharedStateReceiver<T> {
        /// Receive the next serialized state update.
        ///
        /// Returns the deserialized state, or an error if the channel is closed
        /// or the receiver lagged behind.
        pub async fn recv(&mut self) -> Result<T, broadcast::error::RecvError> {
            let data = self.rx.recv().await?;
            // Deserialization is infallible for well-formed state that was
            // serialized by `SharedState::update`.
            Ok(serde_json::from_slice(&data).expect("broadcast data should be valid JSON"))
        }
    }

    impl<T> SharedState<T>
    where
        T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
    {
        /// Create a new `SharedState` with the given initial value.
        ///
        /// The broadcast channel is created with a capacity of 64 messages.
        /// If a subscriber falls behind by more than 64 updates, it will
        /// skip to the latest state.
        pub fn new(initial: T) -> Self {
            let (tx, _) = broadcast::channel(64);
            Self {
                inner: Arc::new(SharedStateInner {
                    state: RwLock::new(initial),
                    tx,
                }),
            }
        }

        /// Read the current state.
        ///
        /// Returns a clone of the current value.
        pub fn read(&self) -> T {
            self.inner.state.read().unwrap().clone()
        }

        /// Mutate the state and broadcast the new value to all connected clients.
        ///
        /// The closure receives a mutable reference to the state. After it returns,
        /// the updated state is serialized to JSON and sent to all subscribers.
        pub fn update(&self, f: impl FnOnce(&mut T)) {
            let mut state = self.inner.state.write().unwrap();
            f(&mut *state);
            // Serialize and broadcast; ignore errors (no active receivers is fine)
            let serialized = serde_json::to_vec(&*state).unwrap();
            let _ = self.inner.tx.send(serialized);
        }

        /// Subscribe to state change broadcasts.
        ///
        /// Returns a `SharedStateReceiver` that yields deserialized state values
        /// each time `update()` is called.
        pub fn subscribe(&self) -> SharedStateReceiver<T> {
            SharedStateReceiver {
                rx: self.inner.tx.subscribe(),
                _phantom: std::marker::PhantomData,
            }
        }

        /// Returns the number of currently connected sync subscribers.
        pub fn subscriber_count(&self) -> usize {
            self.inner.tx.receiver_count()
        }

        /// Returns an axum handler that upgrades connections to WebSocket-based
        /// state synchronization.
        ///
        /// The handler:
        /// 1. Sends the current state snapshot immediately on connect
        /// 2. Subscribes to the broadcast channel
        /// 3. Forwards all subsequent state updates to the client
        ///
        /// # Example
        ///
        /// ```ignore
        /// let app = Router::new()
        ///     .route("/api/sync", get(state.sync_handler()));
        /// ```
        pub fn sync_handler(
            &self,
        ) -> impl Fn(WebSocketUpgrade) -> axum::response::Response + Clone + Send + 'static
        {
            let inner = self.inner.clone();
            move |ws: WebSocketUpgrade| {
                let inner = inner.clone();
                ws.on_upgrade(move |socket| async move {
                    handle_sync_client(inner, socket).await;
                })
            }
        }
    }

    /// Handle a single sync client connection.
    async fn handle_sync_client<T>(inner: Arc<SharedStateInner<T>>, socket: WebSocket)
    where
        T: Serialize + DeserializeOwned + Clone + Send + Sync + 'static,
    {
        let (mut ws_tx, mut ws_rx) = socket.split();

        // 1. Send current state immediately
        let current = {
            let state = inner.state.read().unwrap();
            serde_json::to_vec(&*state).unwrap()
        };
        if ws_tx.send(WsMessage::Binary(current.into())).await.is_err() {
            return;
        }

        // 2. Subscribe to broadcast channel
        let mut rx = inner.tx.subscribe();

        // 3. Forward updates to the WebSocket, also listen for client disconnect
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(data) => {
                            if ws_tx.send(WsMessage::Binary(data.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            // Client fell behind — send latest state to catch up
                            let current = {
                                let state = inner.state.read().unwrap();
                                serde_json::to_vec(&*state).unwrap()
                            };
                            if ws_tx.send(WsMessage::Binary(current.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            break;
                        }
                    }
                }
                msg = ws_rx.next() => {
                    match msg {
                        // Client sent close or disconnected
                        Some(Ok(WsMessage::Close(_))) | None => break,
                        // Respond to pings
                        Some(Ok(WsMessage::Ping(bytes))) => {
                            if ws_tx.send(WsMessage::Pong(bytes)).await.is_err() {
                                break;
                            }
                        }
                        // Ignore other messages from client (server is authoritative)
                        Some(Ok(_)) => {}
                        Some(Err(_)) => break,
                    }
                }
            }
        }

        // Best-effort close
        let _ = ws_tx.send(WsMessage::Close(None)).await;
    }
}

#[cfg(feature = "server")]
pub use server::*;

// ============================================================================
// Client-side state sync
// ============================================================================

#[cfg(feature = "client")]
mod client {
    use futures_channel::mpsc;
    use futures_util::{Stream, StreamExt};
    use gloo_net::websocket::{Message, futures::WebSocket};
    use send_wrapper::SendWrapper;
    use serde::de::DeserializeOwned;
    use std::pin::Pin;
    use std::sync::{Arc, RwLock};
    use std::task::{Context, Poll};

    /// Error type for state synchronization operations.
    #[derive(Debug, Clone)]
    pub enum SyncError {
        /// Failed to connect to the sync endpoint.
        Connection(String),
        /// Failed to deserialize a state snapshot.
        Deserialization(String),
        /// The sync connection was closed.
        Closed,
    }

    impl std::fmt::Display for SyncError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                SyncError::Connection(msg) => write!(f, "Sync connection error: {}", msg),
                SyncError::Deserialization(msg) => {
                    write!(f, "Sync deserialization error: {}", msg)
                }
                SyncError::Closed => write!(f, "Sync connection closed"),
            }
        }
    }

    impl std::error::Error for SyncError {}

    /// Client-side reactive state synchronization.
    ///
    /// Connects to a server's `SharedState` sync endpoint over WebSocket and
    /// maintains a local copy of the state that is automatically updated when
    /// the server broadcasts changes.
    ///
    /// Use `current()` to read the latest state, or `subscribe()` to receive
    /// a stream of state updates.
    pub struct StateSync<T> {
        state: Arc<RwLock<T>>,
        /// Shared list of subscriber notification senders. The background task
        /// pushes to all of them on each update; dead senders are pruned lazily.
        subscribers: Arc<RwLock<Vec<mpsc::UnboundedSender<()>>>>,
    }

    impl<T> StateSync<T>
    where
        T: DeserializeOwned + Clone + 'static,
    {
        /// Connect to a state sync WebSocket endpoint.
        ///
        /// This opens a WebSocket connection, receives the initial state snapshot,
        /// and spawns a background task to keep the local state updated.
        ///
        /// The `url` can be a relative path (e.g., `/api/sync`) or an absolute
        /// WebSocket URL (e.g., `ws://localhost:3000/api/sync`).
        pub async fn connect(url: &str) -> Result<Self, SyncError> {
            // Convert relative URL to absolute WebSocket URL
            let ws_url = if url.starts_with("ws://") || url.starts_with("wss://") {
                url.to_string()
            } else {
                let window = web_sys::window().ok_or(SyncError::Connection(
                    "No window object available".to_string(),
                ))?;
                let location = window.location();
                let protocol = location.protocol().unwrap_or_default();
                let host = location.host().unwrap_or_default();
                let ws_protocol = if protocol == "https:" { "wss:" } else { "ws:" };
                format!("{}//{}{}", ws_protocol, host, url)
            };

            let websocket =
                WebSocket::open(&ws_url).map_err(|e| SyncError::Connection(format!("{:?}", e)))?;

            let (_ws_sink, ws_stream) = websocket.split();

            let mut ws_stream = SendWrapper::new(ws_stream);

            // Receive initial state
            let initial_msg = ws_stream
                .next()
                .await
                .ok_or(SyncError::Closed)?
                .map_err(|e| SyncError::Connection(format!("{:?}", e)))?;

            let initial_bytes = match initial_msg {
                Message::Bytes(b) => b,
                Message::Text(t) => t.into_bytes(),
            };

            let initial: T = serde_json::from_slice(&initial_bytes)
                .map_err(|e| SyncError::Deserialization(e.to_string()))?;

            let state: Arc<RwLock<T>> = Arc::new(RwLock::new(initial));
            let subscribers: Arc<RwLock<Vec<mpsc::UnboundedSender<()>>>> =
                Arc::new(RwLock::new(Vec::new()));

            // Spawn background task to receive updates
            let state_bg = state.clone();
            let subscribers_bg = subscribers.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let mut ws_stream = ws_stream;
                while let Some(msg) = ws_stream.next().await {
                    let bytes = match msg {
                        Ok(Message::Bytes(b)) => b,
                        Ok(Message::Text(t)) => t.into_bytes(),
                        Err(e) => {
                            web_sys::console::error_1(&format!("StateSync error: {:?}", e).into());
                            break;
                        }
                    };

                    if let Ok(new_state) = serde_json::from_slice::<T>(&bytes) {
                        *state_bg.write().unwrap() = new_state;

                        // Notify all subscribers, pruning closed channels
                        let mut subs = subscribers_bg.write().unwrap();
                        subs.retain(|tx: &mpsc::UnboundedSender<()>| tx.unbounded_send(()).is_ok());
                    }
                }
            });

            Ok(Self { state, subscribers })
        }

        /// Get the current state.
        ///
        /// Returns a clone of the latest state snapshot received from the server.
        pub fn current(&self) -> T {
            self.state.read().unwrap().clone()
        }

        /// Subscribe to state changes.
        ///
        /// Returns a `SyncReceiver` stream that yields the new state value
        /// each time the server broadcasts an update.
        pub fn subscribe(&self) -> SyncReceiver<T> {
            let (tx, rx) = mpsc::unbounded();
            self.subscribers.write().unwrap().push(tx);
            SyncReceiver {
                state: self.state.clone(),
                rx,
            }
        }
    }

    /// A stream of state updates from a `StateSync` subscription.
    ///
    /// Each item yielded is the full current state after an update.
    pub struct SyncReceiver<T> {
        state: Arc<RwLock<T>>,
        rx: mpsc::UnboundedReceiver<()>,
    }

    impl<T: Clone> Stream for SyncReceiver<T> {
        type Item = T;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            match Pin::new(&mut self.rx).poll_next(cx) {
                Poll::Ready(Some(())) => {
                    let state = self.state.read().unwrap().clone();
                    Poll::Ready(Some(state))
                }
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            }
        }
    }
}

#[cfg(feature = "client")]
pub use client::*;

// ============================================================================
// Tests
// ============================================================================

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestState {
        counter: i32,
        message: String,
    }

    #[test]
    fn shared_state_new_and_read() {
        let state = SharedState::new(TestState {
            counter: 0,
            message: "hello".into(),
        });
        let current = state.read();
        assert_eq!(current.counter, 0);
        assert_eq!(current.message, "hello");
    }

    #[test]
    fn shared_state_update_mutates_state() {
        let state = SharedState::new(TestState {
            counter: 0,
            message: "hello".into(),
        });
        state.update(|s| s.counter += 1);
        assert_eq!(state.read().counter, 1);

        state.update(|s| {
            s.counter += 10;
            s.message = "updated".into();
        });
        let current = state.read();
        assert_eq!(current.counter, 11);
        assert_eq!(current.message, "updated");
    }

    #[test]
    fn shared_state_clone_shares_data() {
        let state1 = SharedState::new(TestState {
            counter: 0,
            message: "shared".into(),
        });
        let state2 = state1.clone();

        state1.update(|s| s.counter = 42);
        assert_eq!(state2.read().counter, 42);

        state2.update(|s| s.message = "from clone".into());
        assert_eq!(state1.read().message, "from clone");
    }

    #[tokio::test]
    async fn shared_state_broadcast_sends_to_subscribers() {
        let state = SharedState::new(TestState {
            counter: 0,
            message: "init".into(),
        });

        let mut rx = state.subscribe();

        state.update(|s| s.counter = 99);

        let received = rx.recv().await.unwrap();
        assert_eq!(received.counter, 99);
        assert_eq!(received.message, "init");
    }

    #[tokio::test]
    async fn shared_state_multiple_subscribers_receive_updates() {
        let state = SharedState::new(TestState {
            counter: 0,
            message: "start".into(),
        });

        let mut rx1 = state.subscribe();
        let mut rx2 = state.subscribe();

        state.update(|s| s.counter = 7);

        let received1: TestState = rx1.recv().await.unwrap();
        let received2: TestState = rx2.recv().await.unwrap();

        assert_eq!(received1, received2);
        assert_eq!(received1.counter, 7);
    }

    #[test]
    fn shared_state_subscriber_count() {
        let state = SharedState::new(TestState {
            counter: 0,
            message: "test".into(),
        });

        assert_eq!(state.subscriber_count(), 0);

        let _rx1 = state.subscribe();
        assert_eq!(state.subscriber_count(), 1);

        let _rx2 = state.subscribe();
        assert_eq!(state.subscriber_count(), 2);

        drop(_rx1);
        assert_eq!(state.subscriber_count(), 1);
    }

    #[test]
    fn shared_state_update_without_subscribers_does_not_panic() {
        let state = SharedState::new(TestState {
            counter: 0,
            message: "alone".into(),
        });

        // No subscribers — update should succeed without error
        state.update(|s| s.counter = 100);
        assert_eq!(state.read().counter, 100);
    }

    #[tokio::test]
    async fn shared_state_rapid_updates() {
        let state = SharedState::new(TestState {
            counter: 0,
            message: "rapid".into(),
        });

        let mut rx = state.subscribe();

        for i in 1..=10 {
            state.update(|s| s.counter = i);
        }

        // Should receive all 10 updates
        for i in 1..=10 {
            let received: TestState = rx.recv().await.unwrap();
            assert_eq!(received.counter, i);
        }
    }

    #[test]
    fn shared_state_works_with_primitive_types() {
        let state = SharedState::new(42i32);
        assert_eq!(state.read(), 42);
        state.update(|s| *s = 100);
        assert_eq!(state.read(), 100);
    }

    #[test]
    fn shared_state_works_with_vec() {
        let state = SharedState::new(vec![1, 2, 3]);
        state.update(|s| s.push(4));
        assert_eq!(state.read(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn shared_state_sync_handler_returns_handler() {
        let state = SharedState::new(TestState {
            counter: 0,
            message: "handler test".into(),
        });
        // Verify sync_handler() compiles and returns a callable
        let _handler = state.sync_handler();
    }
}
