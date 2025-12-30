//! Frontend egui application.
//!
//! This crate compiles to WASM and runs in the browser.

use basic_shared::{AppState, api};
use server_fn::prelude::*;
use std::sync::mpsc::{Receiver, Sender, channel};
use wasm_bindgen::prelude::*;

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
        let initial_state: AppState = read_initial_state(&document).unwrap_or_default();

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("Not a canvas element");

        let web_options = eframe::WebOptions::default();

        // Create the app with initial state
        let app = ExampleApp::new(initial_state);

        let start_result = eframe::WebRunner::new()
            .start(canvas, web_options, Box::new(move |_cc| Ok(Box::new(app))))
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

// ============================================================================
// Example App
// ============================================================================

enum ApiResponse {
    Greet(Result<String, ServerFnError>),
    Add(Result<i32, ServerFnError>),
}

pub struct ExampleApp {
    // State from server
    label: String,
    value: f32,
    server_message: Option<String>,

    // Local state
    add_result: Option<i32>,
    response_rx: Receiver<ApiResponse>,
    response_tx: Sender<ApiResponse>,

    // SSE state
    counter_stream: Option<server_fn::sse::SseStream<i32>>,
    counter_value: Option<i32>,
    counter_connected: bool,

    // WebSocket state
    ws_echo: Option<server_fn::ws::WsStream<String, String>>,
    ws_input: String,
    ws_messages: Vec<String>,
    ws_connected: bool,
}

impl ExampleApp {
    pub fn new(state: AppState) -> Self {
        let (tx, rx) = channel();
        Self {
            label: state.label,
            value: state.value,
            server_message: state.server_message,
            add_result: None,
            response_rx: rx,
            response_tx: tx,
            counter_stream: None,
            counter_value: None,
            counter_connected: false,
            ws_echo: None,
            ws_input: String::new(),
            ws_messages: Vec::new(),
            ws_connected: false,
        }
    }

    fn call_greet(&self, name: String) {
        let tx = self.response_tx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = api::greet(name).await;
            let _ = tx.send(ApiResponse::Greet(result));
        });
    }

    fn call_add(&self, a: i32, b: i32) {
        let tx = self.response_tx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = api::add(a, b).await;
            let _ = tx.send(ApiResponse::Add(result));
        });
    }

    fn start_counter(&mut self) {
        if self.counter_stream.is_none() {
            self.counter_stream = Some(api::counter());
            self.counter_connected = false;
        }
    }

    fn stop_counter(&mut self) {
        self.counter_stream = None;
        self.counter_connected = false;
    }

    fn connect_ws(&mut self) {
        if self.ws_echo.is_none() {
            self.ws_echo = Some(api::echo());
            self.ws_connected = false;
        }
    }

    fn disconnect_ws(&mut self) {
        self.ws_echo = None;
        self.ws_connected = false;
    }

    fn send_ws(&mut self) {
        if let Some(ws) = &self.ws_echo {
            if !self.ws_input.is_empty() {
                ws.send(self.ws_input.clone());
                self.ws_messages.push(format!("> {}", self.ws_input));
                self.ws_input.clear();
            }
        }
    }

    fn process_responses(&mut self) {
        // Process RPC responses
        while let Ok(response) = self.response_rx.try_recv() {
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

        // Process SSE events
        if let Some(stream) = &mut self.counter_stream {
            self.counter_connected = stream.is_connected();
            for value in stream.try_iter() {
                self.counter_value = Some(value);
            }
        }

        // Process WebSocket events
        if let Some(ws) = &mut self.ws_echo {
            self.ws_connected = ws.is_connected();
            for msg in ws.try_iter() {
                self.ws_messages.push(format!("< {}", msg));
                if self.ws_messages.len() > 20 {
                    self.ws_messages.remove(0);
                }
            }
        }
    }
}

impl eframe::App for ExampleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_responses();

        // Request continuous repaints while SSE or WebSocket is active
        if self.counter_stream.is_some() || self.ws_echo.is_some() {
            ctx.request_repaint();
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
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
                    } else if ui.button("Stop Counter Stream").clicked() {
                        self.stop_counter();
                    }

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
                    ui.add(
                        egui::ProgressBar::new(value as f32 / 100.0).text(format!("{value}/100")),
                    );
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
                    } else if ui.button("Disconnect").clicked() {
                        self.disconnect_ws();
                    }

                    if is_connected {
                        let status = if self.ws_connected {
                            "Connected"
                        } else {
                            "Connecting..."
                        };
                        ui.label(format!("Status: {status}"));
                    }
                });

                if self.ws_echo.is_some() {
                    ui.horizontal(|ui| {
                        let response = ui.text_edit_singleline(&mut self.ws_input);
                        if ui.button("Send").clicked()
                            || (response.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            self.send_ws();
                        }
                    });

                    egui::ScrollArea::vertical()
                        .max_height(100.0)
                        .show(ui, |ui| {
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
