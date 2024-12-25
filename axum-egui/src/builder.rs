use std::{path::PathBuf, process::Command};

/// Builder for compiling and bundling egui apps for web
pub struct WasmBuilder {
    output_dir: PathBuf,
}

impl WasmBuilder {
    /// Create a new builder that will output to the given directory
    pub fn new(output_dir: impl AsRef<PathBuf>) -> Self {
        Self {
            output_dir: output_dir.as_ref().into(),
        }
    }

    /// Build the app and return a handler for serving it
    pub fn build_handler<A: eframe::App + 'static>(&self, app: A) -> crate::AxumEguiHandler<A> {
        crate::AxumEguiHandler::new(app)
    }
} 