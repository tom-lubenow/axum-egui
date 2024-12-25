use include_dir::Dir;
use std::path::Path;

/// Builder for compiling and bundling egui apps for web
pub struct WasmBuilder {
    output_dir: Box<Path>,
}

impl WasmBuilder {
    /// Create a new builder that will output to the given directory
    pub fn new(output_dir: impl AsRef<Path>) -> Self {
        Self {
            output_dir: output_dir.as_ref().into(),
        }
    }

    /// Build the WASM bundle for the given app
    pub fn build(&self) -> std::io::Result<Dir<'static>> {
        // TODO: Implement WASM building
        todo!("Implement WASM building")
    }

    /// Build the app and return a handler for serving it
    pub fn build_handler<A>(&self, app: A) -> std::io::Result<crate::AxumEguiHandler<A>>
    where
        A: eframe::App + Send + Sync + 'static,
    {
        let assets = self.build()?;
        Ok(crate::AxumEguiHandler::new(app, assets))
    }
}
