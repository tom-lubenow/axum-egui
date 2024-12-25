use crate::gui::state::TodoState;
use eframe::egui;

pub fn todo_input(ui: &mut egui::Ui, state: &mut TodoState) {
    ui.horizontal(|ui| {
        let text_edit = ui.text_edit_singleline(&mut state.new_item_text);
        if ui.button("Add").clicked()
            || text_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            state.add_item();
        }
    });
}
