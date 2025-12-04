//! Proc macro for generating server functions.
//!
//! Usage:
//! ```ignore
//! #[server]
//! pub async fn greet(name: String) -> Result<String, ServerFnError> {
//!     Ok(format!("Hello, {}!", name))
//! }
//! ```
//!
//! On the server, this generates an axum handler.
//! On the client (WASM), this generates an async function that calls the API.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, ItemFn, Pat, ReturnType, Type};

/// Attribute macro that generates both server handler and client caller.
///
/// The function must be `async` and return `Result<T, ServerFnError>`.
/// Parameters become the JSON request body fields.
/// The endpoint is automatically `/api/{fn_name}`.
#[proc_macro_attribute]
pub fn server(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);

    let fn_name = &input.sig.ident;
    let fn_name_str = fn_name.to_string();
    let endpoint = format!("/api/{}", fn_name_str);

    let request_name = format_ident!("{}Request", to_pascal_case(&fn_name_str));
    let response_name = format_ident!("{}Response", to_pascal_case(&fn_name_str));

    let vis = &input.vis;
    let asyncness = &input.sig.asyncness;
    let body = &input.block;

    // Extract parameters (skip self if present)
    let params: Vec<_> = input
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let FnArg::Typed(pat_type) = arg {
                if let Pat::Ident(pat_ident) = &*pat_type.pat {
                    let name = &pat_ident.ident;
                    let ty = &*pat_type.ty;
                    return Some((name.clone(), ty.clone()));
                }
            }
            None
        })
        .collect();

    let param_names: Vec<_> = params.iter().map(|(name, _)| name).collect();
    let param_types: Vec<_> = params.iter().map(|(_, ty)| ty).collect();

    // Extract return type (expecting Result<T, ServerFnError>)
    let return_type = match &input.sig.output {
        ReturnType::Type(_, ty) => extract_result_ok_type(ty),
        ReturnType::Default => quote! { () },
    };

    // Generate server-side code (non-WASM)
    let server_code = quote! {
        #[cfg(not(target_arch = "wasm32"))]
        #vis #asyncness fn #fn_name(
            axum::Json(req): axum::Json<#request_name>
        ) -> Result<axum::Json<#response_name>, axum::http::StatusCode> {
            #(let #param_names: #param_types = req.#param_names;)*

            let result: Result<#return_type, ServerFnError> = (|| async #body)().await;

            match result {
                Ok(value) => Ok(axum::Json(#response_name(value))),
                Err(e) => {
                    tracing::error!("Server function error: {:?}", e);
                    Err(axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                }
            }
        }
    };

    // Generate client-side code (WASM)
    let client_code = quote! {
        #[cfg(target_arch = "wasm32")]
        #vis async fn #fn_name(#(#param_names: #param_types),*) -> Result<#return_type, ServerFnError> {
            let req = #request_name { #(#param_names),* };

            let resp = gloo_net::http::Request::post(#endpoint)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&req).map_err(|e| ServerFnError::Serialization(e.to_string()))?)?
                .send()
                .await
                .map_err(|e| ServerFnError::Request(e.to_string()))?;

            if !resp.ok() {
                return Err(ServerFnError::ServerError(resp.status()));
            }

            let data: #response_name = resp.json().await
                .map_err(|e| ServerFnError::Deserialization(e.to_string()))?;

            Ok(data.0)
        }
    };

    // Generate shared types (for both targets)
    let request_struct = quote! {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        #vis struct #request_name {
            #(pub #param_names: #param_types),*
        }
    };

    let response_struct = quote! {
        #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
        #vis struct #response_name(pub #return_type);
    };

    let expanded = quote! {
        #request_struct
        #response_struct
        #server_code
        #client_code
    };

    TokenStream::from(expanded)
}

/// Convert snake_case to PascalCase
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}

/// Extract the Ok type from Result<T, E>
fn extract_result_ok_type(ty: &Type) -> proc_macro2::TokenStream {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Result" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(ok_type)) = args.args.first() {
                        return quote! { #ok_type };
                    }
                }
            }
        }
    }
    // Fallback: return the whole type
    quote! { #ty }
}
