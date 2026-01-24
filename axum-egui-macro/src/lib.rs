//! Proc-macro for axum-egui server functions.
//!
//! This macro generates feature-gated code that works with artifact dependencies.
//! Unlike Leptos's server_fn, which uses `cfg!()` at macro-time, this macro
//! generates `#[cfg]` attributes in the output so the correct code path is
//! selected based on the using crate's features at compile time.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    FnArg, Ident, ItemFn, LitStr, Pat, ReturnType, Type, parse::Parse, parse::ParseStream,
    parse_macro_input,
};

/// Configuration parsed from `#[server]` or `#[server("/custom/path")]`
struct ServerFnArgs {
    path: Option<String>,
}

impl Parse for ServerFnArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(ServerFnArgs { path: None });
        }

        let path: LitStr = input.parse()?;
        Ok(ServerFnArgs {
            path: Some(path.value()),
        })
    }
}

/// The `#[server]` macro for defining server functions.
///
/// # Example
///
/// ```ignore
/// use axum_egui::{server, ServerFnError};
///
/// #[server]
/// pub async fn add(a: i32, b: i32) -> Result<i32, ServerFnError> {
///     Ok(a + b)
/// }
///
/// #[server("/custom/api/greet")]
/// pub async fn greet(name: String) -> Result<String, ServerFnError> {
///     Ok(format!("Hello, {}!", name))
/// }
/// ```
///
/// This generates:
/// - A function that executes directly on the server (when `ssr` feature is enabled)
/// - A function that makes an HTTP POST request (when `hydrate` feature is enabled)
/// - An axum handler function `{name}_handler` for server-side routing (ssr only)
/// - An args struct `{Name}Args` for serialization
#[proc_macro_attribute]
pub fn server(args: TokenStream, input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(args as ServerFnArgs);
    let input_fn = parse_macro_input!(input as ItemFn);

    match server_impl(args, input_fn) {
        Ok(tokens) => tokens.into(),
        Err(e) => e.to_compile_error().into(),
    }
}

fn server_impl(args: ServerFnArgs, input_fn: ItemFn) -> syn::Result<TokenStream2> {
    let fn_name = &input_fn.sig.ident;
    let fn_name_str = fn_name.to_string();
    let vis = &input_fn.vis;
    let asyncness = &input_fn.sig.asyncness;
    let generics = &input_fn.sig.generics;
    let where_clause = &input_fn.sig.generics.where_clause;
    let block = &input_fn.block;
    let attrs = &input_fn.attrs;

    // Determine the API path
    let api_path = args.path.unwrap_or_else(|| format!("/api/{}", fn_name_str));

    // Extract function arguments
    let mut arg_names: Vec<Ident> = Vec::new();
    let mut arg_types: Vec<Type> = Vec::new();
    let mut fn_args: Vec<TokenStream2> = Vec::new();

    for arg in &input_fn.sig.inputs {
        match arg {
            FnArg::Typed(pat_type) => {
                if let Pat::Ident(pat_ident) = &*pat_type.pat {
                    let name = &pat_ident.ident;
                    let ty = &*pat_type.ty;
                    arg_names.push(name.clone());
                    arg_types.push(ty.clone());
                    fn_args.push(quote! { #name: #ty });
                }
            }
            FnArg::Receiver(_) => {
                return Err(syn::Error::new_spanned(
                    arg,
                    "server functions cannot have self parameter",
                ));
            }
        }
    }

    // Extract return type
    let return_type = match &input_fn.sig.output {
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &input_fn.sig,
                "server functions must have a return type",
            ));
        }
        ReturnType::Type(_, ty) => ty.clone(),
    };

    // Generate the args struct name (CamelCase)
    let args_struct_name = format_ident!("{}Args", to_pascal_case(&fn_name_str));
    let handler_name = format_ident!("{}_handler", fn_name);

    // Generate field definitions for the args struct
    let struct_fields: Vec<TokenStream2> = arg_names
        .iter()
        .zip(arg_types.iter())
        .map(|(name, ty)| quote! { pub #name: #ty })
        .collect();

    // Generate the output with BOTH code paths wrapped in #[cfg]
    let output = quote! {
        // Args struct - always generated, used by both client and server
        #[derive(::serde::Serialize, ::serde::Deserialize)]
        #vis struct #args_struct_name {
            #(#struct_fields),*
        }

        // The main function - has feature-gated body
        #(#attrs)*
        #vis #asyncness fn #fn_name #generics (#(#fn_args),*) -> #return_type
        #where_clause
        {
            // Server path: execute directly
            #[cfg(feature = "ssr")]
            {
                #block
            }

            // Client path: make HTTP request
            #[cfg(feature = "hydrate")]
            {
                let __args = #args_struct_name { #(#arg_names: #arg_names.clone()),* };
                ::axum_egui::rpc::call(#api_path, &__args).await
            }

            // Fallback for when neither feature is enabled
            #[cfg(not(any(feature = "ssr", feature = "hydrate")))]
            {
                // Silence unused variable warnings
                let _ = (#(&#arg_names),*);
                unreachable!("Either 'ssr' or 'hydrate' feature must be enabled")
            }
        }

        // Server-only: generate the axum handler
        #[cfg(feature = "ssr")]
        #vis async fn #handler_name(
            ::axum::extract::Json(__args): ::axum::extract::Json<#args_struct_name>,
        ) -> impl ::axum::response::IntoResponse {
            use ::axum::response::IntoResponse;

            // Destructure args
            let #args_struct_name { #(#arg_names),* } = __args;

            // Call the actual function and return JSON response
            match #fn_name(#(#arg_names),*).await {
                Ok(result) => (
                    ::axum::http::StatusCode::OK,
                    ::axum::extract::Json(result),
                ).into_response(),
                Err(e) => (
                    ::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    ::axum::extract::Json(::serde_json::json!({ "error": e.to_string() })),
                ).into_response(),
            }
        }
    };

    Ok(output)
}

/// Convert snake_case to PascalCase
fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}
