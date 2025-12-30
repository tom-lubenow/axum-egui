# axum-egui

Seamlessly embed egui frontends in axum backends with a single deployable binary.

## Features

- **Single-command build** - `cargo build` compiles everything (server + WASM frontends)
- **Server functions** - Define once, call from anywhere (`#[server]`, `#[server(sse)]`, `#[server(ws)]`)
- **Initial state injection** - Server passes state to frontend via `App<T>`
- **Auto-registration** - Server functions register themselves at compile time

## Quick Start

```bash
# Clone and run the example
git clone https://github.com/user/axum-egui
cd axum-egui
cargo run -p basic-server
# Open http://127.0.0.1:3000
```

## Creating a New Project

A typical project has three crates:

```
my-app/
├── .cargo/
│   └── config.toml      # Enable artifact dependencies
├── Cargo.toml           # Workspace
├── shared/              # Types + server functions (both targets)
├── frontend/            # egui WASM app
└── server/              # axum server
```

### Step 1: Workspace Setup

**`.cargo/config.toml`** (required for artifact dependencies):
```toml
[unstable]
bindeps = true
```

**`Cargo.toml`** (workspace root):
```toml
[workspace]
members = ["shared", "frontend", "server"]
resolver = "2"

# Only build server by default (frontend is built via artifact dependency)
default-members = ["shared", "server"]

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### Step 2: Shared Crate

Defines types and server functions that work on both server and client.

**`shared/Cargo.toml`**:
```toml
[package]
name = "my-shared"
version = "0.1.0"
edition = "2024"

[features]
default = ["server"]
server = ["server-fn/server", "dep:async-stream", "dep:tokio"]
web = ["server-fn/web"]

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
server-fn = "0.1"
futures = "0.3"

# Server-only
async-stream = { version = "0.3", optional = true }
tokio = { version = "1", features = ["full"], optional = true }
```

**`shared/src/lib.rs`**:
```rust
use serde::{Deserialize, Serialize};
use server_fn::prelude::*;

/// App state - serialized by server, deserialized by frontend
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppState {
    pub counter: i32,
    pub message: String,
}

/// A server function - runs on server, callable from frontend
#[server]
pub async fn increment(value: i32) -> Result<i32, ServerFnError> {
    Ok(value + 1)
}

/// SSE streaming example
#[server(sse)]
pub async fn counter_stream() -> impl Stream<Item = i32> {
    async_stream::stream! {
        for i in 0..100 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            yield i;
        }
    }
}
```

### Step 3: Frontend Crate

The egui WASM application.

**`frontend/Cargo.toml`**:
```toml
[package]
name = "my-frontend"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
my-shared = { path = "../shared", default-features = false, features = ["web"] }

# egui
egui = "0.31"
eframe = { version = "0.31", default-features = false, features = ["wgpu", "web_screen_reader"] }

# WASM
wasm-bindgen = "=0.2.104"
wasm-bindgen-futures = "0.4"
web-sys = { version = "0.3", features = ["Document", "Element", "HtmlCanvasElement"] }
log = "0.4"

# Server functions
server-fn = { version = "0.1", features = ["web"] }
serde = { workspace = true }
serde_json = { workspace = true }
```

**`frontend/src/lib.rs`**:
```rust
use my_shared::{AppState, increment};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    wasm_bindgen_futures::spawn_local(async {
        let window = web_sys::window().unwrap();
        let document = window.document().unwrap();

        // Read initial state injected by server
        let state: AppState = document
            .get_element_by_id("axum-egui-state")
            .and_then(|el| el.text_content())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .unwrap()
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .unwrap();

        eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(move |_cc| Ok(Box::new(MyApp::new(state)))),
            )
            .await
            .expect("Failed to start eframe");
    });
}

struct MyApp {
    counter: i32,
}

impl MyApp {
    fn new(state: AppState) -> Self {
        Self { counter: state.counter }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("My App");
            ui.label(format!("Counter: {}", self.counter));

            if ui.button("Increment (server)").clicked() {
                let current = self.counter;
                let ctx = ctx.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    if let Ok(new_value) = increment(current).await {
                        // In real app, update state properly
                        log::info!("New value: {}", new_value);
                        ctx.request_repaint();
                    }
                });
            }
        });
    }
}
```

### Step 4: Server Crate

The axum server that serves the frontend and handles server functions.

**`server/Cargo.toml`**:
```toml
[package]
name = "my-server"
version = "0.1.0"
edition = "2024"

[dependencies]
my-shared = { path = "../shared" }
axum-egui = "0.1"
server-fn = { version = "0.1", features = ["server"] }
axum = "0.8"
tokio = { version = "1", features = ["full"] }
tracing-subscriber = "0.3"
rust-embed = { version = "8", features = ["interpolate-folder-path"] }
serde = { workspace = true }
serde_json = { workspace = true }

[build-dependencies]
axum-egui-build = "0.1"
my-frontend = { path = "../frontend", artifact = "cdylib", target = "wasm32-unknown-unknown" }
```

**`server/build.rs`**:
```rust
fn main() {
    axum_egui_build::frontend("my-frontend");
}
```

**`server/src/main.rs`**:
```rust
use axum::{Router, routing::get};
use axum_egui::prelude::*;
use my_shared::AppState;
use rust_embed::RustEmbed;

// Import shared to link server functions
use my_shared as _;

// Embed frontend (convention: {CRATE_NAME}_DIST)
#[derive(RustEmbed)]
#[folder = "$MY_FRONTEND_DIST"]
struct Assets;

async fn index() -> axum_egui::App<AppState, Assets> {
    axum_egui::App::new(AppState {
        counter: 42,
        message: "Hello from server!".into(),
    })
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(index))
        .register_server_fns()
        .fallback(axum_egui::static_handler::<Assets>);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("Server running on http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}
```

### Step 5: Build and Run

```bash
cargo run -p my-server
```

That's it! The frontend WASM is built automatically.

## Server Functions

```rust
use server_fn::prelude::*;

// Simple RPC
#[server]
pub async fn greet(name: String) -> Result<String, ServerFnError> {
    Ok(format!("Hello, {}!", name))
}

// SSE streaming (server → client)
#[server(sse)]
pub async fn events() -> impl Stream<Item = String> {
    async_stream::stream! {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            yield "tick".to_string();
        }
    }
}

// WebSocket (bidirectional)
#[server(ws)]
pub async fn echo(incoming: impl Stream<Item = String>) -> impl Stream<Item = String> {
    incoming.map(|msg| format!("Echo: {}", msg))
}
```

## Multiple Frontends

For apps with multiple frontends (e.g., user + admin):

**`server/build.rs`**:
```rust
fn main() {
    axum_egui_build::frontend("user-frontend");
    axum_egui_build::frontend("admin-frontend");
}
```

**`server/src/main.rs`**:
```rust
#[derive(RustEmbed)]
#[folder = "$USER_FRONTEND_DIST"]
struct UserAssets;

#[derive(RustEmbed)]
#[folder = "$ADMIN_FRONTEND_DIST"]
struct AdminAssets;
```

See `examples/multi-frontend/` for a complete example.

## Prerequisites

Requires Rust nightly (for artifact dependencies):

```bash
rustup install nightly
rustup default nightly
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.104
```

Or use the Nix flake: `nix develop`

## How It Works

```
cargo build
  └─ builds server
       └─ artifact dependency triggers frontend build (wasm32)
            └─ build.rs runs wasm-bindgen
                 └─ rust-embed embeds the result
```

No separate build step. No CLI tool. Just `cargo build`.
