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
    /// WebSocket bidirectional streaming
    Ws,
}

/// Serialization encoding
#[derive(Debug, Clone, Copy, PartialEq)]
enum Encoding {
    Json,
    MsgPack,
}

/// Parsed server function attributes
struct ServerAttrs {
    mode: ServerMode,
    encoding: Encoding,
}

/// Parse the attribute to determine the mode and encoding
fn parse_attrs(attr: TokenStream) -> ServerAttrs {
    let attr_str = attr.to_string();

    let mode = if attr_str.contains("ws") {
        ServerMode::Ws
    } else if attr_str.contains("sse") {
        ServerMode::Sse
    } else {
        ServerMode::Rpc
    };

    let encoding = if attr_str.contains("msgpack") {
        Encoding::MsgPack
    } else {
        Encoding::Json
    };

    ServerAttrs { mode, encoding }
}

/// Attribute macro that generates both server handler and client caller.
///
/// # Modes
///
/// - `#[server]` - Standard RPC (request/response) with JSON encoding
/// - `#[server(msgpack)]` - Standard RPC with MessagePack encoding (smaller, faster)
/// - `#[server(sse)]` - Server-Sent Events (streaming from server)
/// - `#[server(ws)]` - WebSocket (bidirectional streaming)
///
/// # Encoding
///
/// By default, server functions use JSON for serialization. For better performance
/// with large payloads or frequent calls, use MessagePack:
///
/// ```ignore
/// #[server(msgpack)]
/// pub async fn send_data(payload: Vec<u8>) -> Result<Vec<u8>, ServerFnError> {
///     // MessagePack encoding is ~30% smaller and faster than JSON
///     Ok(payload)
/// }
/// ```
///
/// Note: To use MessagePack, enable the `msgpack` feature in server-fn.
#[proc_macro_attribute]
pub fn server(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attrs = parse_attrs(attr);
    let input = parse_macro_input!(item as ItemFn);

    match attrs.mode {
        ServerMode::Rpc => generate_rpc(input, attrs.encoding),
        ServerMode::Sse => generate_sse(input),
        ServerMode::Ws => generate_ws(input),
    }
}

/// Generate code for standard RPC mode
fn generate_rpc(input: ItemFn, encoding: Encoding) -> TokenStream {
    let fn_name = &input.sig.ident;
    let fn_name_str = fn_name.to_string();
    let endpoint = format!("/api/{}", fn_name_str);

    // Use double underscore prefix to avoid conflicts with user-defined types
    let request_name = format_ident!("__{}Request", to_pascal_case(&fn_name_str));
    let response_name = format_ident!("__{}Response", to_pascal_case(&fn_name_str));

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

    // Extract return type (expecting Result<T, ServerFnError<E>>)
    let return_type = match &input.sig.output {
        ReturnType::Type(_, ty) => extract_result_ok_type(ty),
        ReturnType::Default => quote! { () },
    };

    // Extract error type (expecting Result<T, ServerFnError<E>>)
    let error_type = match &input.sig.output {
        ReturnType::Type(_, ty) => extract_result_err_type(ty),
        ReturnType::Default => quote! { ServerFnError },
    };

    // Generate server-side and client-side code based on encoding
    let (server_code, client_code) = match encoding {
        Encoding::Json => generate_rpc_json(
            vis, asyncness, fn_name, &endpoint, body,
            &param_names, &param_types, &return_type, &error_type,
            &request_name, &response_name,
        ),
        Encoding::MsgPack => generate_rpc_msgpack(
            vis, asyncness, fn_name, &endpoint, body,
            &param_names, &param_types, &return_type, &error_type,
            &request_name, &response_name,
        ),
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

/// Generate JSON-encoded RPC code
fn generate_rpc_json(
    vis: &syn::Visibility,
    asyncness: &Option<syn::token::Async>,
    fn_name: &syn::Ident,
    endpoint: &str,
    body: &syn::Block,
    param_names: &[&syn::Ident],
    param_types: &[&syn::Type],
    return_type: &proc_macro2::TokenStream,
    error_type: &proc_macro2::TokenStream,
    request_name: &syn::Ident,
    response_name: &syn::Ident,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let server_code = quote! {
        #[cfg(not(target_arch = "wasm32"))]
        #vis #asyncness fn #fn_name(
            headers: axum::http::HeaderMap,
            uri: axum::http::Uri,
            axum::Json(req): axum::Json<#request_name>,
        ) -> axum::response::Response {
            use axum::response::IntoResponse;

            #(let #param_names: #param_types = req.#param_names;)*

            // Build request context (IP is extracted from headers like X-Forwarded-For)
            let path = uri.path().to_string();
            let query = uri.query().map(|s| s.to_string());
            let ctx = server_fn::context::RequestContext::from_parts(headers, None, path, query);

            // Run the user's function body with full context available
            let result: Result<#return_type, #error_type> = server_fn::context::with_full_context(ctx, async {
                (|| async #body)().await
            }).await;

            match result {
                Ok(value) => axum::Json(#response_name(value)).into_response(),
                Err(e) => {
                    tracing::error!("Server function error: {:?}", e);
                    // Serialize error as JSON with 400 status
                    let error_json = serde_json::to_string(&e).unwrap_or_else(|_| {
                        r#"{"type":"Custom","data":"Serialization failed"}"#.to_string()
                    });
                    (
                        axum::http::StatusCode::BAD_REQUEST,
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        error_json,
                    ).into_response()
                }
            }
        }
    };

    let client_code = quote! {
        #[cfg(target_arch = "wasm32")]
        #vis async fn #fn_name(#(#param_names: #param_types),*) -> Result<#return_type, #error_type> {
            let req = #request_name { #(#param_names),* };

            let resp = gloo_net::http::Request::post(#endpoint)
                .header("Content-Type", "application/json")
                .body(serde_json::to_string(&req).map_err(|e| ServerFnError::Serialization(e.to_string()))?)?
                .send()
                .await
                .map_err(|e| ServerFnError::Request(e.to_string()))?;

            if !resp.ok() {
                let status = resp.status();
                // Try to parse error as JSON
                let error_text = resp.text().await
                    .map_err(|e| ServerFnError::Deserialization(e.to_string()))?;

                // Try to deserialize as the typed error
                match serde_json::from_str::<#error_type>(&error_text) {
                    Ok(err) => return Err(err),
                    Err(_) => return Err(ServerFnError::ServerError(status)),
                }
            }

            let data: #response_name = resp.json().await
                .map_err(|e| ServerFnError::Deserialization(e.to_string()))?;

            Ok(data.0)
        }
    };

    (server_code, client_code)
}

/// Generate MessagePack-encoded RPC code
fn generate_rpc_msgpack(
    vis: &syn::Visibility,
    asyncness: &Option<syn::token::Async>,
    fn_name: &syn::Ident,
    endpoint: &str,
    body: &syn::Block,
    param_names: &[&syn::Ident],
    param_types: &[&syn::Type],
    return_type: &proc_macro2::TokenStream,
    error_type: &proc_macro2::TokenStream,
    request_name: &syn::Ident,
    response_name: &syn::Ident,
) -> (proc_macro2::TokenStream, proc_macro2::TokenStream) {
    let server_code = quote! {
        #[cfg(not(target_arch = "wasm32"))]
        #vis #asyncness fn #fn_name(
            headers: axum::http::HeaderMap,
            uri: axum::http::Uri,
            body: axum::body::Bytes,
        ) -> axum::response::Response {
            use axum::response::IntoResponse;

            // Deserialize request from MessagePack
            let req: #request_name = match rmp_serde::from_slice(&body) {
                Ok(r) => r,
                Err(e) => {
                    let error_json = format!(r#"{{"type":"Deserialization","data":"{}"}}"#, e);
                    return (
                        axum::http::StatusCode::BAD_REQUEST,
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        error_json,
                    ).into_response();
                }
            };

            #(let #param_names: #param_types = req.#param_names;)*

            // Build request context
            let path = uri.path().to_string();
            let query = uri.query().map(|s| s.to_string());
            let ctx = server_fn::context::RequestContext::from_parts(headers, None, path, query);

            // Run the user's function body with full context available
            let result: Result<#return_type, #error_type> = server_fn::context::with_full_context(ctx, async {
                (|| async #body)().await
            }).await;

            match result {
                Ok(value) => {
                    match rmp_serde::to_vec(&(#response_name(value))) {
                        Ok(bytes) => (
                            axum::http::StatusCode::OK,
                            [(axum::http::header::CONTENT_TYPE, "application/msgpack")],
                            bytes,
                        ).into_response(),
                        Err(e) => {
                            let error_json = format!(r#"{{"type":"Serialization","data":"{}"}}"#, e);
                            (
                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                [(axum::http::header::CONTENT_TYPE, "application/json")],
                                error_json,
                            ).into_response()
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Server function error: {:?}", e);
                    // Serialize error as JSON for consistency
                    let error_json = serde_json::to_string(&e).unwrap_or_else(|_| {
                        r#"{"type":"Custom","data":"Serialization failed"}"#.to_string()
                    });
                    (
                        axum::http::StatusCode::BAD_REQUEST,
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        error_json,
                    ).into_response()
                }
            }
        }
    };

    let client_code = quote! {
        #[cfg(target_arch = "wasm32")]
        #vis async fn #fn_name(#(#param_names: #param_types),*) -> Result<#return_type, #error_type> {
            let req = #request_name { #(#param_names),* };

            // Serialize request as MessagePack
            let body_bytes = rmp_serde::to_vec(&req)
                .map_err(|e| ServerFnError::Serialization(e.to_string()))?;

            let resp = gloo_net::http::Request::post(#endpoint)
                .header("Content-Type", "application/msgpack")
                .body(body_bytes)?
                .send()
                .await
                .map_err(|e| ServerFnError::Request(e.to_string()))?;

            if !resp.ok() {
                let status = resp.status();
                // Errors are always JSON
                let error_text = resp.text().await
                    .map_err(|e| ServerFnError::Deserialization(e.to_string()))?;

                match serde_json::from_str::<#error_type>(&error_text) {
                    Ok(err) => return Err(err),
                    Err(_) => return Err(ServerFnError::ServerError(status)),
                }
            }

            // Deserialize response from MessagePack
            let bytes = resp.binary().await
                .map_err(|e| ServerFnError::Deserialization(e.to_string()))?;

            let data: #response_name = rmp_serde::from_slice(&bytes)
                .map_err(|e| ServerFnError::Deserialization(e.to_string()))?;

            Ok(data.0)
        }
    };

    (server_code, client_code)
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
    // Use double underscore prefix to avoid conflicts with user-defined types
    let request_name = format_ident!("__{}Request", to_pascal_case(&fn_name_str));

    // Generate server-side code (non-WASM)
    // SSE handlers return Sse<impl Stream<Item = Result<Event, Infallible>>>
    let server_code = if params.is_empty() {
        // No parameters - simple GET endpoint
        quote! {
            #[cfg(not(target_arch = "wasm32"))]
            #vis async fn #fn_name(
                headers: axum::http::HeaderMap,
                uri: axum::http::Uri,
            ) -> axum::response::sse::Sse<impl server_fn::prelude::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> + Send> {
                use server_fn::prelude::futures::StreamExt;

                // Build request context
                let path = uri.path().to_string();
                let query = uri.query().map(|s| s.to_string());
                let _ctx = server_fn::context::RequestContext::from_parts(headers, None, path, query);

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
                headers: axum::http::HeaderMap,
                uri: axum::http::Uri,
                axum::extract::Query(req): axum::extract::Query<#request_name>,
            ) -> axum::response::sse::Sse<impl server_fn::prelude::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> + Send> {
                use server_fn::prelude::futures::StreamExt;

                #(let #param_names: #param_types = req.#param_names;)*

                // Build request context
                let path = uri.path().to_string();
                let query_str = uri.query().map(|s| s.to_string());
                let _ctx = server_fn::context::RequestContext::from_parts(headers, None, path, query_str);

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

/// Extract the Error type from Result<T, E>
fn extract_result_err_type(ty: &Type) -> proc_macro2::TokenStream {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            if segment.ident == "Result" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    // Get the second type argument (the error type)
                    let mut iter = args.args.iter();
                    iter.next(); // Skip the Ok type
                    if let Some(syn::GenericArgument::Type(err_type)) = iter.next() {
                        return quote! { #err_type };
                    }
                }
            }
        }
    }
    // Fallback: return ServerFnError
    quote! { ServerFnError }
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

/// Generate code for WebSocket mode
///
/// Expected signature:
/// ```ignore
/// #[server(ws)]
/// pub async fn echo(incoming: impl Stream<Item = String>) -> impl Stream<Item = String> {
///     incoming.map(|msg| format!("Echo: {}", msg))
/// }
/// ```
fn generate_ws(input: ItemFn) -> TokenStream {
    let fn_name = &input.sig.ident;
    let fn_name_str = fn_name.to_string();
    let endpoint = format!("/api/{}", fn_name_str);

    let vis = &input.vis;
    let body = &input.block;

    // Extract the incoming stream parameter
    // We expect exactly one parameter: `incoming: impl Stream<Item = T>`
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

    // Extract the input type from the first parameter (impl Stream<Item = In>)
    let in_type = if let Some((_, ty)) = params.first() {
        extract_stream_item_type(ty)
    } else {
        quote! { () }
    };

    // Extract the output type from return type (impl Stream<Item = Out>)
    let out_type = match &input.sig.output {
        ReturnType::Type(_, ty) => extract_stream_item_type(ty),
        ReturnType::Default => quote! { () },
    };

    // Get the incoming stream parameter name (usually "incoming")
    let incoming_name = if let Some((name, _)) = params.first() {
        quote! { #name }
    } else {
        quote! { _incoming }
    };

    // Generate server-side code (non-WASM)
    let server_code = quote! {
        #[cfg(not(target_arch = "wasm32"))]
        #vis async fn #fn_name(
            headers: axum::http::HeaderMap,
            uri: axum::http::Uri,
            ws: axum::extract::ws::WebSocketUpgrade,
        ) -> impl axum::response::IntoResponse {
            // Build request context before upgrade
            let path = uri.path().to_string();
            let query = uri.query().map(|s| s.to_string());
            let ctx = server_fn::context::RequestContext::from_parts(headers, None, path, query);

            ws.on_upgrade(move |socket| async move {
                use server_fn::prelude::futures::{StreamExt, SinkExt};

                // Make context available during WebSocket handling
                let _ = ctx; // Context is captured and available

                let (mut ws_tx, ws_rx) = socket.split();

                // Create a stream of parsed incoming messages
                let #incoming_name = ws_rx.filter_map(|result| async move {
                    match result {
                        Ok(axum::extract::ws::Message::Text(text)) => {
                            serde_json::from_str::<#in_type>(&text).ok()
                        }
                        Ok(axum::extract::ws::Message::Binary(bytes)) => {
                            serde_json::from_slice::<#in_type>(&bytes).ok()
                        }
                        _ => None,
                    }
                });

                // The user's stream transformation
                let outgoing: std::pin::Pin<Box<dyn server_fn::prelude::Stream<Item = #out_type> + Send>> =
                    Box::pin(#body);

                // Send outgoing messages
                futures::pin_mut!(outgoing);
                while let Some(msg) = outgoing.next().await {
                    if let Ok(json) = serde_json::to_string(&msg) {
                        if ws_tx.send(axum::extract::ws::Message::Text(json.into())).await.is_err() {
                            break;
                        }
                    }
                }
            })
        }
    };

    // Generate client-side code (WASM)
    // Returns WsStream<In, Out> that connects to the endpoint
    let client_code = quote! {
        #[cfg(target_arch = "wasm32")]
        #vis fn #fn_name() -> server_fn::ws::WsStream<#in_type, #out_type> {
            server_fn::ws::WsStream::connect(#endpoint)
        }
    };

    let expanded = quote! {
        #server_code
        #client_code
    };

    TokenStream::from(expanded)
}
