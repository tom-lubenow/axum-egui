use crate::gui::state::TodoState;
use eframe::egui;

pub fn todo_list(ui: &mut egui::Ui, state: &mut TodoState) {
    let mut to_toggle = None;
    let mut to_remove = None;

    for (index, item) in state.items.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let mut completed = item.completed;
            if ui.checkbox(&mut completed, "").clicked() {
                to_toggle = Some(index);
            }

            let text = if item.completed {
                egui::RichText::new(&item.text).strikethrough()
            } else {
                egui::RichText::new(&item.text)
            };
            ui.label(text);

            if ui.small_button("🗑").clicked() {
                to_remove = Some(index);
            }
        });
    }

    if let Some(index) = to_toggle {
        state.toggle_item(index);
    }

    if let Some(index) = to_remove {
        state.remove_item(index);
    }
}
