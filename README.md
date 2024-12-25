# axum-egui

A Rust library for seamlessly integrating [egui](https://github.com/emilk/egui) web applications with [axum](https://github.com/tokio-rs/axum) backends. This crate simplifies the process of serving egui web apps through axum by handling all the building, bundling, and asset management automatically.

## Features

- 🔧 Zero-configuration integration of egui apps with axum
- 📦 Automatic building and bundling of egui web assets
- 🚀 Binary inclusion of assets using `include_dir`
- 🛠 Simple API with proc-macro support
- 🔄 Hot-reload support for development (coming soon)

## Example Usage

```rust
use axum_egui::AxumEguiApp;
use eframe::App;

// Your existing egui app
#[derive(Default)]
struct MyApp {
    name: String,
    age: u32,
}

impl App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("My Egui App");
            ui.text_edit_singleline(&mut self.name);
            ui.add(egui::Slider::new(&mut self.age, 0..=120).text("age"));
        });
    }
}

// Integrate with axum using our derive macro
#[derive(AxumEguiApp)]
#[app(MyApp)] // Specify which egui App to serve
struct MyAxumApp;

// Use in your axum router
async fn main() {
    let app = Router::new()
        .merge(MyAxumApp::router()) // Adds all necessary routes
        .fallback(MyAxumApp::fallback());
        
    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}
```

## How it Works

`axum-egui` handles all the complexity of:
1. Building your egui app for web (WASM)
2. Bundling all necessary assets
3. Including the built assets in your binary using `include_dir`
4. Serving the assets and handling WebSocket connections for your egui app
5. Managing the communication between your axum backend and egui frontend

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
axum-egui = "0.1.0"
```

## Development Status

🚧 This project is currently in early development. The API is subject to change.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
