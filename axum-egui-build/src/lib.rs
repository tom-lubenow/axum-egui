//! Build-time utilities for axum-egui.
//!
//! This crate provides helpers for processing WASM frontend artifacts in your `build.rs`.
//!
//! # Usage
//!
//! Add to your server's `Cargo.toml`:
//!
//! ```toml
//! [build-dependencies]
//! axum-egui-build = "0.1"
//!
//! # Artifact dependency - triggers automatic WASM build
//! my-frontend = { path = "../frontend", artifact = "cdylib", target = "wasm32-unknown-unknown" }
//! ```
//!
//! Then in `build.rs`:
//!
//! ```ignore
//! fn main() {
//!     axum_egui_build::frontend("my-frontend");
//! }
//! ```
//!
//! This will:
//! 1. Find the WASM artifact from the `my-frontend` crate
//! 2. Run `wasm-bindgen` to generate JS bindings
//! 3. Create a default `index.html` if none exists
//! 4. Set the `MY_FRONTEND_DIST` environment variable for `rust-embed`
//!
//! In your server code, use the derived env var name:
//!
//! ```ignore
//! #[derive(RustEmbed)]
//! #[folder = "$MY_FRONTEND_DIST"]
//! struct Assets;
//! ```

use std::path::Path;
use std::process::Command;
use std::{env, fs};

/// Process a frontend WASM artifact.
///
/// This function:
/// 1. Locates the WASM artifact built via Cargo's artifact dependency
/// 2. Runs `wasm-bindgen` to generate JS bindings
/// 3. Creates a default `index.html` if none exists in `../{crate_name}/index.html`
/// 4. Sets `{CRATE_NAME}_DIST` env var pointing to the output directory
///
/// # Arguments
///
/// * `crate_name` - The name of the frontend crate (e.g., "my-frontend")
///
/// # Panics
///
/// Panics if:
/// - The artifact dependency environment variable is not found
/// - `wasm-bindgen` is not installed or fails
///
/// # Example
///
/// ```ignore
/// // build.rs
/// fn main() {
///     axum_egui_build::frontend("basic-frontend");
/// }
/// ```
///
/// For multiple frontends:
///
/// ```ignore
/// // build.rs
/// fn main() {
///     axum_egui_build::frontend("user-frontend");
///     axum_egui_build::frontend("admin-frontend");
/// }
/// ```
pub fn frontend(crate_name: &str) {
    let crate_name_underscored = crate_name.replace('-', "_");
    let crate_name_upper = crate_name_underscored.to_uppercase();

    // Set up rerun triggers
    println!("cargo:rerun-if-changed=../{}/src/", crate_name);
    println!("cargo:rerun-if-changed=../{}/Cargo.toml", crate_name);

    // Create output directory
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR not set");
    let dist_dir = Path::new(&out_dir).join(format!("{}-dist", crate_name));
    fs::create_dir_all(&dist_dir).expect("Failed to create dist directory");

    // Find the WASM artifact
    // Cargo sets: CARGO_CDYLIB_FILE_{CRATE_NAME}_{crate_name}
    let env_var_name = format!(
        "CARGO_CDYLIB_FILE_{}_{}",
        crate_name_upper, crate_name_underscored
    );
    let wasm_path = env::var(&env_var_name)
        .or_else(|_| env::var(format!("CARGO_CDYLIB_FILE_{}", crate_name_upper)))
        .unwrap_or_else(|_| {
            panic!(
                "Artifact dependency not found. Expected env var: {}\n\
                 Make sure you have this in Cargo.toml:\n\n\
                 [build-dependencies]\n\
                 {} = {{ path = \"../{}\", artifact = \"cdylib\", target = \"wasm32-unknown-unknown\" }}\n\n\
                 And .cargo/config.toml has:\n\n\
                 [unstable]\n\
                 bindeps = true",
                env_var_name, crate_name, crate_name
            )
        });

    // Run wasm-bindgen
    let status = Command::new("wasm-bindgen")
        .args([
            &wasm_path,
            "--out-dir",
            dist_dir.to_str().unwrap(),
            "--target",
            "web",
            "--no-typescript",
        ])
        .status()
        .expect(
            "Failed to run wasm-bindgen. Is it installed?\n\
             Run: cargo install wasm-bindgen-cli --version 0.2.104",
        );

    if !status.success() {
        panic!("wasm-bindgen failed for {}", crate_name);
    }

    // Copy or create index.html
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let html_src = Path::new(&manifest_dir).join(format!("../{}/index.html", crate_name));
    let html_dst = dist_dir.join("index.html");

    if html_src.exists() {
        fs::copy(&html_src, &html_dst).expect("Failed to copy index.html");
    } else {
        // Create default HTML
        let js_name = format!("{}.js", crate_name_underscored);
        let default_html = format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>axum-egui</title>
    <style>
        html, body {{ margin: 0; padding: 0; width: 100%; height: 100%; overflow: hidden; }}
        canvas {{ width: 100%; height: 100%; }}
        #loading_text {{ position: absolute; top: 50%; left: 50%; transform: translate(-50%, -50%); font-family: sans-serif; font-size: 1.5em; color: #888; }}
    </style>
    <!--AXUM_EGUI_INITIAL_STATE-->
</head>
<body>
    <p id="loading_text">Loading...</p>
    <canvas id="the_canvas_id"></canvas>
    <script type="module">
        import init from './{js_name}';
        init();
    </script>
</body>
</html>"#
        );
        fs::write(&html_dst, default_html).expect("Failed to write index.html");
    }

    // Export the dist directory path for rust-embed
    // Convention: {CRATE_NAME}_DIST
    let env_var_out = format!("{}_DIST", crate_name_upper);
    println!("cargo:rustc-env={}={}", env_var_out, dist_dir.display());
}
