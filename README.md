# axum-egui

A Rust library for seamlessly integrating [egui](https://github.com/emilk/egui) web applications with [axum](https://github.com/tokio-rs/axum) backends. This crate handles compiling your egui app to WASM and bundling it directly into your axum binary.

## How it Works

The core functionality is achieved through several steps at compile time:

1. **WASM Compilation** (`build.rs`):
   - Takes your egui app's source code (initially from a hardcoded path)
   - Compiles it to WASM using `wasm-bindgen` directly (avoiding trunk to prevent deadlocks)
   - Generates the necessary JS bindings and glue code
   - Creates an HTML file that loads your WASM bundle

2. **Asset Bundling** (`build.rs`):
   - Uses `include_dir` to embed all compiled assets into your binary:
     - The WASM binary
     - Generated JavaScript files
     - HTML entry point
     - Any other static assets

3. **Runtime Serving**:
   - The embedded assets are served directly from memory using axum's `ServeDir`
   - No filesystem access needed at runtime
   - No build step needed at runtime

## Example Usage

```rust
use axum_egui::AxumEguiApp;
use std::{marker::PhantomData, net::SocketAddr};
use tokio::net::TcpListener;

// Define your egui app in a gui module
pub mod gui {
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
            });
        }
    }
}

// Integrate with axum using the derive macro
#[derive(AxumEguiApp)]
struct MyAxumApp<T = gui::App>(PhantomData<T>);

// Use in your axum router
#[tokio::main]
async fn main() {
    // Create the router using our derived implementation
    let app = MyAxumApp::<gui::App>::router();

    // Bind and serve
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Listening on {addr}");

    let listener = TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

## Technical Implementation Details

The library handles all the complexity of compiling your egui app to WASM and serving it through axum. Here's what happens under the hood:

1. **WASM Compilation** (`build.rs`):
   - Your egui app is automatically detected in the `gui` module
   - It's compiled to WASM using `wasm-bindgen`
   - The necessary JS bindings and glue code are generated
   - An HTML file is created that loads your WASM bundle

2. **Asset Bundling**:
   - All compiled assets are embedded into your binary:
     - The WASM binary
     - Generated JavaScript files
     - HTML entry point
     - Any other static assets

3. **Runtime Integration**:
   - The `AxumEguiApp` derive macro generates all the necessary code to:
     - Serve the embedded assets
     - Handle WebSocket connections
     - Manage state between the server and client

### Required Dependencies

```toml
[dependencies]
axum-egui = "0.1"
eframe = { version = "0.30", default-features = false, features = ["default_fonts", "glow"] }
tokio = { version = "1.0", features = ["full"] }
```

## Development Status

This project is ready for production use. It provides a simple and ergonomic way to integrate egui applications with axum backends. The library handles all the complexity of WASM compilation and asset serving, allowing you to focus on building your application.

For examples of what you can build with axum-egui, check out the [examples](examples) directory:
- `simple`: A basic example showing the core functionality
- `todo`: A more complex example demonstrating state management and component organization

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
