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
//! 4. Rename `.js` and `.wasm` files with content-hash suffixes for cache busting
//! 5. Pre-compress assets with gzip and brotli
//! 6. Set the `MY_FRONTEND_DIST` environment variable for `rust-embed`
//!
//! In your server code, use the derived env var name:
//!
//! ```ignore
//! #[derive(RustEmbed)]
//! #[folder = "$MY_FRONTEND_DIST"]
//! struct Assets;
//! ```

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};

/// Compute the first 8 hex characters of a SHA-256 hash of the file contents.
fn content_hash(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    format!("{:x}", hash)[..8].to_string()
}

/// Rename a file to include a content hash: `stem.hash.ext`.
///
/// Returns the new file name (not full path).
fn rename_with_hash(path: &Path) -> String {
    let data = fs::read(path).expect("Failed to read file for hashing");
    let hash = content_hash(&data);

    let stem = path
        .file_stem()
        .expect("no file stem")
        .to_str()
        .expect("non-UTF-8 file stem");
    let ext = path
        .extension()
        .expect("no extension")
        .to_str()
        .expect("non-UTF-8 extension");

    let new_name = format!("{}.{}.{}", stem, hash, ext);
    let new_path = path.with_file_name(&new_name);
    fs::rename(path, &new_path).expect("Failed to rename file with hash");

    new_name
}

/// Compress a file with gzip (best compression) and write `{path}.gz`.
fn compress_gzip(path: &Path) {
    let data = fs::read(path).expect("Failed to read file for gzip");
    let gz_path = path.with_extension(format!(
        "{}.gz",
        path.extension().unwrap().to_str().unwrap()
    ));
    let file = fs::File::create(&gz_path).expect("Failed to create .gz file");
    let mut encoder = GzEncoder::new(file, Compression::best());
    encoder.write_all(&data).expect("Failed to write gzip data");
    encoder.finish().expect("Failed to finish gzip");
}

/// Compress a file with brotli and write `{path}.br`.
fn compress_brotli(path: &Path) {
    let data = fs::read(path).expect("Failed to read file for brotli");
    let br_path = path.with_extension(format!(
        "{}.br",
        path.extension().unwrap().to_str().unwrap()
    ));
    let file = fs::File::create(&br_path).expect("Failed to create .br file");
    let mut encoder = brotli::CompressorWriter::new(file, 4096, 11, 22);
    encoder
        .write_all(&data)
        .expect("Failed to write brotli data");
    drop(encoder); // flush and finish
}

/// Pre-compress a file with both gzip and brotli.
fn compress_file(path: &Path) {
    compress_gzip(path);
    compress_brotli(path);
}

/// Check if a file extension should be compressed and content-hashed.
fn should_process(ext: &str) -> bool {
    matches!(ext, "js" | "wasm" | "html" | "css")
}

/// Process a frontend WASM artifact.
///
/// This function:
/// 1. Locates the WASM artifact built via Cargo's artifact dependency
/// 2. Runs `wasm-bindgen` to generate JS bindings
/// 3. Creates a default `index.html` if none exists in `../{crate_name}/index.html`
/// 4. Renames `.js` and `.wasm` files with content hashes for cache busting
/// 5. Pre-compresses `.js`, `.wasm`, `.html`, and `.css` files with gzip and brotli
/// 6. Sets `{CRATE_NAME}_DIST` env var pointing to the output directory
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

    // Collect wasm-bindgen output files for hashing
    let js_path = dist_dir.join(format!("{}.js", crate_name_underscored));
    let wasm_bg_path = dist_dir.join(format!("{}_bg.wasm", crate_name_underscored));

    // Content-hash the WASM file first (JS references it, HTML references JS)
    let hashed_wasm_name = rename_with_hash(&wasm_bg_path);

    // Update JS file: replace the original WASM filename with the hashed one
    let original_wasm_name = format!("{}_bg.wasm", crate_name_underscored);
    let js_content = fs::read_to_string(&js_path).expect("Failed to read JS file");
    let js_updated = js_content.replace(&original_wasm_name, &hashed_wasm_name);
    fs::write(&js_path, &js_updated).expect("Failed to write updated JS file");

    // Content-hash the JS file (after updating its WASM reference)
    let hashed_js_name = rename_with_hash(&js_path);

    // Now create or copy index.html with hashed JS name
    let original_js_name = format!("{}.js", crate_name_underscored);
    if html_src.exists() {
        let html_content = fs::read_to_string(&html_src).expect("Failed to read index.html");
        let html_updated = html_content.replace(&original_js_name, &hashed_js_name);
        fs::write(&html_dst, html_updated).expect("Failed to write index.html");
    } else {
        // Create default HTML with hashed JS name
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
        import init from './{hashed_js_name}';
        init();
    </script>
</body>
</html>"#
        );
        fs::write(&html_dst, default_html).expect("Failed to write index.html");
    }

    // Pre-compress all processable files in the dist directory
    let entries: Vec<PathBuf> = fs::read_dir(&dist_dir)
        .expect("Failed to read dist directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(should_process)
        })
        .collect();

    for path in &entries {
        compress_file(path);
    }

    // Export the dist directory path for rust-embed
    // Convention: {CRATE_NAME}_DIST
    let env_var_out = format!("{}_DIST", crate_name_upper);
    println!("cargo:rustc-env={}={}", env_var_out, dist_dir.display());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_content_hash_deterministic() {
        let data = b"hello world";
        let h1 = content_hash(data);
        let h2 = content_hash(data);
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 8);
    }

    #[test]
    fn test_content_hash_differs_for_different_data() {
        let h1 = content_hash(b"hello");
        let h2 = content_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn test_rename_with_hash() {
        let dir = tempdir();
        let file_path = dir.join("app.js");
        fs::write(&file_path, b"console.log('test');").unwrap();

        let new_name = rename_with_hash(&file_path);

        // Original should be gone
        assert!(!file_path.exists());

        // New file should exist with hash in name
        let new_path = dir.join(&new_name);
        assert!(new_path.exists());

        // Name format: stem.hash.ext
        assert!(new_name.starts_with("app."));
        assert!(new_name.ends_with(".js"));
        // Hash is 8 hex chars between stem and ext
        let parts: Vec<&str> = new_name.splitn(3, '.').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "app");
        assert_eq!(parts[1].len(), 8);
        assert_eq!(parts[2], "js");
    }

    #[test]
    fn test_gzip_compression() {
        let dir = tempdir();
        let file_path = dir.join("test.js");
        fs::write(&file_path, b"function hello() { return 'world'; }").unwrap();

        compress_gzip(&file_path);

        let gz_path = dir.join("test.js.gz");
        assert!(gz_path.exists());

        // Verify it's valid gzip (starts with magic bytes 1f 8b)
        let gz_data = fs::read(&gz_path).unwrap();
        assert!(gz_data.len() >= 2);
        assert_eq!(gz_data[0], 0x1f);
        assert_eq!(gz_data[1], 0x8b);
    }

    #[test]
    fn test_brotli_compression() {
        let dir = tempdir();
        let file_path = dir.join("test.js");
        fs::write(&file_path, b"function hello() { return 'world'; }").unwrap();

        compress_brotli(&file_path);

        let br_path = dir.join("test.js.br");
        assert!(br_path.exists());

        // Just verify the file is non-empty
        let br_data = fs::read(&br_path).unwrap();
        assert!(!br_data.is_empty());
    }

    #[test]
    fn test_should_process() {
        assert!(should_process("js"));
        assert!(should_process("wasm"));
        assert!(should_process("html"));
        assert!(should_process("css"));
        assert!(!should_process("png"));
        assert!(!should_process("gz"));
        assert!(!should_process("br"));
    }

    /// Create a temporary directory for testing.
    fn tempdir() -> PathBuf {
        let dir = env::temp_dir().join(format!("axum-egui-build-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
