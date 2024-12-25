use super::components::{todo_input, todo_list};
use super::state::TodoState;
use eframe::egui;

#[derive(Default)]
pub struct App {
    state: TodoState,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Todo List");
            ui.add_space(8.0);

            todo_input(ui, &mut self.state);
            ui.add_space(16.0);

            todo_list(ui, &mut self.state);
        });
    }
}
