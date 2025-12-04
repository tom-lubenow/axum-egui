//! Build script for compiling multiple egui WASM frontends.
//!
//! This example builds two frontends:
//! - user-frontend/ -> User-facing counter app
//! - admin-frontend/ -> Admin dashboard

use std::path::Path;
use std::process::Command;
use std::{env, fs};

/// Configuration for building a frontend.
struct FrontendConfig<'a> {
    /// Directory name relative to CARGO_MANIFEST_DIR
    dir: &'a str,
    /// Crate name (determines .wasm filename)
    crate_name: &'a str,
}

/// Build a single frontend to WASM.
fn build_frontend(manifest_dir: &Path, config: &FrontendConfig) {
    let frontend_dir = manifest_dir.join(config.dir);
    let dist_dir = frontend_dir.join("dist");

    // Tell cargo to rerun if frontend sources change
    println!("cargo:rerun-if-changed={}/src", config.dir);
    println!("cargo:rerun-if-changed={}/Cargo.toml", config.dir);
    println!("cargo:rerun-if-changed={}/index.html", config.dir);

    // Create dist directory
    fs::create_dir_all(&dist_dir)
        .unwrap_or_else(|e| panic!("Failed to create dist directory for {}: {}", config.dir, e));

    // Step 1: Build frontend to WASM
    println!("cargo:warning=Building {} WASM...", config.dir);

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
        .unwrap_or_else(|e| panic!("Failed to run cargo build for {}: {}", config.dir, e));

    if !status.success() {
        panic!("{} WASM build failed", config.dir);
    }

    // Step 2: Run wasm-bindgen
    println!("cargo:warning=Running wasm-bindgen for {}...", config.dir);

    let wasm_filename = format!("{}.wasm", config.crate_name.replace('-', "_"));
    let wasm_file = frontend_dir.join(format!(
        "target/wasm32-unknown-unknown/release/{}",
        wasm_filename
    ));

    if !wasm_file.exists() {
        panic!(
            "WASM file not found at {:?}. Expected crate name: {}",
            wasm_file, config.crate_name
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
        .expect("Failed to run wasm-bindgen");

    if !status.success() {
        panic!("wasm-bindgen failed for {}", config.dir);
    }

    // Step 3: Copy HTML template
    let html_src = frontend_dir.join("index.html");
    let html_dst = dist_dir.join("index.html");
    fs::copy(&html_src, &html_dst)
        .unwrap_or_else(|e| panic!("Failed to copy index.html for {}: {}", config.dir, e));

    let count = fs::read_dir(&dist_dir)
        .map(|entries| entries.count())
        .unwrap_or(0);
    println!(
        "cargo:warning={} build complete! {} files in dist/",
        config.dir, count
    );
}

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let manifest_path = Path::new(&manifest_dir);

    // Build both frontends
    let frontends = [
        FrontendConfig {
            dir: "user-frontend",
            crate_name: "user_frontend",
        },
        FrontendConfig {
            dir: "admin-frontend",
            crate_name: "admin_frontend",
        },
    ];

    for config in &frontends {
        build_frontend(manifest_path, config);
    }
}
