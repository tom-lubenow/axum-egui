//! Admin frontend - Dashboard with server stats.

use eframe::egui;
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

/// Admin app state - defined here to satisfy orphan rules.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AdminApp {
    pub total_users: i32,
    pub active_sessions: i32,
    pub server_uptime_secs: u64,
}

impl AdminApp {
    fn format_uptime(&self) -> String {
        let hours = self.server_uptime_secs / 3600;
        let minutes = (self.server_uptime_secs % 3600) / 60;
        let seconds = self.server_uptime_secs % 60;
        format!("{}h {}m {}s", hours, minutes, seconds)
    }
}

impl eframe::App for AdminApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Admin Dashboard");
            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Total Users:");
                ui.strong(format!("{}", self.total_users));
            });

            ui.horizontal(|ui| {
                ui.label("Active Sessions:");
                ui.strong(format!("{}", self.active_sessions));
            });

            ui.horizontal(|ui| {
                ui.label("Server Uptime:");
                ui.strong(self.format_uptime());
            });

            ui.separator();
            ui.colored_label(egui::Color32::YELLOW, "Admin Panel - Restricted Access");
        });
    }
}

/// Get initial state from the server-injected JSON.
fn get_initial_state() -> AdminApp {
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
