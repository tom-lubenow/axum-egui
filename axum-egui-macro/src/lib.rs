//! Proc-macro for axum-egui server functions.
//!
//! This macro generates feature-gated code that works with artifact dependencies.
//! Unlike Leptos's server_fn, which uses `cfg!()` at macro-time, this macro
//! generates `#[cfg]` attributes in the output so the correct code path is
//! selected based on the using crate's features at compile time.
//!
//! # Features
//!
//! - **Axum extractor access**: Use `#[extract]` on parameters to inject axum extractors
//!   (server-only, omitted from client-side function signature)
//! - **Streaming**: Use `#[server(stream)]` to return SSE streams instead of single values
//! - **Middleware/guards**: Use `#[server(middleware = "my_middleware")]` to wrap handlers

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    Attribute, FnArg, GenericParam, Ident, ItemFn, LitStr, Pat, PatType, ReturnType, Token, Type,
    TypePath, parse::Parse, parse::ParseStream, parse_macro_input,
};

/// Configuration parsed from `#[server]`, `#[server("/custom/path")]`,
/// `#[server(stream)]`, `#[server(stream, endpoint = "/api/counter")]`,
/// or `#[server(middleware = "require_auth")]`.
struct ServerFnArgs {
    path: Option<String>,
    stream: bool,
    middleware: Option<String>,
}

impl Parse for ServerFnArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(ServerFnArgs {
                path: None,
                stream: false,
                middleware: None,
            });
        }

        // Try to parse as a plain string literal first (backward-compatible)
        if input.peek(LitStr) {
            let path: LitStr = input.parse()?;
            return Ok(ServerFnArgs {
                path: Some(path.value()),
                stream: false,
                middleware: None,
            });
        }

        // Otherwise parse named arguments: stream, endpoint = "...", middleware = "..."
        let mut path = None;
        let mut stream = false;
        let mut middleware = None;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            match ident.to_string().as_str() {
                "stream" => {
                    stream = true;
                }
                "endpoint" => {
                    let _: Token![=] = input.parse()?;
                    let lit: LitStr = input.parse()?;
                    path = Some(lit.value());
                }
                "middleware" => {
                    let _: Token![=] = input.parse()?;
                    let lit: LitStr = input.parse()?;
                    middleware = Some(lit.value());
                }
                other => {
                    return Err(syn::Error::new(
                        ident.span(),
                        format!(
                            "unknown server function attribute `{}`. \
                             Expected `stream`, `endpoint = \"...\"`, or `middleware = \"...\"`",
                            other
                        ),
                    ));
                }
            }

            // Consume optional comma
            if input.peek(Token![,]) {
                let _: Token![,] = input.parse()?;
            }
        }

        Ok(ServerFnArgs {
            path,
            stream,
            middleware,
        })
    }
}

/// Validate that an API path is safe and well-formed.
///
/// This prevents:
/// - Path traversal attacks (e.g., `/api/../../etc/passwd`)
/// - Malformed paths (double slashes, missing leading slash)
/// - Invalid characters that could cause routing issues
fn validate_api_path(path: &str, span: Span) -> syn::Result<()> {
    // Must start with /
    if !path.starts_with('/') {
        return Err(syn::Error::new(span, "API path must start with '/'"));
    }

    // No path traversal
    if path.contains("..") {
        return Err(syn::Error::new(
            span,
            "API path must not contain '..' (path traversal is not allowed for security reasons)",
        ));
    }

    // No double slashes
    if path.contains("//") {
        return Err(syn::Error::new(
            span,
            "API path must not contain '//' (double slashes)",
        ));
    }

    // Valid URL characters only (alphanumeric, /, -, _)
    for c in path.chars() {
        if !c.is_ascii_alphanumeric() && !"-_/".contains(c) {
            return Err(syn::Error::new(
                span,
                format!(
                    "API path contains invalid character '{}'. \
                    Allowed characters: alphanumeric, '/', '-', '_'",
                    c
                ),
            ));
        }
    }

    // Must not end with / (except for root path "/")
    if path.len() > 1 && path.ends_with('/') {
        return Err(syn::Error::new(
            span,
            "API path must not end with '/' (trailing slash)",
        ));
    }

    Ok(())
}

/// Validate that the return type is `Result<T, ServerFnError>`.
fn validate_return_type(ret: &ReturnType) -> syn::Result<()> {
    match ret {
        ReturnType::Default => Err(syn::Error::new_spanned(
            ret,
            "server functions must return `Result<T, ServerFnError>`. \
                The #[server] macro generates code that serializes the return value, \
                so a Result type is required to handle potential errors.",
        )),
        ReturnType::Type(_, ty) => {
            // Check if it's Result<_, _>
            if let Type::Path(TypePath { path, .. }) = ty.as_ref()
                && let Some(seg) = path.segments.last()
            {
                if seg.ident == "Result" {
                    // Could add more detailed validation of generic args here,
                    // but checking for Result is the main requirement
                    return Ok(());
                }
                return Err(syn::Error::new_spanned(
                    ty,
                    format!(
                        "server functions must return `Result<T, ServerFnError>`, found `{}`. \
                        The #[server] macro generates code that handles both success and error cases, \
                        so a Result type is required.",
                        seg.ident
                    ),
                ));
            }
            Ok(())
        }
    }
}

/// Check if the function has generic type parameters.
fn validate_generics(generics: &syn::Generics) -> syn::Result<()> {
    for param in &generics.params {
        if let GenericParam::Type(type_param) = param {
            return Err(syn::Error::new_spanned(
                type_param,
                format!(
                    "server functions do not currently support generic type parameters like `{}`. \
                    The #[server] macro generates a concrete args struct for serialization, \
                    which requires known types at compile time. Consider using a concrete type \
                    or an enum to represent different data shapes.",
                    type_param.ident
                ),
            ));
        }
    }
    Ok(())
}

/// Check whether a `PatType` has an `#[extract]` attribute.
fn has_extract_attr(pat_type: &PatType) -> bool {
    pat_type
        .attrs
        .iter()
        .any(|attr| attr.path().is_ident("extract"))
}

/// Remove the `#[extract]` attribute from a list of attributes.
fn strip_extract_attr(attrs: &[Attribute]) -> Vec<Attribute> {
    attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("extract"))
        .cloned()
        .collect()
}

/// The `#[server]` macro for defining server functions.
///
/// # Basic Usage
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
/// # Axum Extractor Access
///
/// Use `#[extract]` on parameters to inject axum extractors. These parameters
/// are server-only and omitted from the client-side function signature.
///
/// ```ignore
/// use axum::extract::State;
///
/// #[server]
/// pub async fn get_user(
///     #[extract] State(db): State<DbPool>,
///     user_id: i32,
/// ) -> Result<User, ServerFnError> {
///     db.get_user(user_id).await.map_err(|e| ServerFnError::ServerError(e.to_string()))
/// }
/// ```
///
/// # Streaming Server Functions
///
/// Use `#[server(stream)]` to return an SSE stream. On the server, the function
/// body should return a `Result<impl Stream<Item = T>, ServerFnError>`. On the
/// client, the generated function returns an `SseStream<T>`.
///
/// ```ignore
/// #[server(stream)]
/// pub async fn counter_stream() -> Result<impl Stream<Item = i32>, ServerFnError> {
///     Ok(futures_util::stream::unfold(0, |n| async move {
///         tokio::time::sleep(Duration::from_secs(1)).await;
///         Some((n, n + 1))
///     }))
/// }
/// ```
///
/// # Middleware / Guards
///
/// Use `#[server(middleware = "my_fn")]` to wrap the generated handler in a
/// middleware layer. The named function must return an axum middleware layer.
///
/// ```ignore
/// #[server(middleware = "require_auth")]
/// pub async fn admin_action() -> Result<String, ServerFnError> {
///     Ok("done".into())
/// }
/// ```
///
/// This generates:
/// - A function that executes directly on the server (when `ssr` feature is enabled)
/// - A function that makes an HTTP POST/GET request (when `hydrate` feature is enabled)
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

/// Represents a parsed function parameter, classified by kind.
struct ParsedParam {
    /// The parameter name (identifier) for the args struct or simple reference.
    name: Ident,
    /// The parameter type.
    ty: Type,
    /// The full pattern (may include destructuring, e.g., `State(db)`).
    pat: Box<Pat>,
    /// Whether this parameter is an extractor (`#[extract]`).
    is_extract: bool,
    /// Attributes on this parameter (with `#[extract]` stripped).
    attrs: Vec<Attribute>,
    /// The synthetic handler parameter name (used for extractors in the handler).
    /// For extractors, we use a generated name like `__extractor_0` to accept the
    /// whole extractor value, then pass it to the function which does its own
    /// destructuring.
    handler_param_name: Ident,
}

fn server_impl(args: ServerFnArgs, input_fn: ItemFn) -> syn::Result<TokenStream2> {
    let fn_name = &input_fn.sig.ident;
    let fn_name_str = fn_name.to_string();
    let vis = &input_fn.vis;
    let block = &input_fn.block;
    let attrs = &input_fn.attrs;
    let generics = &input_fn.sig.generics;
    let where_clause = &input_fn.sig.generics.where_clause;

    // Validate generics (not supported yet)
    validate_generics(generics)?;

    // Validate return type
    validate_return_type(&input_fn.sig.output)?;

    // Determine the API path and validate it
    let api_path = args.path.unwrap_or_else(|| format!("/api/{}", fn_name_str));
    validate_api_path(&api_path, Span::call_site())?;

    // Parse and classify function arguments
    let mut params: Vec<ParsedParam> = Vec::new();
    let mut extract_count = 0usize;

    for arg in &input_fn.sig.inputs {
        match arg {
            FnArg::Typed(pat_type) => {
                let is_extract = has_extract_attr(pat_type);
                let stripped_attrs = strip_extract_attr(&pat_type.attrs);

                let name = match &*pat_type.pat {
                    Pat::Ident(pat_ident) => pat_ident.ident.clone(),
                    _ if is_extract => {
                        // For extractors with complex patterns, generate a synthetic name
                        format_ident!("__extract_{}", extract_count)
                    }
                    _ => {
                        return Err(syn::Error::new_spanned(
                            &pat_type.pat,
                            "non-extractor parameters must use simple identifier patterns \
                             (e.g., `name: String`). Complex patterns are only supported \
                             with `#[extract]`.",
                        ));
                    }
                };

                let handler_param_name = if is_extract {
                    let n = format_ident!("__extractor_{}", extract_count);
                    extract_count += 1;
                    n
                } else {
                    name.clone()
                };

                params.push(ParsedParam {
                    name,
                    ty: (*pat_type.ty).clone(),
                    pat: pat_type.pat.clone(),
                    is_extract,
                    attrs: stripped_attrs,
                    handler_param_name,
                });
            }
            FnArg::Receiver(_) => {
                return Err(syn::Error::new_spanned(
                    arg,
                    "server functions cannot have `self` parameter. \
                    The #[server] macro generates a standalone handler function \
                    that cannot access struct state. Pass any required data as \
                    function arguments instead.",
                ));
            }
        }
    }

    // Split into extractors and serialized args
    let extract_params: Vec<&ParsedParam> = params.iter().filter(|p| p.is_extract).collect();
    let arg_params: Vec<&ParsedParam> = params.iter().filter(|p| !p.is_extract).collect();

    // Collect names and types for args
    let arg_names: Vec<&Ident> = arg_params.iter().map(|p| &p.name).collect();
    let arg_types: Vec<&Type> = arg_params.iter().map(|p| &p.ty).collect();

    // Extract return type (already validated above)
    let return_type = match &input_fn.sig.output {
        ReturnType::Default => {
            return Err(syn::Error::new_spanned(
                &input_fn.sig,
                "server functions must return `Result<T, ServerFnError>`",
            ));
        }
        ReturnType::Type(_, ty) => ty.clone(),
    };

    // Generate the args struct name (CamelCase)
    let args_struct_name = format_ident!("{}Args", to_pascal_case(&fn_name_str));
    let handler_name = format_ident!("{}_handler", fn_name);

    // Generate struct fields for serialized args
    let struct_fields: Vec<TokenStream2> = arg_names
        .iter()
        .zip(arg_types.iter())
        .map(|(name, ty)| quote! { pub #name: #ty })
        .collect();

    // === Server function params (extractors + args with original patterns) ===
    let server_fn_params: Vec<TokenStream2> = extract_params
        .iter()
        .map(|p| {
            let pat = &p.pat;
            let ty = &p.ty;
            let attrs = &p.attrs;
            quote! { #(#attrs)* #pat: #ty }
        })
        .chain(
            arg_names
                .iter()
                .zip(arg_types.iter())
                .map(|(name, ty)| quote! { #name: #ty }),
        )
        .collect();

    // === Client function params (only serialized args) ===
    let client_fn_params: Vec<TokenStream2> = arg_names
        .iter()
        .zip(arg_types.iter())
        .map(|(name, ty)| quote! { #name: #ty })
        .collect();

    // === Handler extractor params: use generated names with the extractor type ===
    // In the handler, we use `__extractor_N: ExtractorType` so the value is not
    // destructured. Then we pass the whole value to the function, which does
    // its own destructuring via its pattern.
    let handler_extract_params: Vec<TokenStream2> = extract_params
        .iter()
        .map(|p| {
            let handler_name = &p.handler_param_name;
            let ty = &p.ty;
            let attrs = &p.attrs;
            quote! { #(#attrs)* #handler_name: #ty }
        })
        .collect();

    // === Handler call args: pass extractor values (whole) then arg names ===
    let handler_call_args: Vec<TokenStream2> = extract_params
        .iter()
        .map(|p| {
            let handler_name = &p.handler_param_name;
            quote! { #handler_name }
        })
        .chain(arg_names.iter().map(|name| quote! { #name }))
        .collect();

    let middleware_wrapper = generate_middleware_wrapper(&args.middleware, &handler_name);

    if args.stream {
        generate_streaming(
            vis,
            generics,
            where_clause,
            fn_name,
            &handler_name,
            &args_struct_name,
            attrs,
            block,
            &return_type,
            &api_path,
            &struct_fields,
            &server_fn_params,
            &client_fn_params,
            &handler_extract_params,
            &handler_call_args,
            &arg_names,
            &middleware_wrapper,
        )
    } else {
        generate_regular(
            vis,
            generics,
            where_clause,
            fn_name,
            &handler_name,
            &args_struct_name,
            attrs,
            block,
            &return_type,
            &api_path,
            &struct_fields,
            &server_fn_params,
            &client_fn_params,
            &handler_extract_params,
            &handler_call_args,
            &arg_names,
            &middleware_wrapper,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn generate_regular(
    vis: &syn::Visibility,
    generics: &syn::Generics,
    where_clause: &Option<syn::WhereClause>,
    fn_name: &Ident,
    handler_name: &Ident,
    args_struct_name: &Ident,
    attrs: &[Attribute],
    block: &syn::Block,
    return_type: &Type,
    api_path: &str,
    struct_fields: &[TokenStream2],
    server_fn_params: &[TokenStream2],
    client_fn_params: &[TokenStream2],
    handler_extract_params: &[TokenStream2],
    handler_call_args: &[TokenStream2],
    arg_names: &[&Ident],
    middleware_wrapper: &TokenStream2,
) -> syn::Result<TokenStream2> {
    let output = quote! {
        // Args struct - always generated, used by both client and server
        #[derive(::serde::Serialize, ::serde::Deserialize)]
        #vis struct #args_struct_name {
            #(#struct_fields),*
        }

        // Server path: function takes extractors + args
        #[cfg(feature = "ssr")]
        #(#attrs)*
        #vis async fn #fn_name #generics (#(#server_fn_params),*) -> #return_type
        #where_clause
        {
            #block
        }

        // Client path: function takes only serialized args
        #[cfg(feature = "hydrate")]
        #(#attrs)*
        #vis async fn #fn_name #generics (#(#client_fn_params),*) -> #return_type
        #where_clause
        {
            let __args = #args_struct_name { #(#arg_names: #arg_names.clone()),* };
            ::axum_egui::rpc::call(#api_path, &__args).await
        }

        // Fallback for when neither feature is enabled
        #[cfg(not(any(feature = "ssr", feature = "hydrate")))]
        #(#attrs)*
        #vis async fn #fn_name #generics (#(#client_fn_params),*) -> #return_type
        #where_clause
        {
            let _ = (#(&#arg_names),*);
            unreachable!("Either 'ssr' or 'hydrate' feature must be enabled")
        }

        // Server-only: generate the axum handler
        #[cfg(feature = "ssr")]
        #vis async fn #handler_name(
            #(#handler_extract_params,)*
            ::axum::extract::Json(__args): ::axum::extract::Json<#args_struct_name>,
        ) -> impl ::axum::response::IntoResponse {
            use ::axum::response::IntoResponse;

            // Destructure args
            let #args_struct_name { #(#arg_names),* } = __args;

            // Call the actual function and return JSON response
            match #fn_name(#(#handler_call_args),*).await {
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

        #middleware_wrapper
    };

    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn generate_streaming(
    vis: &syn::Visibility,
    generics: &syn::Generics,
    where_clause: &Option<syn::WhereClause>,
    fn_name: &Ident,
    handler_name: &Ident,
    args_struct_name: &Ident,
    attrs: &[Attribute],
    block: &syn::Block,
    return_type: &Type,
    api_path: &str,
    struct_fields: &[TokenStream2],
    server_fn_params: &[TokenStream2],
    client_fn_params: &[TokenStream2],
    handler_extract_params: &[TokenStream2],
    handler_call_args: &[TokenStream2],
    arg_names: &[&Ident],
    middleware_wrapper: &TokenStream2,
) -> syn::Result<TokenStream2> {
    // For the streaming handler, use Query instead of Json for GET (SSE convention).
    let handler_args_extraction = if arg_names.is_empty() {
        quote! {}
    } else {
        quote! {
            ::axum::extract::Query(__args): ::axum::extract::Query<#args_struct_name>,
        }
    };

    let handler_destructure = if arg_names.is_empty() {
        quote! {}
    } else {
        quote! {
            let #args_struct_name { #(#arg_names),* } = __args;
        }
    };

    let output = quote! {
        // Args struct - always generated, used by both client and server
        #[derive(::serde::Serialize, ::serde::Deserialize)]
        #vis struct #args_struct_name {
            #(#struct_fields),*
        }

        // Server path: function with full signature including extractors
        #[cfg(feature = "ssr")]
        #(#attrs)*
        #vis async fn #fn_name #generics (#(#server_fn_params),*) -> #return_type
        #where_clause
        {
            #block
        }

        // Client path: returns SseStream wrapping the SSE endpoint
        #[cfg(feature = "hydrate")]
        #(#attrs)*
        #vis fn #fn_name #generics (#(#client_fn_params),*) -> ::std::result::Result<
            ::axum_egui::sse::SseStream<::serde_json::Value>,
            ::axum_egui::rpc::ServerFnError,
        >
        #where_clause
        {
            let __args = #args_struct_name { #(#arg_names: #arg_names.clone()),* };
            let __query = ::serde_json::to_value(&__args)
                .map_err(|e| ::axum_egui::rpc::ServerFnError::Serialization(e.to_string()))?;
            let __query_string = if let ::serde_json::Value::Object(map) = &__query {
                let pairs: ::std::vec::Vec<String> = map.iter().map(|(k, v)| {
                    format!("{}={}", k, v)
                }).collect();
                if pairs.is_empty() {
                    ::std::string::String::new()
                } else {
                    format!("?{}", pairs.join("&"))
                }
            } else {
                ::std::string::String::new()
            };
            let __url = format!("{}{}", #api_path, __query_string);
            ::axum_egui::sse::SseStream::connect(&__url)
                .map_err(|e| ::axum_egui::rpc::ServerFnError::Request(format!("{}", e)))
        }

        // Fallback for when neither feature is enabled
        #[cfg(not(any(feature = "ssr", feature = "hydrate")))]
        #(#attrs)*
        #vis fn #fn_name #generics (#(#client_fn_params),*) -> ::std::result::Result<(), ::axum_egui::rpc::ServerFnError>
        #where_clause
        {
            let _ = (#(&#arg_names),*);
            unreachable!("Either 'ssr' or 'hydrate' feature must be enabled")
        }

        // Server-only: generate the SSE axum handler
        #[cfg(feature = "ssr")]
        #vis async fn #handler_name(
            #(#handler_extract_params,)*
            #handler_args_extraction
        ) -> impl ::axum::response::IntoResponse {
            use ::axum::response::IntoResponse;

            #handler_destructure

            match #fn_name(#(#handler_call_args),*).await {
                Ok(stream) => {
                    use ::futures_util::StreamExt;
                    let sse_stream = stream.map(|item| {
                        let event = ::axum_egui::sse::Event::new()
                            .json_data(&item)
                            .unwrap_or_else(|e| {
                                ::axum_egui::sse::Event::new()
                                    .data(format!("serialization error: {}", e))
                            });
                        let axum_event: ::axum::response::sse::Event = event.into();
                        Ok::<_, ::std::convert::Infallible>(axum_event)
                    });
                    ::axum_egui::sse::Sse::new(sse_stream)
                        .keep_alive(::axum_egui::sse::KeepAlive::default())
                        .into_response()
                }
                Err(e) => (
                    ::axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    ::axum::extract::Json(::serde_json::json!({ "error": e.to_string() })),
                ).into_response(),
            }
        }

        #middleware_wrapper
    };

    Ok(output)
}

/// Generate optional middleware wrapper function.
///
/// When `middleware = "fn_name"` is specified, generates a `{handler}_route` function
/// that returns an axum `MethodRouter` with the middleware layer applied.
fn generate_middleware_wrapper(middleware: &Option<String>, handler_name: &Ident) -> TokenStream2 {
    match middleware {
        Some(mw_name) => {
            let mw_ident = format_ident!("{}", mw_name);
            let route_fn_name = format_ident!("{}_route", handler_name);
            quote! {
                /// Returns a `MethodRouter` with the middleware layer applied.
                /// Use this with `Router::route()` instead of `post(handler)`.
                #[cfg(feature = "ssr")]
                pub fn #route_fn_name() -> ::axum::routing::MethodRouter {
                    use ::axum::routing::post;
                    post(#handler_name).layer(#mw_ident())
                }
            }
        }
        None => quote! {},
    }
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
