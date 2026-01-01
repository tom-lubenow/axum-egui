//! Proc macros for axum-egui server functions.
//!
//! This crate provides the `#[server]` attribute macro for defining
//! functions that work on both server and client.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    FnArg, Ident, ItemFn, Pat, PatType, ReturnType, Token, Type, TypePath, parse::Parse,
    parse::ParseStream, parse_macro_input,
};

/// Configuration for the server function.
#[derive(Default)]
struct ServerConfig {
    /// Custom endpoint path (defaults to /api/{fn_name})
    endpoint: Option<String>,
}

impl Parse for ServerConfig {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut config = ServerConfig::default();

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "endpoint" => {
                    let _: Token![=] = input.parse()?;
                    let lit: syn::LitStr = input.parse()?;
                    config.endpoint = Some(lit.value());
                }
                _ => return Err(syn::Error::new(ident.span(), "unknown attribute")),
            }
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }

        Ok(config)
    }
}

/// Mark a function as a server function.
///
/// On the server, this function executes normally.
/// On the client (WASM), this function makes an HTTP request to the server.
///
/// # Example
///
/// ```ignore
/// use axum_egui::server;
///
/// #[server]
/// pub async fn greet(name: String) -> Result<String, ServerFnError> {
///     Ok(format!("Hello, {}!", name))
/// }
///
/// // Custom endpoint path
/// #[server(endpoint = "/api/v2/greet")]
/// pub async fn greet_v2(name: String) -> Result<String, ServerFnError> {
///     Ok(format!("Hello, {}!", name))
/// }
/// ```
#[proc_macro_attribute]
pub fn server(attr: TokenStream, item: TokenStream) -> TokenStream {
    let config = parse_macro_input!(attr as ServerConfig);
    let input = parse_macro_input!(item as ItemFn);

    match server_impl(config, input) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn server_impl(config: ServerConfig, input: ItemFn) -> syn::Result<TokenStream2> {
    let fn_name = &input.sig.ident;
    let vis = &input.vis;
    let asyncness = &input.sig.asyncness;

    if asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            input.sig.fn_token,
            "server functions must be async",
        ));
    }

    // Extract function arguments
    let args: Vec<_> = input.sig.inputs.iter().collect();
    let (arg_names, arg_types): (Vec<_>, Vec<_>) = args
        .iter()
        .filter_map(|arg| {
            if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
                if let Pat::Ident(pat_ident) = pat.as_ref() {
                    return Some((pat_ident.ident.clone(), ty.as_ref().clone()));
                }
            }
            None
        })
        .unzip();

    // Generate request struct name
    let request_struct_name = format_ident!("{}Request", to_pascal_case(&fn_name.to_string()));

    // Extract the success type from Result<T, E>
    let return_type = match &input.sig.output {
        ReturnType::Type(_, ty) => extract_result_ok_type(ty)?,
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &input.sig,
                "server functions must return Result<T, ServerFnError>",
            ));
        }
    };

    // Get the endpoint path
    let endpoint = config
        .endpoint
        .unwrap_or_else(|| format!("/api/{}", fn_name));

    // Generate the request struct (with same visibility as the function)
    let request_struct = if arg_names.is_empty() {
        quote! {}
    } else {
        quote! {
            #[derive(::serde::Serialize, ::serde::Deserialize)]
            #[allow(non_camel_case_types)]
            #vis struct #request_struct_name {
                #(pub #arg_names: #arg_types),*
            }
        }
    };

    // Original function body
    let block = &input.block;
    let original_output = &input.sig.output;

    // Generate server-side code (native target)
    let server_code = {
        let handler_name = format_ident!("{}_handler", fn_name);

        if arg_names.is_empty() {
            // No arguments - GET request
            quote! {
                #[cfg(not(target_arch = "wasm32"))]
                #vis async fn #fn_name() #original_output {
                    #block
                }

                #[cfg(not(target_arch = "wasm32"))]
                #vis async fn #handler_name() -> impl ::axum::response::IntoResponse {
                    use ::axum::response::IntoResponse;
                    match #fn_name().await {
                        Ok(value) => ::axum::Json(value).into_response(),
                        Err(e) => (
                            ::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            e.to_string(),
                        ).into_response(),
                    }
                }
            }
        } else {
            // Has arguments - POST request
            quote! {
                #[cfg(not(target_arch = "wasm32"))]
                #request_struct

                #[cfg(not(target_arch = "wasm32"))]
                #vis async fn #fn_name(#(#arg_names: #arg_types),*) #original_output {
                    #block
                }

                #[cfg(not(target_arch = "wasm32"))]
                #vis async fn #handler_name(
                    ::axum::Json(req): ::axum::Json<#request_struct_name>,
                ) -> impl ::axum::response::IntoResponse {
                    use ::axum::response::IntoResponse;
                    match #fn_name(#(req.#arg_names),*).await {
                        Ok(value) => ::axum::Json(value).into_response(),
                        Err(e) => (
                            ::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                            e.to_string(),
                        ).into_response(),
                    }
                }
            }
        }
    };

    // Generate client-side code (WASM target)
    let client_code = {
        if arg_names.is_empty() {
            // No arguments - GET request
            quote! {
                #[cfg(target_arch = "wasm32")]
                #vis async fn #fn_name() -> ::std::result::Result<#return_type, ::axum_egui::ServerFnError> {
                    use ::axum_egui::__private::gloo_net::http::Request;

                    let response = Request::get(#endpoint)
                        .send()
                        .await
                        .map_err(|e| ::axum_egui::ServerFnError::Request(e.to_string()))?;

                    if !response.ok() {
                        let text = response.text().await.unwrap_or_default();
                        return Err(::axum_egui::ServerFnError::ServerError(text));
                    }

                    response
                        .json()
                        .await
                        .map_err(|e| ::axum_egui::ServerFnError::Deserialization(e.to_string()))
                }
            }
        } else {
            // Has arguments - POST request
            quote! {
                #[cfg(target_arch = "wasm32")]
                #request_struct

                #[cfg(target_arch = "wasm32")]
                #vis async fn #fn_name(#(#arg_names: #arg_types),*) -> ::std::result::Result<#return_type, ::axum_egui::ServerFnError> {
                    use ::axum_egui::__private::gloo_net::http::Request;

                    let request = #request_struct_name {
                        #(#arg_names),*
                    };

                    let body = ::axum_egui::__private::serde_json::to_string(&request)
                        .map_err(|e| ::axum_egui::ServerFnError::Serialization(e.to_string()))?;

                    let response = Request::post(#endpoint)
                        .header("Content-Type", "application/json")
                        .body(&body)
                        .map_err(|e| ::axum_egui::ServerFnError::Request(e.to_string()))?
                        .send()
                        .await
                        .map_err(|e| ::axum_egui::ServerFnError::Request(e.to_string()))?;

                    if !response.ok() {
                        let text = response.text().await.unwrap_or_default();
                        return Err(::axum_egui::ServerFnError::ServerError(text));
                    }

                    response
                        .json()
                        .await
                        .map_err(|e| ::axum_egui::ServerFnError::Deserialization(e.to_string()))
                }
            }
        }
    };

    Ok(quote! {
        #server_code
        #client_code
    })
}

/// Extract T from Result<T, E>
fn extract_result_ok_type(ty: &Type) -> syn::Result<&Type> {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(segment) = path.segments.last() {
            if segment.ident == "Result" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(syn::GenericArgument::Type(ok_type)) = args.args.first() {
                        return Ok(ok_type);
                    }
                }
            }
        }
    }
    Err(syn::Error::new_spanned(
        ty,
        "expected Result<T, ServerFnError>",
    ))
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
