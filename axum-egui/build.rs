use std::{env, fs, path::PathBuf};
use tempfile::TempDir;
use wasm_bindgen_cli_support::Bindgen;

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed=examples/simple/app.rs");
    println!("cargo:rerun-if-changed=build.rs");
    
    // Create a temporary directory for our WASM build
    let temp_dir = TempDir::new()?;
    let wasm_dir = temp_dir.path().join("wasm");
    fs::create_dir_all(&wasm_dir)?;
    
    // Set up the minimal project structure
    setup_wasm_project(&wasm_dir)?;
    
    // Compile to WASM
    compile_to_wasm(&wasm_dir)?;
    
    // Process with wasm-bindgen
    process_wasm_bindgen(&wasm_dir)?;
    
    // Create the assets directory in the source tree
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let assets_dir = manifest_dir.join("assets/dist");
    
    // Remove any existing assets
    if assets_dir.exists() {
        fs::remove_dir_all(&assets_dir)?;
    }
    fs::create_dir_all(&assets_dir)?;
    
    // Copy the processed files to the assets directory
    copy_assets(&wasm_dir, &assets_dir)?;
    
    Ok(())
}

fn setup_wasm_project(wasm_dir: &PathBuf) -> std::io::Result<()> {
    // Create Cargo.toml
    let cargo_toml = r#"[package]
name = "egui-app"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
egui = "0.30"
eframe = { version = "0.30", default-features = false, features = ["glow"] }
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
web-sys = { version = "0.3", features = ["HtmlCanvasElement"] }
console_error_panic_hook = "0.1"
"#;
    fs::write(wasm_dir.join("Cargo.toml"), cargo_toml)?;
    
    // Create src directory
    let src_dir = wasm_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    
    // Create the lib.rs with wasm-bindgen wrapper
    let app_code = fs::read_to_string("examples/simple/app.rs")?;
    let lib_rs = format!(
r#"use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;
use eframe::wasm_bindgen::JsValue;

{app_code}

#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<(), JsValue> {{
    // Make sure panics are logged using `console.error`.
    console_error_panic_hook::set_once();

    let app = Box::new(SimpleApp::default());
    
    eframe::WebRunner::new()
        .start(
            canvas,
            eframe::WebOptions::default(),
            Box::new(|_cc| Ok(app)),
        )
        .await
}}
"#);

    fs::write(src_dir.join("lib.rs"), lib_rs)?;
    
    Ok(())
}

fn compile_to_wasm(wasm_dir: &PathBuf) -> std::io::Result<()> {
    // Run cargo build --target wasm32-unknown-unknown
    let status = std::process::Command::new("cargo")
        .args([
            "build",
            "--target", "wasm32-unknown-unknown",
            "--release",
        ])
        .current_dir(wasm_dir)
        .status()?;
    
    if !status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Failed to compile to WASM",
        ));
    }
    
    Ok(())
}

fn process_wasm_bindgen(wasm_dir: &PathBuf) -> std::io::Result<()> {
    let target_dir = wasm_dir.join("target/wasm32-unknown-unknown/release");
    let wasm_file = target_dir.join("egui_app.wasm");
    
    // Process with wasm-bindgen
    let mut bindgen = Bindgen::new();
    bindgen
        .input_path(wasm_file)
        .web(true)
        .unwrap()
        .generate(wasm_dir)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
    
    Ok(())
}

fn copy_assets(wasm_dir: &PathBuf, dist_dir: &PathBuf) -> std::io::Result<()> {
    // Copy the WASM and JS files
    fs::copy(
        wasm_dir.join("egui_app_bg.wasm"),
        dist_dir.join("egui_app_bg.wasm"),
    )?;
    fs::copy(
        wasm_dir.join("egui_app.js"),
        dist_dir.join("egui_app.js"),
    )?;
    
    // Create and write the HTML file
    let html = r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8" />
    <title>Egui App</title>
    <script type="module">
        import init, { start } from './egui_app.js';
        async function run() {
            await init();
            const canvas = document.getElementById('egui_canvas');
            await start(canvas);
        }
        run();
    </script>
    <style>
        html, body {
            margin: 0;
            padding: 0;
            height: 100%;
            overflow: hidden;
        }
        #egui_canvas {
            width: 100%;
            height: 100%;
        }
    </style>
</head>
<body>
    <canvas id="egui_canvas"></canvas>
</body>
</html>"#;
    fs::write(dist_dir.join("index.html"), html)?;
    
    Ok(())
} 