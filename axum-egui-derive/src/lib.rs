use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

/// Derives the AxumEguiApp trait for a struct, allowing it to serve an egui app through axum.
/// 
/// # Example
/// ```rust,no_run
/// use axum_egui::AxumEguiApp;
/// use eframe::App;
/// 
/// #[derive(Default)]
/// struct MyApp;
/// 
/// impl App for MyApp {
///     fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
///         // Your egui app code here
///     }
/// }
/// 
/// #[derive(AxumEguiApp)]
/// #[app(MyApp)]
/// struct MyAxumApp;
/// ```
#[proc_macro_derive(AxumEguiApp, attributes(app))]
pub fn derive_axum_egui_app(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    
    // TODO: Implement the actual derive macro
    let expanded = quote! {
        // Temporary implementation
        compile_error!("AxumEguiApp derive macro is not yet implemented");
    };

    TokenStream::from(expanded)
} 