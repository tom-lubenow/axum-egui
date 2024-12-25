#[derive(Default)]
pub struct TodoItem {
    pub text: String,
    pub completed: bool,
}

#[derive(Default)]
pub struct TodoState {
    pub items: Vec<TodoItem>,
    pub new_item_text: String,
}

impl TodoState {
    pub fn add_item(&mut self) {
        if !self.new_item_text.is_empty() {
            self.items.push(TodoItem {
                text: self.new_item_text.clone(),
                completed: false,
            });
            self.new_item_text.clear();
        }
    }

    pub fn remove_item(&mut self, index: usize) {
        self.items.remove(index);
    }

    pub fn toggle_item(&mut self, index: usize) {
        if let Some(item) = self.items.get_mut(index) {
            item.completed = !item.completed;
        }
    }
}
