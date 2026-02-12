//! Frontend egui application with co-located server functions.
//!
//! This crate can be compiled in two modes:
//! - `ssr` feature: Compiled as a native library for server function registration
//! - `hydrate` feature: Compiled to WASM and runs in the browser

pub mod api;
pub mod state;

pub use api::*;
pub use state::*;

// ============================================================================
// WASM Entry Point (hydrate feature only)
// ============================================================================

#[cfg(feature = "hydrate")]
mod app {
    use crate::api;
    use crate::state::AppState;
    use axum_egui::ServerFnError;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(start)]
    pub fn main() {
        eframe::WebLogger::init(log::LevelFilter::Debug).ok();

        wasm_bindgen_futures::spawn_local(async {
            let document = web_sys::window()
                .expect("No window")
                .document()
                .expect("No document");

            let initial_state: AppState = read_initial_state(&document).unwrap_or_default();

            let canvas = document
                .get_element_by_id("the_canvas_id")
                .expect("Failed to find canvas")
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .expect("Not a canvas element");

            let web_options = eframe::WebOptions::default();
            let app = CounterApp::new(initial_state);

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

    fn read_initial_state<T: serde::de::DeserializeOwned>(
        document: &web_sys::Document,
    ) -> Option<T> {
        let script = document.get_element_by_id("axum-egui-state")?;
        let json = script.text_content()?;
        serde_json::from_str(&json).ok()
    }

    // ============================================================================
    // Counter App
    // ============================================================================

    enum ApiResponse {
        Increment(Result<i32, ServerFnError>),
        Greet(Result<String, ServerFnError>),
    }

    pub struct CounterApp {
        counter: i32,
        name: String,
        greeting: Option<String>,
        response_rx: Receiver<ApiResponse>,
        response_tx: Sender<ApiResponse>,
    }

    impl CounterApp {
        pub fn new(state: AppState) -> Self {
            let (tx, rx) = channel();
            Self {
                counter: state.counter,
                name: String::new(),
                greeting: if state.message.is_empty() {
                    None
                } else {
                    Some(state.message)
                },
                response_rx: rx,
                response_tx: tx,
            }
        }

        fn call_increment(&self, value: i32) {
            let tx = self.response_tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = api::increment(value).await;
                let _ = tx.send(ApiResponse::Increment(result));
            });
        }

        fn call_greet(&self, name: String) {
            let tx = self.response_tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = api::greet(name).await;
                let _ = tx.send(ApiResponse::Greet(result));
            });
        }

        fn process_responses(&mut self) {
            while let Ok(response) = self.response_rx.try_recv() {
                match response {
                    ApiResponse::Increment(Ok(value)) => self.counter = value,
                    ApiResponse::Increment(Err(e)) => {
                        log::error!("Increment error: {e}");
                    }
                    ApiResponse::Greet(Ok(msg)) => self.greeting = Some(msg),
                    ApiResponse::Greet(Err(e)) => {
                        log::error!("Greet error: {e}");
                        self.greeting = Some(format!("Error: {e}"));
                    }
                }
            }
        }
    }

    impl eframe::App for CounterApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.process_responses();

            egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    egui::widgets::global_theme_preference_buttons(ui);
                });
            });

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("Counter App");

                ui.add_space(10.0);

                // Counter section
                ui.group(|ui| {
                    ui.label(format!("Counter: {}", self.counter));
                    if ui.button("Increment (server)").clicked() {
                        self.call_increment(self.counter);
                    }
                });

                ui.add_space(10.0);

                // Greet section
                ui.group(|ui| {
                    ui.label("Server Greeting");
                    ui.horizontal(|ui| {
                        ui.label("Your name: ");
                        ui.text_edit_singleline(&mut self.name);
                    });
                    if ui.button("Greet from Server").clicked() {
                        self.call_greet(self.name.clone());
                    }
                    if let Some(msg) = &self.greeting {
                        ui.label(msg.as_str());
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
                });
            });
        }
    }
}
