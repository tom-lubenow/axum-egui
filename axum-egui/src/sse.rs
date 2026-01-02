//! Server-Sent Events (SSE) support for axum-egui.
//!
//! This module provides utilities for streaming real-time updates from
//! server to client using SSE.
//!
//! # Server Example
//!
//! ```ignore
//! use axum_egui::sse::{Sse, Event, KeepAlive};
//! use futures_util::stream::{self, Stream};
//! use std::time::Duration;
//!
//! async fn counter() -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
//!     let stream = stream::unfold(0, |count| async move {
//!         tokio::time::sleep(Duration::from_secs(1)).await;
//!         let event = Event::default()
//!             .json_data(count)
//!             .unwrap();
//!         Some((Ok(event), count + 1))
//!     });
//!
//!     Sse::new(stream).keep_alive(KeepAlive::default())
//! }
//! ```

#[cfg(feature = "server")]
mod server {
    use axum::response::sse::{Event as AxumEvent, KeepAlive as AxumKeepAlive, Sse as AxumSse};
    use serde::Serialize;

    /// SSE response type. Wraps axum's Sse.
    pub type Sse<S> = AxumSse<S>;

    /// Keep-alive configuration for SSE streams.
    pub type KeepAlive = AxumKeepAlive;

    /// An SSE event with convenience methods for JSON serialization.
    #[derive(Debug, Clone)]
    pub struct Event {
        inner: AxumEvent,
    }

    impl Default for Event {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Event {
        /// Create a new empty event.
        pub fn new() -> Self {
            Self {
                inner: AxumEvent::default(),
            }
        }

        /// Set the event data as JSON.
        ///
        /// This is the most common way to send structured data to clients.
        pub fn json_data<T: Serialize>(mut self, data: T) -> Result<Self, serde_json::Error> {
            let json = serde_json::to_string(&data)?;
            self.inner = self.inner.data(json);
            Ok(self)
        }

        /// Set the event data as a raw string.
        pub fn data<T: AsRef<str>>(mut self, data: T) -> Self {
            self.inner = self.inner.data(data);
            self
        }

        /// Set the event name/type.
        ///
        /// Clients can filter events by name using `EventSource.addEventListener()`.
        pub fn event<T: AsRef<str>>(mut self, event: T) -> Self {
            self.inner = self.inner.event(event);
            self
        }

        /// Set the event ID.
        ///
        /// Clients can use this for resuming streams after disconnection.
        pub fn id<T: AsRef<str>>(mut self, id: T) -> Self {
            self.inner = self.inner.id(id);
            self
        }

        /// Set the retry interval in milliseconds.
        pub fn retry(mut self, duration: std::time::Duration) -> Self {
            self.inner = self.inner.retry(duration);
            self
        }

        /// Add a comment to the event.
        pub fn comment<T: AsRef<str>>(mut self, comment: T) -> Self {
            self.inner = self.inner.comment(comment);
            self
        }
    }

    impl From<Event> for AxumEvent {
        fn from(event: Event) -> Self {
            event.inner
        }
    }

    /// Extension trait for creating SSE streams from iterators.
    pub trait SseExt<T, E>: Sized {
        /// Convert a stream of serializable items into an SSE stream.
        fn into_sse_stream(
            self,
        ) -> impl futures_util::Stream<Item = Result<AxumEvent, std::convert::Infallible>>;
    }

    impl<S, T, E> SseExt<T, E> for S
    where
        S: futures_util::Stream<Item = Result<T, E>>,
        T: Serialize,
        E: std::fmt::Display,
    {
        fn into_sse_stream(
            self,
        ) -> impl futures_util::Stream<Item = Result<AxumEvent, std::convert::Infallible>> {
            use futures_util::StreamExt;

            self.map(|result| {
                Ok(match result {
                    Ok(data) => Event::new()
                        .json_data(&data)
                        .unwrap_or_else(|e| Event::new().data(format!("serialization error: {e}")))
                        .into(),
                    Err(e) => Event::new().event("error").data(e.to_string()).into(),
                })
            })
        }
    }
}

#[cfg(feature = "server")]
pub use server::*;

// ============================================================================
// Client-side SSE support
// ============================================================================

#[cfg(feature = "client")]
mod client {
    use futures_util::stream::Stream;
    use gloo_net::eventsource::futures::{EventSource, EventSourceSubscription};
    use serde::de::DeserializeOwned;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use wasm_bindgen::JsCast;

    /// Error type for SSE client operations.
    #[derive(Debug, Clone)]
    pub enum SseError {
        /// Failed to connect to the SSE endpoint.
        Connection(String),
        /// Failed to parse the event data.
        Parse(String),
        /// The stream was closed.
        Closed,
    }

    impl std::fmt::Display for SseError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                SseError::Connection(msg) => write!(f, "SSE connection error: {}", msg),
                SseError::Parse(msg) => write!(f, "SSE parse error: {}", msg),
                SseError::Closed => write!(f, "SSE stream closed"),
            }
        }
    }

    impl std::error::Error for SseError {}

    /// A client-side SSE stream that deserializes JSON events.
    ///
    /// This stream connects to an SSE endpoint and automatically deserializes
    /// incoming JSON events into the specified type.
    pub struct SseStream<T> {
        #[allow(dead_code)]
        source: EventSource,
        subscription: EventSourceSubscription,
        _phantom: std::marker::PhantomData<T>,
    }

    impl<T> SseStream<T> {
        /// Connect to an SSE endpoint.
        ///
        /// Returns a stream that yields deserialized events from the server.
        pub fn connect(url: &str) -> Result<Self, SseError> {
            let mut source =
                EventSource::new(url).map_err(|e| SseError::Connection(format!("{:?}", e)))?;

            let subscription = source
                .subscribe("message")
                .map_err(|e| SseError::Connection(format!("{:?}", e)))?;

            Ok(Self {
                source,
                subscription,
                _phantom: std::marker::PhantomData,
            })
        }
    }

    impl<T: DeserializeOwned + Unpin> Stream for SseStream<T> {
        type Item = Result<T, SseError>;

        fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            match Pin::new(&mut self.subscription).poll_next(cx) {
                Poll::Ready(Some(Ok((_, msg)))) => {
                    let data = msg
                        .data()
                        .dyn_into::<js_sys::JsString>()
                        .map(String::from)
                        .unwrap_or_default();

                    match serde_json::from_str(&data) {
                        Ok(value) => Poll::Ready(Some(Ok(value))),
                        Err(e) => Poll::Ready(Some(Err(SseError::Parse(e.to_string())))),
                    }
                }
                Poll::Ready(Some(Err(e))) => {
                    Poll::Ready(Some(Err(SseError::Connection(format!("{:?}", e)))))
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

    #[test]
    fn event_new_creates_empty_event() {
        let event = Event::new();
        // Should not panic
        let _: axum::response::sse::Event = event.into();
    }

    #[test]
    fn event_default_creates_empty_event() {
        let event = Event::default();
        let _: axum::response::sse::Event = event.into();
    }

    #[test]
    fn event_json_data_serializes_primitives() {
        let event = Event::new().json_data(42).unwrap();
        let _: axum::response::sse::Event = event.into();
    }

    #[test]
    fn event_json_data_serializes_structs() {
        #[derive(serde::Serialize)]
        struct TestData {
            name: String,
            value: i32,
        }

        let data = TestData {
            name: "test".into(),
            value: 123,
        };
        let event = Event::new().json_data(data).unwrap();
        let _: axum::response::sse::Event = event.into();
    }

    #[test]
    fn event_data_sets_raw_string() {
        let event = Event::new().data("raw data");
        let _: axum::response::sse::Event = event.into();
    }

    #[test]
    fn event_with_name() {
        let event = Event::new().event("custom-event").data("payload");
        let _: axum::response::sse::Event = event.into();
    }

    #[test]
    fn event_with_id() {
        let event = Event::new().id("msg-123").data("payload");
        let _: axum::response::sse::Event = event.into();
    }

    #[test]
    fn event_with_retry() {
        use std::time::Duration;
        let event = Event::new().retry(Duration::from_secs(5)).data("payload");
        let _: axum::response::sse::Event = event.into();
    }

    #[test]
    fn event_with_comment() {
        let event = Event::new().comment("this is a comment").data("payload");
        let _: axum::response::sse::Event = event.into();
    }

    #[test]
    fn event_chaining() {
        let event = Event::new()
            .event("update")
            .id("1")
            .json_data(serde_json::json!({"count": 42}))
            .unwrap()
            .comment("status update");
        let _: axum::response::sse::Event = event.into();
    }
}
