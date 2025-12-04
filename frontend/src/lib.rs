//! Frontend WASM entry point for axum-egui.

use eframe::wasm_bindgen::{self, prelude::*};
use server_fn::prelude::*;
use std::sync::mpsc::{channel, Receiver, Sender};

// ============================================================================
// Server Functions - RPC (same signatures as backend)
// ============================================================================

#[server]
pub async fn greet(name: String) -> Result<String, ServerFnError> {
    // This body is only used on the server; on WASM it generates HTTP call
    unreachable!()
}

#[server]
pub async fn add(a: i32, b: i32) -> Result<i32, ServerFnError> {
    unreachable!()
}

// ============================================================================
// Server Functions - SSE (same signatures as backend)
// ============================================================================

#[server(sse)]
pub async fn counter() -> impl Stream<Item = i32> {
    unreachable!()
}

// ============================================================================
// Server Functions - WebSocket (same signatures as backend)
// ============================================================================

#[server(ws)]
pub async fn echo(incoming: impl Stream<Item = String>) -> impl Stream<Item = String> {
    unreachable!()
}

// ============================================================================
// App State
// ============================================================================

/// Responses from API calls.
enum ApiResponse {
    Greet(Result<String, ServerFnError>),
    Add(Result<i32, ServerFnError>),
}

/// The example app state.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ExampleApp {
    pub label: String,
    pub value: f32,
    #[serde(skip)]
    pub server_message: Option<String>,
    #[serde(skip)]
    pub add_result: Option<i32>,
    #[serde(skip)]
    response_rx: Option<Receiver<ApiResponse>>,
    #[serde(skip)]
    response_tx: Option<Sender<ApiResponse>>,
    // SSE state
    #[serde(skip)]
    counter_stream: Option<SseStream<i32>>,
    #[serde(skip)]
    counter_value: Option<i32>,
    #[serde(skip)]
    counter_connected: bool,
    // WebSocket state
    #[serde(skip)]
    ws_echo: Option<WsStream<String, String>>,
    #[serde(skip)]
    ws_input: String,
    #[serde(skip)]
    ws_messages: Vec<String>,
    #[serde(skip)]
    ws_connected: bool,
}

impl Default for ExampleApp {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            label: "Hello World!".to_owned(),
            value: 2.7,
            server_message: None,
            add_result: None,
            response_rx: Some(rx),
            response_tx: Some(tx),
            counter_stream: None,
            counter_value: None,
            counter_connected: false,
            ws_echo: None,
            ws_input: String::new(),
            ws_messages: Vec::new(),
            ws_connected: false,
        }
    }
}

impl ExampleApp {
    /// Call the greet server function.
    fn call_greet(&self, name: String) {
        if let Some(tx) = &self.response_tx {
            let tx = tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = greet(name).await;
                let _ = tx.send(ApiResponse::Greet(result));
            });
        }
    }

    /// Call the add server function.
    fn call_add(&self, a: i32, b: i32) {
        if let Some(tx) = &self.response_tx {
            let tx = tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = add(a, b).await;
                let _ = tx.send(ApiResponse::Add(result));
            });
        }
    }

    /// Start the counter SSE stream.
    fn start_counter(&mut self) {
        if self.counter_stream.is_none() {
            self.counter_stream = Some(counter());
            self.counter_connected = false;
        }
    }

    /// Stop the counter SSE stream.
    fn stop_counter(&mut self) {
        self.counter_stream = None;
        self.counter_connected = false;
    }

    /// Connect the WebSocket echo.
    fn connect_ws(&mut self) {
        if self.ws_echo.is_none() {
            self.ws_echo = Some(echo());
            self.ws_connected = false;
        }
    }

    /// Disconnect the WebSocket.
    fn disconnect_ws(&mut self) {
        self.ws_echo = None;
        self.ws_connected = false;
    }

    /// Send a message through the WebSocket.
    fn send_ws(&mut self) {
        if let Some(ws) = &self.ws_echo {
            if !self.ws_input.is_empty() {
                ws.send(self.ws_input.clone());
                self.ws_messages.push(format!("> {}", self.ws_input));
                self.ws_input.clear();
            }
        }
    }

    /// Process any pending API responses.
    fn process_responses(&mut self) {
        if let Some(rx) = &self.response_rx {
            while let Ok(response) = rx.try_recv() {
                match response {
                    ApiResponse::Greet(Ok(msg)) => self.server_message = Some(msg),
                    ApiResponse::Greet(Err(e)) => {
                        log::error!("Greet error: {e}");
                        self.server_message = Some(format!("Error: {e}"));
                    }
                    ApiResponse::Add(Ok(result)) => self.add_result = Some(result),
                    ApiResponse::Add(Err(e)) => {
                        log::error!("Add error: {e}");
                        self.server_message = Some(format!("Error: {e}"));
                    }
                }
            }
        }

        // Process SSE events
        if let Some(stream) = &mut self.counter_stream {
            // Update connection state
            self.counter_connected = stream.is_connected();

            // Get all pending events
            for value in stream.try_iter() {
                self.counter_value = Some(value);
            }
        }

        // Process WebSocket events
        if let Some(ws) = &mut self.ws_echo {
            self.ws_connected = ws.is_connected();

            for msg in ws.try_iter() {
                self.ws_messages.push(format!("< {}", msg));
                // Keep only last 20 messages
                if self.ws_messages.len() > 20 {
                    self.ws_messages.remove(0);
                }
            }
        }
    }
}

impl eframe::App for ExampleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process any pending API responses
        self.process_responses();

        // Request continuous repaints while SSE or WebSocket is active
        if self.counter_stream.is_some() || self.ws_echo.is_some() {
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("axum-egui example");

            // RPC Section
            ui.group(|ui| {
                ui.label("RPC Functions");
                ui.horizontal(|ui| {
                    ui.label("Your name: ");
                    ui.text_edit_singleline(&mut self.label);
                });

                ui.horizontal(|ui| {
                    if ui.button("Greet from Server").clicked() {
                        self.call_greet(self.label.clone());
                    }
                    if ui.button("Add 10 + 32").clicked() {
                        self.call_add(10, 32);
                    }
                });

                if let Some(msg) = &self.server_message {
                    ui.label(format!("Server says: {msg}"));
                }
                if let Some(result) = &self.add_result {
                    ui.label(format!("Add result: {result}"));
                }
            });

            ui.add_space(10.0);

            // SSE Section
            ui.group(|ui| {
                ui.label("SSE Stream (Server-Sent Events)");

                ui.horizontal(|ui| {
                    let is_running = self.counter_stream.is_some();

                    if !is_running {
                        if ui.button("Start Counter Stream").clicked() {
                            self.start_counter();
                        }
                    } else {
                        if ui.button("Stop Counter Stream").clicked() {
                            self.stop_counter();
                        }
                    }

                    // Show connection status
                    if is_running {
                        let status = if self.counter_connected {
                            "Connected"
                        } else {
                            "Connecting..."
                        };
                        ui.label(format!("Status: {status}"));
                    }
                });

                if let Some(value) = self.counter_value {
                    ui.label(format!("Counter: {value}"));
                    // Visual progress bar
                    ui.add(egui::ProgressBar::new(value as f32 / 100.0).text(format!("{value}/100")));
                }
            });

            ui.add_space(10.0);

            // WebSocket Section
            ui.group(|ui| {
                ui.label("WebSocket Echo (Bidirectional)");

                ui.horizontal(|ui| {
                    let is_connected = self.ws_echo.is_some();

                    if !is_connected {
                        if ui.button("Connect").clicked() {
                            self.connect_ws();
                        }
                    } else {
                        if ui.button("Disconnect").clicked() {
                            self.disconnect_ws();
                        }
                    }

                    // Show connection status
                    if is_connected {
                        let status = if self.ws_connected {
                            "Connected"
                        } else {
                            "Connecting..."
                        };
                        ui.label(format!("Status: {status}"));
                    }
                });

                // Message input and send
                if self.ws_echo.is_some() {
                    ui.horizontal(|ui| {
                        let response = ui.text_edit_singleline(&mut self.ws_input);
                        if ui.button("Send").clicked() || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                            self.send_ws();
                        }
                    });

                    // Message log
                    egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
                        for msg in &self.ws_messages {
                            ui.monospace(msg);
                        }
                    });
                }
            });

            ui.add_space(10.0);

            ui.add(egui::Slider::new(&mut self.value, 0.0..=10.0).text("value"));
            if ui.button("Increment").clicked() {
                self.value += 1.0;
            }

            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label("Powered by ");
                    ui.hyperlink_to("egui", "https://github.com/emilk/egui");
                    ui.label(" and ");
                    ui.hyperlink_to("axum", "https://github.com/tokio-rs/axum");
                    ui.label(".");
                });
                egui::warn_if_debug_build(ui);
            });
        });
    }
}

// ============================================================================
// WASM Entry Point
// ============================================================================

#[wasm_bindgen(start)]
pub fn main() {
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        // Try to read initial state from the DOM
        let mut initial_state: ExampleApp = read_initial_state(&document).unwrap_or_default();

        // Set up the channel for API responses
        let (tx, rx) = channel();
        initial_state.response_tx = Some(tx);
        initial_state.response_rx = Some(rx);

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("Not a canvas element");

        let web_options = eframe::WebOptions::default();

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(move |_cc| Ok(Box::new(initial_state))),
            )
            .await;

        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p>The app has crashed. See the developer console for details.</p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });
}

fn read_initial_state<T: serde::de::DeserializeOwned>(document: &web_sys::Document) -> Option<T> {
    let script = document.get_element_by_id("axum-egui-state")?;
    let json = script.text_content()?;
    serde_json::from_str(&json).ok()
}
