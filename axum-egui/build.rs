use std::{env, fs, path::PathBuf};

fn main() {
    // Get the output directory from cargo
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());

    // Create a temporary directory for the WASM build
    let temp_dir = tempfile::tempdir().unwrap();
    let wasm_dir = temp_dir.path();

    // Create the src directory
    fs::create_dir_all(wasm_dir.join("src")).unwrap();

    // Look for the gui directory
    let gui_dir = manifest_dir.join("gui");
    if !gui_dir.exists() {
        // No GUI to compile, just create an empty assets directory
        let assets_dir = manifest_dir.join("assets/dist");
        fs::create_dir_all(&assets_dir).unwrap();
        return;
    }

    // Copy the entire gui directory to the temporary directory
    let mut copy_options = fs_extra::dir::CopyOptions::new();
    copy_options.overwrite = true;
    fs_extra::dir::copy(&gui_dir, wasm_dir.join("src"), &copy_options).unwrap();

    // Generate lib.rs
    let lib_rs = format!(
        r#"use eframe::egui;
        use wasm_bindgen::prelude::*;
        use web_sys::HtmlCanvasElement;

        mod app;
        pub use app::App;

        #[wasm_bindgen(start)]
        pub fn start() {{
            console_error_panic_hook::set_once();
        }}

        #[wasm_bindgen]
        pub async fn start_app(canvas_id: &str) -> Result<(), JsValue> {{
            let window = web_sys::window().unwrap();
            let document = window.document().unwrap();
            let canvas = document
                .get_element_by_id(canvas_id)
                .unwrap()
                .dyn_into::<HtmlCanvasElement>()
                .unwrap();
            
            let web_options = eframe::WebOptions::default();
            
            eframe::WebRunner::new()
                .start(
                    canvas,
                    web_options,
                    Box::new(|_cc| Ok(Box::new(App::default()))),
                )
                .await
        }}"#
    );

    // Write lib.rs
    fs::write(wasm_dir.join("src/gui/lib.rs"), lib_rs).unwrap();

    // Create Cargo.toml for the WASM crate
    let cargo_toml = format!(
        r#"[package]
        name = "egui-app"
        version = "0.1.0"
        edition = "2021"

        [lib]
        path = "src/gui/lib.rs"
        crate-type = ["cdylib"]

        [dependencies]
        eframe = {{ version = "0.30", default-features = false, features = ["default_fonts", "glow"] }}
        wasm-bindgen = "0.2"
        wasm-bindgen-futures = "0.4"
        web-sys = {{ version = "0.3", features = ["Window", "Document", "Element", "HtmlCanvasElement"] }}
        console_error_panic_hook = "0.1"
        getrandom = {{ version = "0.2", features = ["js"] }}
        "#
    );

    fs::write(wasm_dir.join("Cargo.toml"), cargo_toml).unwrap();

    // Build the WASM package
    let status = std::process::Command::new("cargo")
        .current_dir(wasm_dir)
        .args(["build", "--target", "wasm32-unknown-unknown", "--release"])
        .status()
        .unwrap();

    if !status.success() {
        panic!("Failed to compile to WASM");
    }

    // Process with wasm-bindgen
    let wasm_file = wasm_dir.join("target/wasm32-unknown-unknown/release/egui_app.wasm");

    let bindgen_status = std::process::Command::new("wasm-bindgen")
        .current_dir(wasm_dir)
        .args([
            wasm_file.to_str().unwrap(),
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--target",
            "web",
            "--no-typescript",
        ])
        .status()
        .unwrap();

    if !bindgen_status.success() {
        panic!("Failed to run wasm-bindgen");
    }

    // Create the HTML file
    let html = format!(
        r#"<!DOCTYPE html>
        <html>
        <head>
            <meta charset="utf-8">
            <title>egui app</title>
            <style>
                html, body {{
                    margin: 0;
                    padding: 0;
                    height: 100%;
                    width: 100%;
                }}
                canvas {{
                    width: 100%;
                    height: 100%;
                }}
            </style>
        </head>
        <body>
            <canvas id="canvas"></canvas>
            <script type="module">
                import init, {{ start_app }} from './egui_app.js';
                
                async function run() {{
                    await init();
                    await start_app('canvas').catch(console.error);
                }}
                
                run();
            </script>
        </body>
        </html>
        "#
    );

    // Write the HTML file
    fs::write(out_dir.join("index.html"), html).unwrap();

    // Create the assets directory
    let assets_dir = manifest_dir.join("assets/dist");
    fs::create_dir_all(&assets_dir).unwrap();

    // Copy the output files to the assets directory
    for entry in fs::read_dir(&out_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path
            .extension()
            .map_or(false, |ext| ext == "js" || ext == "wasm" || ext == "html")
        {
            fs::copy(&path, assets_dir.join(path.file_name().unwrap())).unwrap();
        }
    }

    println!("cargo:rerun-if-changed=gui");
    println!("cargo:rerun-if-changed=build.rs");
}
