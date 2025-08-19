//! Frontend WASM entry point for axum-egui.

use eframe::wasm_bindgen::{self, prelude::*};
use serde::{Deserialize, Serialize};

/// The example app state - shared between server and client.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExampleApp {
    pub label: String,
    pub value: f32,
}

impl Default for ExampleApp {
    fn default() -> Self {
        Self {
            label: "Hello World!".to_owned(),
            value: 2.7,
        }
    }
}

impl eframe::App for ExampleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                egui::widgets::global_theme_preference_buttons(ui);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("axum-egui example");

            ui.horizontal(|ui| {
                ui.label("Write something: ");
                ui.text_edit_singleline(&mut self.label);
            });

            ui.add(egui::Slider::new(&mut self.value, 0.0..=10.0).text("value"));
            if ui.button("Increment").clicked() {
                self.value += 1.0;
            }

            ui.separator();

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

/// WASM entry point - called from JavaScript.
#[wasm_bindgen(start)]
pub fn main() {
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        // Try to read initial state from the DOM
        let initial_state: ExampleApp = read_initial_state(&document).unwrap_or_default();

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

        // Remove the loading text
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

/// Read initial state from a script tag in the DOM.
fn read_initial_state<T: serde::de::DeserializeOwned>(document: &web_sys::Document) -> Option<T> {
    let script = document.get_element_by_id("axum-egui-state")?;
    let json = script.text_content()?;
    serde_json::from_str(&json).ok()
}
