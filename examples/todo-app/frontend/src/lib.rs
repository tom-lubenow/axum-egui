//! Todo app frontend with co-located server functions.
//!
//! This crate can be compiled in two modes:
//! - `ssr` feature: Compiled as a native library for server function registration
//! - `hydrate` feature: Compiled to WASM and runs in the browser
//!
//! Demonstrates:
//! - Token-based authentication (login/logout)
//! - CRUD operations (add, toggle, delete todos)
//! - Async API calls with channel-based response handling

pub mod api;
pub mod state;

pub use api::*;
pub use state::*;

// ============================================================================
// WASM Entry Point (hydrate feature only)
// ============================================================================

#[cfg(feature = "hydrate")]
mod app {
    use crate::api::{self, LoginResponse};
    use crate::state::{AppState, Todo};
    use axum_egui::ServerFnError;
    use std::sync::mpsc::{Receiver, Sender, channel};
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(start)]
    pub fn main() {
        eframe::WebLogger::init(log::LevelFilter::Debug).ok();

        wasm_bindgen_futures::spawn_local(async {
            let document = web_sys::window()
                .expect("No window")
                .document()
                .expect("No document");

            // Try to read initial state from the DOM
            let initial_state: AppState = read_initial_state(&document).unwrap_or_default();

            let canvas = document
                .get_element_by_id("the_canvas_id")
                .expect("Failed to find canvas")
                .dyn_into::<web_sys::HtmlCanvasElement>()
                .expect("Not a canvas element");

            let web_options = eframe::WebOptions::default();

            let app = TodoApp::new(initial_state);

            let start_result = eframe::WebRunner::new()
                .start(canvas, web_options, Box::new(move |_cc| Ok(Box::new(app))))
                .await;

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

    fn read_initial_state<T: serde::de::DeserializeOwned>(
        document: &web_sys::Document,
    ) -> Option<T> {
        let script = document.get_element_by_id("axum-egui-state")?;
        let json = script.text_content()?;
        serde_json::from_str(&json).ok()
    }

    // ========================================================================
    // API Response Types
    // ========================================================================

    enum ApiResponse {
        Login(Result<LoginResponse, ServerFnError>),
        GetTodos(Result<Vec<Todo>, ServerFnError>),
        AddTodo(Result<Todo, ServerFnError>),
        ToggleTodo(Result<Todo, ServerFnError>),
        DeleteTodo(u64, Result<(), ServerFnError>),
        Logout(Result<(), ServerFnError>),
    }

    // ========================================================================
    // Todo App
    // ========================================================================

    pub struct TodoApp {
        // Auth state
        username: Option<String>,
        token: Option<String>,
        login_username: String,
        login_password: String,
        login_error: Option<String>,

        // Todo state
        todos: Vec<Todo>,
        new_todo_title: String,
        error_message: Option<String>,

        // Async communication
        response_rx: Receiver<ApiResponse>,
        response_tx: Sender<ApiResponse>,
    }

    impl TodoApp {
        pub fn new(state: AppState) -> Self {
            let (tx, rx) = channel();
            Self {
                username: state.username,
                token: None,
                login_username: String::new(),
                login_password: String::new(),
                login_error: None,
                todos: state.todos,
                new_todo_title: String::new(),
                error_message: None,
                response_rx: rx,
                response_tx: tx,
            }
        }

        // ====================================================================
        // API Callers
        // ====================================================================

        fn call_login(&self, username: String, password: String) {
            let tx = self.response_tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                let result = api::login(username, password).await;
                let _ = tx.send(ApiResponse::Login(result));
            });
        }

        fn call_get_todos(&self) {
            if let Some(token) = &self.token {
                let tx = self.response_tx.clone();
                let token = token.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let result = api::get_todos(token).await;
                    let _ = tx.send(ApiResponse::GetTodos(result));
                });
            }
        }

        fn call_add_todo(&self, title: String) {
            if let Some(token) = &self.token {
                let tx = self.response_tx.clone();
                let token = token.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let result = api::add_todo(token, title).await;
                    let _ = tx.send(ApiResponse::AddTodo(result));
                });
            }
        }

        fn call_toggle_todo(&self, id: u64) {
            if let Some(token) = &self.token {
                let tx = self.response_tx.clone();
                let token = token.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let result = api::toggle_todo(token, id).await;
                    let _ = tx.send(ApiResponse::ToggleTodo(result));
                });
            }
        }

        fn call_delete_todo(&self, id: u64) {
            if let Some(token) = &self.token {
                let tx = self.response_tx.clone();
                let token = token.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let result = api::delete_todo(token, id).await;
                    let _ = tx.send(ApiResponse::DeleteTodo(id, result));
                });
            }
        }

        fn call_logout(&self) {
            if let Some(token) = &self.token {
                let tx = self.response_tx.clone();
                let token = token.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    let result = api::logout(token).await;
                    let _ = tx.send(ApiResponse::Logout(result));
                });
            }
        }

        // ====================================================================
        // Response Processing
        // ====================================================================

        fn process_responses(&mut self) {
            while let Ok(response) = self.response_rx.try_recv() {
                match response {
                    ApiResponse::Login(Ok(resp)) => {
                        self.username = Some(resp.username);
                        self.token = Some(resp.token);
                        self.login_error = None;
                        self.login_password.clear();
                        // Fetch todos after login
                        self.call_get_todos();
                    }
                    ApiResponse::Login(Err(e)) => {
                        log::error!("Login error: {e}");
                        self.login_error = Some(e.to_string());
                    }
                    ApiResponse::GetTodos(Ok(todos)) => {
                        self.todos = todos;
                        self.error_message = None;
                    }
                    ApiResponse::GetTodos(Err(e)) => {
                        log::error!("Get todos error: {e}");
                        self.error_message = Some(e.to_string());
                    }
                    ApiResponse::AddTodo(Ok(todo)) => {
                        self.todos.push(todo);
                        self.error_message = None;
                    }
                    ApiResponse::AddTodo(Err(e)) => {
                        log::error!("Add todo error: {e}");
                        self.error_message = Some(e.to_string());
                    }
                    ApiResponse::ToggleTodo(Ok(updated)) => {
                        if let Some(todo) = self.todos.iter_mut().find(|t| t.id == updated.id) {
                            todo.completed = updated.completed;
                        }
                        self.error_message = None;
                    }
                    ApiResponse::ToggleTodo(Err(e)) => {
                        log::error!("Toggle todo error: {e}");
                        self.error_message = Some(e.to_string());
                    }
                    ApiResponse::DeleteTodo(id, Ok(())) => {
                        self.todos.retain(|t| t.id != id);
                        self.error_message = None;
                    }
                    ApiResponse::DeleteTodo(_, Err(e)) => {
                        log::error!("Delete todo error: {e}");
                        self.error_message = Some(e.to_string());
                    }
                    ApiResponse::Logout(Ok(())) => {
                        self.username = None;
                        self.token = None;
                        self.todos.clear();
                        self.error_message = None;
                    }
                    ApiResponse::Logout(Err(e)) => {
                        log::error!("Logout error: {e}");
                        // Log out locally even if server call fails
                        self.username = None;
                        self.token = None;
                        self.todos.clear();
                    }
                }
            }
        }

        // ====================================================================
        // UI Rendering
        // ====================================================================

        fn render_login(&mut self, ui: &mut egui::Ui) {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.heading("Todo App - Login");
                ui.add_space(20.0);

                ui.set_max_width(300.0);

                ui.horizontal(|ui| {
                    ui.label("Username:");
                    ui.text_edit_singleline(&mut self.login_username);
                });

                ui.horizontal(|ui| {
                    ui.label("Password:");
                    let response =
                        ui.add(egui::TextEdit::singleline(&mut self.login_password).password(true));
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        let username = self.login_username.clone();
                        let password = self.login_password.clone();
                        self.call_login(username, password);
                    }
                });

                ui.add_space(10.0);

                if ui.button("Log in").clicked() {
                    let username = self.login_username.clone();
                    let password = self.login_password.clone();
                    self.call_login(username, password);
                }

                if let Some(err) = &self.login_error {
                    ui.add_space(10.0);
                    ui.colored_label(egui::Color32::RED, err);
                }

                ui.add_space(20.0);
                ui.label("Hint: any non-empty password works for this demo.");
            });
        }

        fn render_todos(&mut self, ui: &mut egui::Ui) {
            let completed = self.todos.iter().filter(|t| t.completed).count();
            let total = self.todos.len();

            // Header with user info and logout
            ui.horizontal(|ui| {
                if let Some(username) = &self.username {
                    ui.heading(format!("Hello, {}!", username));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Logout").clicked() {
                        self.call_logout();
                    }
                });
            });

            ui.separator();

            // Add new todo
            ui.horizontal(|ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.new_todo_title)
                        .hint_text("What needs to be done?")
                        .desired_width(ui.available_width() - 60.0),
                );
                let enter_pressed =
                    response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (ui.button("Add").clicked() || enter_pressed) && !self.new_todo_title.is_empty()
                {
                    let title = std::mem::take(&mut self.new_todo_title);
                    self.call_add_todo(title);
                }
            });

            ui.add_space(5.0);

            // Status line
            ui.label(format!("{} / {} completed", completed, total));

            ui.separator();

            // Error message
            if let Some(err) = &self.error_message {
                ui.colored_label(egui::Color32::RED, format!("Error: {}", err));
                ui.add_space(5.0);
            }

            // Todo list
            egui::ScrollArea::vertical().show(ui, |ui| {
                // Collect actions to avoid borrow issues
                let mut toggle_id = None;
                let mut delete_id = None;

                for todo in &self.todos {
                    ui.horizontal(|ui| {
                        let mut completed = todo.completed;
                        if ui.checkbox(&mut completed, "").changed() {
                            toggle_id = Some(todo.id);
                        }

                        if todo.completed {
                            ui.label(
                                egui::RichText::new(&todo.title)
                                    .strikethrough()
                                    .color(egui::Color32::GRAY),
                            );
                        } else {
                            ui.label(&todo.title);
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .button(egui::RichText::new("X").color(egui::Color32::RED))
                                .clicked()
                            {
                                delete_id = Some(todo.id);
                            }
                        });
                    });
                }

                if let Some(id) = toggle_id {
                    self.call_toggle_todo(id);
                }
                if let Some(id) = delete_id {
                    self.call_delete_todo(id);
                }
            });

            if self.todos.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No todos yet. Add one above!")
                            .color(egui::Color32::GRAY),
                    );
                });
            }
        }
    }

    impl eframe::App for TodoApp {
        fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
            self.process_responses();

            egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    egui::widgets::global_theme_preference_buttons(ui);
                });
            });

            egui::CentralPanel::default().show(ctx, |ui| {
                if self.username.is_some() && self.token.is_some() {
                    self.render_todos(ui);
                } else {
                    self.render_login(ui);
                }

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
}
