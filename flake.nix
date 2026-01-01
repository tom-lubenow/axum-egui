{
  description = "axum-egui - Seamlessly embed egui frontends in axum backends";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        # Nightly required for artifact dependencies (RFC 3028 / -Z bindeps)
        rustToolchain = pkgs.rust-bin.nightly.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" "clippy" ];
          targets = [ "wasm32-unknown-unknown" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            # Rust toolchain with WASM target
            rustToolchain

            # Build tools
            trunk
            wasm-bindgen-cli

            # Development tools
            cargo-watch
            cargo-edit
            cargo-nextest

            # System dependencies for native builds (optional)
            pkg-config
            openssl
          ];

          # Environment variables
          RUST_BACKTRACE = "1";
        };
      }
    );
}
