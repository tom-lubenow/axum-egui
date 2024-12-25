mod core;
mod builder;
mod assets;

pub use axum_egui_derive::AxumEguiApp;
pub use core::AxumEguiHandler;
pub use builder::WasmBuilder;

/// Core trait that provides the integration between axum and egui.
/// This trait is typically derived using the `AxumEguiApp` derive macro.
pub trait AxumEguiApp: Sized {
    /// The egui App type that this axum integration serves
    type App: eframe::App;
    
    /// Creates a new instance of the egui app
    fn create_app() -> Self::App;
    
    /// Returns the router that serves this app
    fn router() -> axum::Router;
    
    /// Returns the fallback handler for serving static files
    fn fallback() -> axum::routing::MethodRouter;
} 