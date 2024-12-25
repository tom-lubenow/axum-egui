use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, GenericParam};

#[proc_macro_derive(AxumEguiApp)]
pub fn derive_axum_egui_app(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = input.ident;

    // Get the type parameter T if it exists, otherwise use a generic T
    let type_param = input
        .generics
        .params
        .iter()
        .next()
        .map(|p| match p {
            GenericParam::Type(t) => t.ident.clone(),
            _ => panic!("Expected type parameter"),
        })
        .unwrap_or_else(|| syn::Ident::new("T", proc_macro2::Span::call_site()));

    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    let where_clause = where_clause
        .map(|w| quote!(#w))
        .unwrap_or_else(|| quote!(where #type_param: eframe::App + Default + 'static));

    let expanded = quote! {
        impl #impl_generics axum_egui::AxumEguiApp for #struct_name #type_generics #where_clause {
            type App = #type_param;

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
