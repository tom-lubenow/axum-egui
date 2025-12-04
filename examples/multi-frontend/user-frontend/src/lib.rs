//! User frontend - A simple counter app.

use eframe::egui;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

/// User app state - must match the backend's UserApp struct.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserApp {
    pub counter: i32,
    pub username: Option<String>,
}

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

    if let Some(element) = document.get_element_by_id("axum-egui-state") {
        if let Some(json) = element.text_content() {
            if let Ok(state) = serde_json::from_str(&json) {
                return state;
            }
        }
    }

    UserApp::default()
}

#[wasm_bindgen(start)]
pub fn main() {
    let window = web_sys::window().expect("no window");
    let document = window.document().expect("no document");

    // Hide loading text
    if let Some(loading) = document.get_element_by_id("loading_text") {
        let _ = loading.set_attribute("style", "display: none;");
    }

    // Get the canvas element
    let canvas = document
        .get_element_by_id("the_canvas_id")
        .expect("no canvas element")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("not a canvas element");

    let app = get_initial_state();

    let options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async move {
        eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(|_cc| Ok(Box::new(app))),
            )
            .await
            .expect("failed to start eframe");
    });
}
