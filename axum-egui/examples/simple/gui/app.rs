use eframe::egui;

#[derive(Default)]
pub struct App {
    name: String,
    age: u32,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("My egui Application");

            ui.horizontal(|ui| {
                ui.label(format!("Your name: {}", self.name));
                ui.text_edit_singleline(&mut self.name);
            });

            ui.horizontal(|ui| {
                ui.label(format!("Your age: {}", self.age));
                ui.add(egui::DragValue::new(&mut self.age));
            });

            if ui.button("Reset").clicked() {
                self.name.clear();
                self.age = 0;
            }
            if ui.button("Increment").clicked() {
                self.age += 1;
            }
        });
    }
}

