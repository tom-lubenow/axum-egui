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
struct MyAxumApp {
    // The path to your egui app's source code
    // (This will be made more ergonomic in the future)
    source: &'static str = "src/egui_app.rs",
}

// Use in your axum router
async fn main() {
    let app = Router::new()
        .merge(MyAxumApp::router());
        
    axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}
```

## Technical Implementation Details

### Build Process (`build.rs`)

1. **WASM Compilation**:
   ```rust
   // 1. Read the egui app source code
   let app_source = std::fs::read_to_string("src/egui_app.rs")?;
   
   // 2. Create a temporary workspace for compilation
   let temp_dir = tempfile::tempdir()?;
   
   // 3. Set up a minimal project structure
   // - Cargo.toml with wasm32 target
   // - Source files
   // - wasm-bindgen configuration
   
   // 4. Compile to WASM directly using rustc
   Command::new("rustc")
       .args([
           "--target", "wasm32-unknown-unknown",
           "-O", "--crate-type=cdylib",
           // ... other necessary flags
       ])
       .output()?;
   
   // 5. Process with wasm-bindgen
   // Generate JS bindings and optimized WASM
   
   // 6. Create the HTML entry point
   let html = generate_html_template();
   ```

2. **Asset Bundling**:
   ```rust
   // In build.rs:
   println!("cargo:rerun-if-changed=src/egui_app.rs");
   
   // Bundle everything into the binary
   static ASSETS: Dir = include_dir!("path/to/compiled/assets");
   ```

3. **Runtime Integration**:
   ```rust
   // In your library:
   pub struct AxumEguiHandler {
       assets: Dir<'static>,
   }
   
   impl AxumEguiHandler {
       pub fn router(&self) -> Router {
           Router::new()
               .fallback_service(ServeDir::new(self.assets.clone()))
       }
   }
   ```

### Required Dependencies

```toml
[build-dependencies]
# For WASM compilation
wasm-bindgen-cli = "0.2"
# For temporary file handling during build
tempfile = "3.0"
# For including assets in binary
include_dir = "0.7"
```

## Development Status

🚧 This project is currently in early development. The initial implementation will use a hardcoded path to the egui app source code. Future versions will make this more ergonomic through proc macros and better configuration options.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
