use eframe::App;
use egui::Context;

#[derive(Default)]
pub struct SimpleApp {
    name: String,
    age: u32,
}

impl App for SimpleApp {
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Simple Egui App");
            ui.horizontal(|ui| {
                ui.label("Name: ");
                ui.text_edit_singleline(&mut self.name);
            });
            ui.horizontal(|ui| {
                ui.label("Age: ");
                ui.add(egui::Slider::new(&mut self.age, 0..=120));
            });
            if ui.button("Reset").clicked() {
                self.name.clear();
                self.age = 0;
            }
        });
    }
} 