//! Frontend egui application.
//!
//! This crate compiles to WASM and runs in the browser.

use axum_egui::ws::{WsClientSender, WsStream};
use basic_shared::AppState;
use basic_shared::api::{self, ApiError};
use futures_util::StreamExt;
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
    Greet(Result<String, ApiError>),
    Add(Result<i32, ApiError>),
    Whoami(Result<api::WhoamiResponse, ApiError>),
    WsMessage(String),
    WsConnected(WsClientSender<String>),
    WsError(String),
}

pub struct ExampleApp {
    // State from server
    label: String,
    value: f32,
    server_message: Option<String>,

    // Local state
    add_result: Option<i32>,
    whoami_result: Option<api::WhoamiResponse>,
    response_rx: Receiver<ApiResponse>,
    response_tx: Sender<ApiResponse>,

    // WebSocket state
    ws_input: String,
    ws_messages: Vec<String>,
    ws_sender: Option<WsClientSender<String>>,
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
            whoami_result: None,
            response_rx: rx,
            response_tx: tx,
            ws_input: String::new(),
            ws_messages: Vec::new(),
            ws_sender: None,
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

    fn call_whoami(&self) {
        let tx = self.response_tx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = api::whoami().await;
            let _ = tx.send(ApiResponse::Whoami(result));
        });
    }

    fn connect_websocket(&self) {
        let tx = self.response_tx.clone();
        wasm_bindgen_futures::spawn_local(async move {
            // Build WebSocket URL from current origin
            let window = web_sys::window().expect("no window");
            let location = window.location();
            let protocol = if location.protocol().unwrap_or_default() == "https:" {
                "wss:"
            } else {
                "ws:"
            };
            let host = location.host().unwrap_or_else(|_| "localhost:3000".into());
            let url = format!("{}//{}/api/ws", protocol, host);

            match WsStream::<String, String>::connect(&url).await {
                Ok((sender, mut receiver)) => {
                    let _ = tx.send(ApiResponse::WsConnected(sender));

                    // Spawn a task to receive messages
                    let tx_recv = tx.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        while let Some(result) = receiver.next().await {
                            match result {
                                Ok(msg) => {
                                    let _ = tx_recv.send(ApiResponse::WsMessage(msg));
                                }
                                Err(e) => {
                                    let _ = tx_recv.send(ApiResponse::WsError(format!("{:?}", e)));
                                    break;
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    let _ = tx.send(ApiResponse::WsError(format!("Connect error: {:?}", e)));
                }
            }
        });
    }

    fn send_ws_message(&self, msg: String) {
        if let Some(sender) = &self.ws_sender
            && let Err(e) = sender.send(msg)
        {
            log::error!("Failed to send WebSocket message: {:?}", e);
        }
    }

    fn process_responses(&mut self) {
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
                ApiResponse::Whoami(Ok(result)) => self.whoami_result = Some(result),
                ApiResponse::Whoami(Err(e)) => {
                    log::error!("Whoami error: {e}");
                    self.server_message = Some(format!("Error: {e}"));
                }
                ApiResponse::WsConnected(sender) => {
                    self.ws_sender = Some(sender);
                    self.ws_connected = true;
                    self.ws_messages.push("Connected to WebSocket!".into());
                }
                ApiResponse::WsMessage(msg) => {
                    self.ws_messages.push(msg);
                }
                ApiResponse::WsError(e) => {
                    log::error!("WebSocket error: {e}");
                    self.ws_messages.push(format!("Error: {e}"));
                    self.ws_connected = false;
                    self.ws_sender = None;
                }
            }
        }
    }
}

impl eframe::App for ExampleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.process_responses();

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("axum-egui example");

            // RPC Section
            ui.group(|ui| {
                ui.label("API Functions");

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
                    if ui.button("Whoami").clicked() {
                        self.call_whoami();
                    }
                });

                if let Some(msg) = &self.server_message {
                    ui.label(format!("Server says: {msg}"));
                }
                if let Some(result) = &self.add_result {
                    ui.label(format!("Add result: {result}"));
                }
                if let Some(whoami) = &self.whoami_result {
                    ui.label(format!(
                        "Whoami: {} (at {})",
                        whoami.message, whoami.timestamp
                    ));
                }
            });

            ui.add_space(10.0);

            ui.add(egui::Slider::new(&mut self.value, 0.0..=10.0).text("value"));
            if ui.button("Increment").clicked() {
                self.value += 1.0;
            }

            ui.add_space(20.0);

            ui.group(|ui| {
                ui.label("Initial state received from server:");
                ui.monospace(format!("label: {}", self.label));
                ui.monospace(format!("value: {}", self.value));
            });

            ui.add_space(10.0);

            // WebSocket Section
            ui.group(|ui| {
                ui.label("WebSocket Echo");

                ui.horizontal(|ui| {
                    if self.ws_connected {
                        ui.label("Status: Connected");
                    } else if ui.button("Connect").clicked() {
                        self.connect_websocket();
                    }
                });

                if self.ws_connected {
                    ui.horizontal(|ui| {
                        ui.label("Message: ");
                        let response = ui.text_edit_singleline(&mut self.ws_input);
                        if (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                            || ui.button("Send").clicked()
                        {
                            let msg = std::mem::take(&mut self.ws_input);
                            if !msg.is_empty() {
                                self.send_ws_message(msg);
                            }
                        }
                    });
                }

                // Show recent messages (last 5)
                if !self.ws_messages.is_empty() {
                    ui.separator();
                    ui.label("Messages:");
                    for msg in self.ws_messages.iter().rev().take(5).rev() {
                        ui.monospace(msg);
                    }
                }
            });

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
