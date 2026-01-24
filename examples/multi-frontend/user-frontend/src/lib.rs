//! User frontend - A simple counter app.
//!
//! This crate can be compiled in two modes:
//! - `ssr` feature: Compiled as a native library for type sharing
//! - `hydrate` feature: Compiled to WASM and runs in the browser

use serde::{Deserialize, Serialize};

/// User app state - defined here to satisfy orphan rules.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserApp {
    pub counter: i32,
    pub username: Option<String>,
}

// ============================================================================
// WASM Entry Point (hydrate feature only)
// ============================================================================

#[cfg(feature = "hydrate")]
mod app {
    use super::UserApp;
    use eframe::egui;
    use wasm_bindgen::prelude::*;

    impl eframe::App for UserApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.heading("User Counter App");
                ui.separator();

                ui.horizontal(|ui| {
                    ui.label("Counter:");
                    ui.label(format!("{}", self.counter));
                });

                ui.horizontal(|ui| {
                    if ui.button("-").clicked() {
                        self.counter -= 1;
                    }
                    if ui.button("+").clicked() {
                        self.counter += 1;
                    }
                });

                if let Some(username) = &self.username {
                    ui.label(format!("Logged in as: {}", username));
                }

                ui.separator();
                ui.small("This is the user-facing frontend.");
            });
        }
    }

    /// Get initial state from the server-injected JSON.
    fn get_initial_state() -> UserApp {
        let window = web_sys::window().expect("no window");
        let document = window.document().expect("no document");

        document
            .get_element_by_id("axum-egui-state")
            .and_then(|el| el.text_content())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default()
    }

    #[wasm_bindgen(start)]
    pub fn main() {
        eframe::WebLogger::init(log::LevelFilter::Debug).ok();

        wasm_bindgen_futures::spawn_local(async {
            let document = web_sys::window()
                .expect("No window")
                .document()
                .expect("No document");

            let canvas = document
                .get_element_by_id("the_canvas_id")
                .expect("Failed to find canvas")
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .expect("Not a canvas element");

            let app = get_initial_state();

            let start_result = eframe::WebRunner::new()
                .start(
                    canvas,
                    eframe::WebOptions::default(),
                    Box::new(move |_cc| Ok(Box::new(app))),
                )
                .await;

            if let Some(loading_text) = document.get_element_by_id("loading_text") {
                match start_result {
                    Ok(_) => loading_text.remove(),
                    Err(e) => {
                        loading_text.set_inner_html("<p>App crashed. See console.</p>");
                        panic!("Failed to start eframe: {e:?}");
                    }
                }
            }
        });
    }
}
