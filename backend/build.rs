use std::path::Path;
use std::process::Command;
use std::{env, fs};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let frontend_dir = Path::new(&manifest_dir).join("../frontend");
    let dist_dir = frontend_dir.join("dist");

    // Tell cargo to rerun if frontend sources change
    println!("cargo:rerun-if-changed=../frontend/src");
    println!("cargo:rerun-if-changed=../frontend/Cargo.toml");
    println!("cargo:rerun-if-changed=../frontend/index.html");

    // Create dist directory
    fs::create_dir_all(&dist_dir).expect("Failed to create dist directory");

    // Step 1: Build frontend to WASM
    // Since frontend has its own Cargo.lock (not in workspace), this won't deadlock
    println!("cargo:warning=Building frontend WASM...");

    let status = Command::new("cargo")
        .current_dir(&frontend_dir)
        .args([
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--release",
            "--lib",
        ])
        .status()
        .expect("Failed to run cargo build for frontend");

    if !status.success() {
        panic!("Frontend WASM build failed");
    }

    // Step 2: Run wasm-bindgen to generate JS bindings
    println!("cargo:warning=Running wasm-bindgen...");

    let wasm_file = frontend_dir.join("target/wasm32-unknown-unknown/release/frontend.wasm");

    if !wasm_file.exists() {
        panic!(
            "WASM file not found at {:?}. Frontend build may have failed.",
            wasm_file
        );
    }

    let status = Command::new("wasm-bindgen")
        .args([
            wasm_file.to_str().unwrap(),
            "--out-dir",
            dist_dir.to_str().unwrap(),
            "--target",
            "web",
            "--no-typescript",
        ])
        .status()
        .expect("Failed to run wasm-bindgen. Is it installed? Run: cargo install wasm-bindgen-cli");

    if !status.success() {
        panic!("wasm-bindgen failed");
    }

    // Step 3: Copy HTML template to dist
    println!("cargo:warning=Copying HTML template...");

    let html_src = frontend_dir.join("index.html");
    let html_dst = dist_dir.join("index.html");
    fs::copy(&html_src, &html_dst).expect("Failed to copy index.html");

    // Count files in dist
    let count = fs::read_dir(&dist_dir)
        .map(|entries| entries.count())
        .unwrap_or(0);
    println!(
        "cargo:warning=Frontend build complete! {} files in dist/",
        count
    );
}
