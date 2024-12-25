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
    
    // Generate the assets module
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let assets_rs = out_dir.join("assets.rs");
    
    let assets_dir_str = assets_dir.to_str().unwrap();
    fs::write(assets_rs, format!(r#"
use include_dir::{{include_dir, Dir}};
pub static ASSETS: Dir = include_dir!("{assets_dir_str}");
"#))?;
    
    println!("cargo:rerun-if-changed=assets/dist");
    
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
eframe = { version = "0.30", default-features = false, features = [
    "default_fonts",
    "glow",
] }
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
web-sys = { version = "0.3", features = [
    "HtmlCanvasElement",
    "Window",
    "Document",
    "Element",
    "console"
] }
console_error_panic_hook = "0.1"
getrandom = { version = "0.2", features = ["js"] }
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

// Initialize debug logging and panic hook
#[wasm_bindgen(start)]
pub fn init() {{
    console_error_panic_hook::set_once();
}}

{app_code}

#[wasm_bindgen]
pub async fn start(canvas: HtmlCanvasElement) -> Result<(), JsValue> {{
    let app = Box::new(SimpleApp::default());
    
    let web_options = eframe::WebOptions::default();
    
    eframe::WebRunner::new()
        .start(
            canvas,
            web_options,
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
        .omit_default_module_path(true)
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
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Egui App</title>
    <style>
        html, body {
            margin: 0;
            padding: 0;
            height: 100%;
            overflow: hidden;
            background-color: #1f1f1f;
        }
        #egui_canvas {
            width: 100%;
            height: 100%;
            display: block;
        }
        #loading {
            position: fixed;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            color: #888;
            font-family: sans-serif;
        }
    </style>
</head>
<body>
    <canvas id="egui_canvas"></canvas>
    <div id="loading">Loading...</div>
    <script type="module">
        const loadingText = document.getElementById('loading');
        try {
            const { default: init, start } = await import('./egui_app.js');
            const wasm = await init('./egui_app_bg.wasm');
            const canvas = document.getElementById('egui_canvas');
            await start(canvas);
            loadingText.remove();
        } catch (error) {
            loadingText.textContent = `Error: ${error.message}`;
            console.error(error);
        }
    </script>
</body>
</html>"#;
    fs::write(dist_dir.join("index.html"), html)?;
    
    Ok(())
} 