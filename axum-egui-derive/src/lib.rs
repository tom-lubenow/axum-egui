use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(AxumEguiApp)]
pub fn derive_axum_egui_app(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = input.ident;
    
    let expanded = quote! {
        impl axum_egui::AxumEguiApp for #struct_name {
            type App = crate::gui::App;

            fn create_app() -> Self::App {
                Self::App::default()
            }

            fn router() -> axum::Router {
                let app = Self::create_app();
                let handler = axum_egui::AxumEguiHandler::new(app);
                handler.router()
            }

            fn fallback() -> axum::routing::MethodRouter {
                axum::routing::get(axum_egui::AxumEguiHandler::<Self::App>::serve_static_file)
            }
        }
    };

    TokenStream::from(expanded)
} 