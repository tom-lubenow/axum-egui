//! WebSocket support for axum-egui.
//!
//! This module provides utilities for bidirectional real-time communication
//! between server and client using WebSockets.
//!
//! # Server Example
//!
//! ```ignore
//! use axum::{Router, routing::get};
//! use axum_egui::ws::{WebSocketUpgrade, WebSocket};
//! use futures_util::{StreamExt, SinkExt};
//!
//! async fn echo(ws: WebSocketUpgrade) -> impl axum::response::IntoResponse {
//!     ws.on_upgrade(|mut socket| async move {
//!         while let Some(Ok(msg)) = socket.recv().await {
//!             if let Some(text) = msg.as_text() {
//!                 if socket.send(Message::text(text)).await.is_err() {
//!                     break;
//!                 }
//!             }
//!         }
//!     })
//! }
//!
//! let app = Router::new().route("/ws", get(echo));
//! ```
//!
//! # Client Example (WASM)
//!
//! ```ignore
//! use axum_egui::ws::WsStream;
//! use futures_util::{StreamExt, SinkExt};
//!
//! async fn connect() -> Result<(), axum_egui::ws::WsError> {
//!     let (mut tx, mut rx) = WsStream::<String, String>::connect("/ws").await?;
//!
//!     tx.send("Hello".to_string()).await?;
//!
//!     while let Some(msg) = rx.next().await {
//!         match msg {
//!             Ok(response) => log::info!("Received: {}", response),
//!             Err(e) => log::error!("Error: {}", e),
//!         }
//!     }
//!     Ok(())
//! }
//! ```

// ============================================================================
// Server-side WebSocket support
// ============================================================================

#[cfg(feature = "server")]
mod server {
    pub use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};

    use axum::extract::ws::Message as WsMessage;
    use bytes::Bytes;
    use futures_util::{SinkExt, Stream, StreamExt};
    use serde::{Serialize, de::DeserializeOwned};
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;

    /// A JSON-based WebSocket stream for sending typed messages.
    ///
    /// This wraps an axum WebSocket and provides automatic JSON serialization.
    pub struct JsonWebSocket<T, R> {
        rx: ReceiverStream<Result<R, String>>,
        tx: mpsc::Sender<T>,
    }

    impl<T, R> JsonWebSocket<T, R>
    where
        T: Serialize + Send + 'static,
        R: DeserializeOwned + Send + 'static,
    {
        /// Create a new JSON WebSocket from an axum WebSocket.
        ///
        /// This spawns a background task to handle message serialization/deserialization.
        pub fn new(socket: WebSocket) -> Self {
            let (mut ws_tx, mut ws_rx) = socket.split();
            let (incoming_tx, incoming_rx) = mpsc::channel::<Result<R, String>>(256);
            let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<T>(256);

            // Spawn task to handle the WebSocket
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        // Handle outgoing messages (T -> WebSocket)
                        outgoing = outgoing_rx.recv() => {
                            match outgoing {
                                Some(msg) => {
                                    match serde_json::to_string(&msg) {
                                        Ok(json) => {
                                            if ws_tx.send(WsMessage::Text(json.into())).await.is_err() {
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            let _ = incoming_tx.send(Err(format!("Serialization error: {}", e))).await;
                                        }
                                    }
                                }
                                None => break,
                            }
                        }
                        // Handle incoming messages (WebSocket -> R)
                        incoming = ws_rx.next() => {
                            match incoming {
                                Some(Ok(WsMessage::Text(text))) => {
                                    match serde_json::from_str::<R>(&text) {
                                        Ok(msg) => {
                                            if incoming_tx.send(Ok(msg)).await.is_err() {
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            let _ = incoming_tx.send(Err(format!("Parse error: {}", e))).await;
                                        }
                                    }
                                }
                                Some(Ok(WsMessage::Binary(bytes))) => {
                                    match serde_json::from_slice::<R>(&bytes) {
                                        Ok(msg) => {
                                            if incoming_tx.send(Ok(msg)).await.is_err() {
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            let _ = incoming_tx.send(Err(format!("Parse error: {}", e))).await;
                                        }
                                    }
                                }
                                Some(Ok(WsMessage::Ping(bytes))) => {
                                    if ws_tx.send(WsMessage::Pong(bytes)).await.is_err() {
                                        break;
                                    }
                                }
                                Some(Ok(WsMessage::Pong(_))) => {}
                                Some(Ok(WsMessage::Close(_))) => break,
                                Some(Err(e)) => {
                                    let _ = incoming_tx.send(Err(format!("WebSocket error: {}", e))).await;
                                    break;
                                }
                                None => break,
                            }
                        }
                    }
                }

                let _ = ws_tx.send(WsMessage::Close(None)).await;
            });

            Self {
                rx: ReceiverStream::new(incoming_rx),
                tx: outgoing_tx,
            }
        }

        /// Send a message to the client.
        pub fn send(&self, msg: T) -> Result<(), String> {
            self.tx
                .try_send(msg)
                .map_err(|e| format!("Send error: {}", e))
        }

        /// Split into separate sender and receiver.
        pub fn split(self) -> (WsSender<T>, WsReceiver<R>) {
            (WsSender { tx: self.tx }, WsReceiver { rx: self.rx })
        }
    }

    impl<T, R> Stream for JsonWebSocket<T, R> {
        type Item = Result<R, String>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Pin::new(&mut self.rx).poll_next(cx)
        }
    }

    /// Sender half of a split JsonWebSocket.
    pub struct WsSender<T> {
        tx: mpsc::Sender<T>,
    }

    impl<T> WsSender<T> {
        /// Send a message.
        pub fn send(&self, msg: T) -> Result<(), String> {
            self.tx
                .try_send(msg)
                .map_err(|e| format!("Send error: {}", e))
        }
    }

    /// Receiver half of a split JsonWebSocket.
    pub struct WsReceiver<R> {
        rx: ReceiverStream<Result<R, String>>,
    }

    impl<R> Stream for WsReceiver<R> {
        type Item = Result<R, String>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Pin::new(&mut self.rx).poll_next(cx)
        }
    }

    /// Extension trait for upgrading WebSocket connections with JSON handling.
    pub trait WebSocketUpgradeExt {
        /// Upgrade the connection and handle it with a JSON-typed WebSocket.
        fn on_upgrade_json<T, R, F, Fut>(self, callback: F) -> axum::response::Response
        where
            T: Serialize + Send + 'static,
            R: DeserializeOwned + Send + 'static,
            F: FnOnce(JsonWebSocket<T, R>) -> Fut + Send + 'static,
            Fut: std::future::Future<Output = ()> + Send + 'static;
    }

    impl WebSocketUpgradeExt for WebSocketUpgrade {
        fn on_upgrade_json<T, R, F, Fut>(self, callback: F) -> axum::response::Response
        where
            T: Serialize + Send + 'static,
            R: DeserializeOwned + Send + 'static,
            F: FnOnce(JsonWebSocket<T, R>) -> Fut + Send + 'static,
            Fut: std::future::Future<Output = ()> + Send + 'static,
        {
            self.on_upgrade(|socket| async move {
                let json_socket = JsonWebSocket::new(socket);
                callback(json_socket).await;
            })
        }
    }

    /// Create a raw bidirectional byte stream from a WebSocket.
    ///
    /// This is a lower-level API similar to Leptos's WebSocket protocol.
    /// Returns (incoming_stream, outgoing_sink, response).
    pub fn into_raw_websocket(
        upgrade: WebSocketUpgrade,
    ) -> (
        ReceiverStream<Result<Bytes, String>>,
        mpsc::Sender<Bytes>,
        axum::response::Response,
    ) {
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<Result<Bytes, String>>(2048);
        let (incoming_tx, mut incoming_rx) = mpsc::channel::<Bytes>(2048);

        let response = upgrade.on_upgrade(|socket| async move {
            let (mut ws_tx, mut ws_rx) = socket.split();

            loop {
                tokio::select! {
                    incoming = incoming_rx.recv() => {
                        let Some(incoming) = incoming else {
                            break;
                        };
                        if ws_tx.send(WsMessage::Binary(incoming)).await.is_err() {
                            break;
                        }
                    }
                    outgoing = ws_rx.next() => {
                        match outgoing {
                            Some(Ok(WsMessage::Binary(bytes))) => {
                                let _ = outgoing_tx.send(Ok(bytes)).await;
                            }
                            Some(Ok(WsMessage::Text(text))) => {
                                let _ = outgoing_tx.send(Ok(Bytes::from(text.to_string()))).await;
                            }
                            Some(Ok(WsMessage::Ping(bytes))) => {
                                if ws_tx.send(WsMessage::Pong(bytes)).await.is_err() {
                                    break;
                                }
                            }
                            Some(Ok(WsMessage::Pong(_))) => {}
                            Some(Ok(WsMessage::Close(_))) => break,
                            Some(Err(e)) => {
                                let _ = outgoing_tx.send(Err(e.to_string())).await;
                                break;
                            }
                            None => break,
                        }
                    }
                }
            }

            let _ = ws_tx.send(WsMessage::Close(None)).await;
        });

        (ReceiverStream::new(outgoing_rx), incoming_tx, response)
    }
}

#[cfg(feature = "server")]
pub use server::*;

// ============================================================================
// Client-side WebSocket support
// ============================================================================

#[cfg(feature = "client")]
mod client {
    use futures_channel::mpsc;
    use futures_util::{SinkExt, Stream, StreamExt};
    use gloo_net::websocket::{Message, futures::WebSocket};
    use send_wrapper::SendWrapper;
    use serde::{Serialize, de::DeserializeOwned};
    use std::pin::Pin;
    use std::task::{Context, Poll};

    /// Error type for WebSocket client operations.
    #[derive(Debug, Clone)]
    pub enum WsError {
        /// Failed to connect to the WebSocket endpoint.
        Connection(String),
        /// Failed to parse the message data.
        Parse(String),
        /// Failed to send a message.
        Send(String),
        /// The connection was closed.
        Closed,
    }

    impl std::fmt::Display for WsError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                WsError::Connection(msg) => write!(f, "WebSocket connection error: {}", msg),
                WsError::Parse(msg) => write!(f, "WebSocket parse error: {}", msg),
                WsError::Send(msg) => write!(f, "WebSocket send error: {}", msg),
                WsError::Closed => write!(f, "WebSocket closed"),
            }
        }
    }

    impl std::error::Error for WsError {}

    /// Client-side WebSocket connection helper with JSON serialization.
    ///
    /// Use `WsStream::connect` to establish a typed WebSocket connection.
    pub struct WsStream<T, R>(std::marker::PhantomData<(T, R)>);

    impl<T, R> WsStream<T, R>
    where
        T: Serialize + 'static,
        R: DeserializeOwned + 'static,
    {
        /// Connect to a WebSocket endpoint.
        ///
        /// Returns a sender for type T and receiver for type R.
        pub async fn connect(
            url: &str,
        ) -> Result<(WsClientSender<T>, WsClientReceiver<R>), WsError> {
            // Convert relative URL to absolute WebSocket URL
            let ws_url = if url.starts_with("ws://") || url.starts_with("wss://") {
                url.to_string()
            } else {
                let window = web_sys::window().ok_or(WsError::Connection(
                    "No window object available".to_string(),
                ))?;
                let location = window.location();
                let protocol = location.protocol().unwrap_or_default();
                let host = location.host().unwrap_or_default();
                let ws_protocol = if protocol == "https:" { "wss:" } else { "ws:" };
                format!("{}//{}{}", ws_protocol, host, url)
            };

            let websocket =
                WebSocket::open(&ws_url).map_err(|e| WsError::Connection(format!("{:?}", e)))?;

            let (ws_sink, ws_stream) = websocket.split();

            // Create unbounded channels for typed messages
            let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded::<T>();
            let (incoming_tx, incoming_rx) = mpsc::unbounded::<Result<R, WsError>>();

            // Wrap sink for sending
            let ws_sink = SendWrapper::new(ws_sink);
            let ws_stream = SendWrapper::new(ws_stream);

            // Spawn task to handle outgoing messages
            wasm_bindgen_futures::spawn_local(async move {
                let mut ws_sink = ws_sink;
                while let Some(msg) = outgoing_rx.next().await {
                    match serde_json::to_string(&msg) {
                        Ok(json) => {
                            if ws_sink.send(Message::Text(json)).await.is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            web_sys::console::error_1(
                                &format!("Serialization error: {}", e).into(),
                            );
                        }
                    }
                }
            });

            // Spawn task to handle incoming messages
            wasm_bindgen_futures::spawn_local(async move {
                let mut ws_stream = ws_stream;
                while let Some(msg) = ws_stream.next().await {
                    match msg {
                        Ok(Message::Text(text)) => match serde_json::from_str::<R>(&text) {
                            Ok(parsed) => {
                                if incoming_tx.unbounded_send(Ok(parsed)).is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ =
                                    incoming_tx.unbounded_send(Err(WsError::Parse(e.to_string())));
                            }
                        },
                        Ok(Message::Bytes(bytes)) => match serde_json::from_slice::<R>(&bytes) {
                            Ok(parsed) => {
                                if incoming_tx.unbounded_send(Ok(parsed)).is_err() {
                                    break;
                                }
                            }
                            Err(e) => {
                                let _ =
                                    incoming_tx.unbounded_send(Err(WsError::Parse(e.to_string())));
                            }
                        },
                        Err(e) => {
                            web_sys::console::error_1(&format!("WebSocket error: {:?}", e).into());
                            let _ = incoming_tx
                                .unbounded_send(Err(WsError::Connection(format!("{:?}", e))));
                            break;
                        }
                    }
                }
            });

            Ok((
                WsClientSender {
                    tx: outgoing_tx,
                    _phantom: std::marker::PhantomData,
                },
                WsClientReceiver { rx: incoming_rx },
            ))
        }
    }

    /// Sender half for client WebSocket.
    pub struct WsClientSender<T> {
        tx: mpsc::UnboundedSender<T>,
        _phantom: std::marker::PhantomData<T>,
    }

    impl<T> WsClientSender<T> {
        /// Send a message to the server.
        ///
        /// This is a synchronous operation that queues the message for sending.
        pub fn send(&self, msg: T) -> Result<(), WsError> {
            self.tx
                .unbounded_send(msg)
                .map_err(|e| WsError::Send(e.to_string()))
        }
    }

    /// Receiver half for client WebSocket.
    pub struct WsClientReceiver<R> {
        rx: mpsc::UnboundedReceiver<Result<R, WsError>>,
    }

    impl<R> Stream for WsClientReceiver<R> {
        type Item = Result<R, WsError>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Pin::new(&mut self.rx).poll_next(cx)
        }
    }

    /// Open a raw WebSocket connection returning byte streams.
    ///
    /// This is a lower-level API for custom protocols.
    pub async fn open_raw_websocket(url: &str) -> Result<(WsRawSender, WsRawReceiver), WsError> {
        // Convert relative URL to absolute WebSocket URL
        let ws_url = if url.starts_with("ws://") || url.starts_with("wss://") {
            url.to_string()
        } else {
            let window = web_sys::window().ok_or(WsError::Connection(
                "No window object available".to_string(),
            ))?;
            let location = window.location();
            let protocol = location.protocol().unwrap_or_default();
            let host = location.host().unwrap_or_default();
            let ws_protocol = if protocol == "https:" { "wss:" } else { "ws:" };
            format!("{}//{}{}", ws_protocol, host, url)
        };

        let websocket =
            WebSocket::open(&ws_url).map_err(|e| WsError::Connection(format!("{:?}", e)))?;

        let (ws_sink, ws_stream) = websocket.split();

        // Create channels
        let (outgoing_tx, mut outgoing_rx) = mpsc::unbounded::<Vec<u8>>();
        let (incoming_tx, incoming_rx) = mpsc::unbounded::<Result<Vec<u8>, WsError>>();

        let ws_sink = SendWrapper::new(ws_sink);
        let ws_stream = SendWrapper::new(ws_stream);

        // Spawn task for outgoing messages
        wasm_bindgen_futures::spawn_local(async move {
            let mut ws_sink = ws_sink;
            while let Some(bytes) = outgoing_rx.next().await {
                if ws_sink.send(Message::Bytes(bytes)).await.is_err() {
                    break;
                }
            }
        });

        // Spawn task for incoming messages
        wasm_bindgen_futures::spawn_local(async move {
            let mut ws_stream = ws_stream;
            while let Some(msg) = ws_stream.next().await {
                let result = match msg {
                    Ok(Message::Text(text)) => Ok(text.into_bytes()),
                    Ok(Message::Bytes(bytes)) => Ok(bytes),
                    Err(e) => Err(WsError::Connection(format!("{:?}", e))),
                };
                if incoming_tx.unbounded_send(result).is_err() {
                    break;
                }
            }
        });

        Ok((
            WsRawSender { tx: outgoing_tx },
            WsRawReceiver { rx: incoming_rx },
        ))
    }

    /// Sender for raw WebSocket bytes.
    pub struct WsRawSender {
        tx: mpsc::UnboundedSender<Vec<u8>>,
    }

    impl WsRawSender {
        /// Send raw bytes to the server.
        pub fn send(&self, bytes: Vec<u8>) -> Result<(), WsError> {
            self.tx
                .unbounded_send(bytes)
                .map_err(|e| WsError::Send(e.to_string()))
        }
    }

    /// Receiver for raw WebSocket bytes.
    pub struct WsRawReceiver {
        rx: mpsc::UnboundedReceiver<Result<Vec<u8>, WsError>>,
    }

    impl Stream for WsRawReceiver {
        type Item = Result<Vec<u8>, WsError>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Pin::new(&mut self.rx).poll_next(cx)
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

    #[test]
    fn message_types_exist() {
        // Verify we can access the re-exported types
        let _msg = Message::Text("test".into());
        let _msg = Message::Binary(vec![1, 2, 3].into());
    }
}
