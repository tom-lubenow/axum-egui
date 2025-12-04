//! Proc macro for generating server functions.
//!
//! # RPC Usage
//! ```ignore
//! #[server]
//! pub async fn greet(name: String) -> Result<String, ServerFnError> {
//!     Ok(format!("Hello, {}!", name))
//! }
//! ```
//!
//! # SSE Usage
//! ```ignore
//! #[server(sse)]
//! pub async fn counter() -> impl Stream<Item = i32> {
//!     async_stream::stream! {
//!         for i in 0..10 {
//!             yield i;
//!         }
//!     }
//! }
//! ```
//!
//! On the server, this generates an axum handler.
//! On the client (WASM), this generates a function that returns SseStream<T>.

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, ItemFn, Pat, ReturnType, Type};

/// Server function mode
#[derive(Debug, Clone, Copy, PartialEq)]
enum ServerMode {
    /// Standard request/response RPC
    Rpc,
    /// Server-Sent Events streaming
    Sse,
}

/// Parse the attribute to determine the mode
fn parse_mode(attr: TokenStream) -> ServerMode {
    let attr_str = attr.to_string();
    if attr_str.contains("sse") {
        ServerMode::Sse
    } else {
        ServerMode::Rpc
    }
}

/// Attribute macro that generates both server handler and client caller.
///
/// # Modes
///
/// - `#[server]` - Standard RPC (request/response)
/// - `#[server(sse)]` - Server-Sent Events (streaming)
#[proc_macro_attribute]
pub fn server(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mode = parse_mode(attr);
    let input = parse_macro_input!(item as ItemFn);

    match mode {
        ServerMode::Rpc => generate_rpc(input),
        ServerMode::Sse => generate_sse(input),
    }
}

/// Generate code for standard RPC mode
fn generate_rpc(input: ItemFn) -> TokenStream {
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

/// Generate code for SSE mode
fn generate_sse(input: ItemFn) -> TokenStream {
    let fn_name = &input.sig.ident;
    let fn_name_str = fn_name.to_string();
    let endpoint = format!("/api/{}", fn_name_str);

    let vis = &input.vis;
    let body = &input.block;

    // Extract parameters
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

    // Extract the Item type from `impl Stream<Item = T>`
    let item_type = match &input.sig.output {
        ReturnType::Type(_, ty) => extract_stream_item_type(ty),
        ReturnType::Default => quote! { () },
    };

    // Generate request struct name (for URL params)
    let request_name = format_ident!("{}Request", to_pascal_case(&fn_name_str));

    // Generate server-side code (non-WASM)
    // SSE handlers return Sse<impl Stream<Item = Result<Event, Infallible>>>
    let server_code = if params.is_empty() {
        // No parameters - simple GET endpoint
        quote! {
            #[cfg(not(target_arch = "wasm32"))]
            #vis async fn #fn_name(
            ) -> axum::response::sse::Sse<impl server_fn::prelude::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> + Send> {
                use server_fn::prelude::futures::StreamExt;

                // The body returns impl Stream<Item = T>
                let stream = #body;

                let event_stream = stream.map(|item: #item_type| {
                    let data = serde_json::to_string(&item).unwrap_or_default();
                    Ok(axum::response::sse::Event::default().data(data))
                });

                axum::response::sse::Sse::new(event_stream)
                    .keep_alive(axum::response::sse::KeepAlive::default())
            }
        }
    } else {
        // With parameters - use Query extractor
        quote! {
            #[cfg(not(target_arch = "wasm32"))]
            #vis async fn #fn_name(
                axum::extract::Query(req): axum::extract::Query<#request_name>
            ) -> axum::response::sse::Sse<impl server_fn::prelude::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> + Send> {
                use server_fn::prelude::futures::StreamExt;

                #(let #param_names: #param_types = req.#param_names;)*

                // The body returns impl Stream<Item = T>
                let stream = #body;

                let event_stream = stream.map(|item: #item_type| {
                    let data = serde_json::to_string(&item).unwrap_or_default();
                    Ok(axum::response::sse::Event::default().data(data))
                });

                axum::response::sse::Sse::new(event_stream)
                    .keep_alive(axum::response::sse::KeepAlive::default())
            }
        }
    };

    // Generate client-side code (WASM)
    // Returns SseStream<T> that connects to the endpoint
    let client_code = if params.is_empty() {
        quote! {
            #[cfg(target_arch = "wasm32")]
            #vis fn #fn_name() -> server_fn::sse::SseStream<#item_type> {
                server_fn::sse::SseStream::connect(#endpoint)
            }
        }
    } else {
        // Build URL with query parameters
        quote! {
            #[cfg(target_arch = "wasm32")]
            #vis fn #fn_name(#(#param_names: #param_types),*) -> server_fn::sse::SseStream<#item_type> {
                let req = #request_name { #(#param_names),* };
                let query = serde_urlencoded::to_string(&req).unwrap_or_default();
                let url = format!("{}?{}", #endpoint, query);
                server_fn::sse::SseStream::connect(&url)
            }
        }
    };

    // Generate request struct if there are parameters
    let request_struct = if params.is_empty() {
        quote! {}
    } else {
        quote! {
            #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
            #vis struct #request_name {
                #(pub #param_names: #param_types),*
            }
        }
    };

    let expanded = quote! {
        #request_struct
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

/// Extract the Item type from `impl Stream<Item = T>`
fn extract_stream_item_type(ty: &Type) -> proc_macro2::TokenStream {
    // This is tricky because `impl Stream<Item = T>` is an ImplTrait type
    // We need to find the Item associated type
    if let Type::ImplTrait(impl_trait) = ty {
        for bound in &impl_trait.bounds {
            if let syn::TypeParamBound::Trait(trait_bound) = bound {
                // Look for Stream trait
                if let Some(segment) = trait_bound.path.segments.last() {
                    if segment.ident == "Stream" {
                        if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                            for arg in &args.args {
                                if let syn::GenericArgument::AssocType(assoc) = arg {
                                    if assoc.ident == "Item" {
                                        let item_ty = &assoc.ty;
                                        return quote! { #item_ty };
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // Fallback
    quote! { () }
}
