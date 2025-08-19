# axum-egui

Seamlessly embed egui frontends in axum backends with a single deployable binary.

## The `App<T>` Pattern

Return egui apps directly from axum handlers:

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default)]
pub struct MyApp {
    pub label: String,
    pub value: f32,
}

// The magic - return App(state) from your handler!
async fn my_handler() -> App<MyApp> {
    App(MyApp {
        label: "Hello from the server!".into(),
        value: 42.0,
    })
}
```

The `App<T>` wrapper implements `IntoResponse`, serializing your state to JSON and embedding it in the HTML. The frontend WASM reads this initial state and renders your egui app.

## Project Structure

```
axum-egui/
├── backend/           # Axum server with App<T> wrapper
│   ├── src/main.rs    # Server + handlers
│   └── build.rs       # Auto-builds frontend WASM
├── frontend/          # Standalone egui WASM app
│   ├── src/lib.rs     # egui App implementation
│   └── Cargo.toml     # Independent (own Cargo.lock)
├── Cargo.toml         # Workspace (backend only)
└── flake.nix          # Nix dev environment
```

The frontend is intentionally **not in the workspace** - it has its own `Cargo.lock`. This allows `build.rs` to compile it without the cargo lock deadlock that would occur with nested cargo invocations.

## Prerequisites

Use the Nix flake for all dependencies:

```bash
nix develop
# or with direnv: direnv allow
```

This provides:
- Rust toolchain with wasm32-unknown-unknown target
- wasm-bindgen-cli (version-matched)
- cargo-watch for development

## Building

Just build the backend - it automatically compiles the frontend:

```bash
cargo build --package backend
```

The `build.rs` will:
1. Compile frontend to WASM (`cargo build --target wasm32-unknown-unknown`)
2. Run `wasm-bindgen` to generate JS bindings
3. Copy the HTML template

## Running

```bash
cargo run --package backend
# Server running on http://127.0.0.1:3000
```

## How It Works

1. **Handler returns `App(state)`** - Your state is serialized to JSON
2. **HTML template injection** - JSON is embedded in a `<script id="axum-egui-state">` tag
3. **WASM loads** - Frontend reads the JSON from DOM and deserializes
4. **egui renders** - App starts with server-provided initial state

## Dependencies

- egui/eframe 0.33
- axum 0.8
- wasm-bindgen 0.2.104 (pinned to match CLI)
