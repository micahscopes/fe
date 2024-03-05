extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, FnArg, ImplItem, ItemImpl, ReturnType};

/// Macro for generating tokio channels from [`lsp-types`](https://docs.rs/lsp-types).
///
/// This procedural macro annotates the `tower_lsp::LanguageServer` trait implementation and generates
/// a struct full of tokio broadcast channels that can be used to signal the server to handle
/// defined requests and notifications.
#[proc_macro_attribute]
pub fn dispatcher(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let lang_server_trait_impl = parse_macro_input!(item as ItemImpl);

    let method_calls = parse_method_calls(&lang_server_trait_impl);
    let channel_struct = gen_channel_struct(&method_calls);

    let tokens = quote! {
        #channel_struct
        #lang_server_trait_impl
    };

    tokens.into()
    // item
}

struct LspTypeChannel<'a> {
    // handler_name: &'a syn::Ident,
    tx_name: syn::Ident,
    dispatcher_name: syn::Ident,
    subscriber_name: syn::Ident,
    rx_name: syn::Ident,
    params: Option<&'a syn::Type>,
    result: Option<&'a syn::Type>,
}

fn parse_method_calls(lang_server_trait: &ItemImpl) -> Vec<LspTypeChannel> {
    let mut calls = Vec::new();

    for item in &lang_server_trait.items {
        let method = match item {
            ImplItem::Fn(m) => m,
            _ => continue,
        };

        let params = method.sig.inputs.iter().nth(1).and_then(|arg| match arg {
            FnArg::Typed(pat) => Some(&*pat.ty),
            _ => None,
        });

        let result = match &method.sig.output {
            ReturnType::Default => None,
            ReturnType::Type(_, ty) => Some(&**ty),
        };

        let handler_name = &method.sig.ident;
        let tx_name = format_ident!("{}_tx", handler_name);
        let dispatcher_name = format_ident!("dispatch_{}", handler_name);
        let subscriber_name = format_ident!("subscribe_{}", handler_name);

        let rx_name = format_ident!("{}_rx", handler_name);

        calls.push(LspTypeChannel {
            tx_name,
            rx_name,
            dispatcher_name,
            subscriber_name,
            params,
            result,
        });
    }

    calls
}

fn gen_channel_struct(channels: &[LspTypeChannel]) -> proc_macro2::TokenStream {
    // unit type
    let unit_type = syn::Type::Tuple(syn::TypeTuple {
        paren_token: syn::token::Paren::default(),
        elems: syn::punctuated::Punctuated::new(),
    });

    let channel_declarations: proc_macro2::TokenStream = channels
        .iter()
        .map(|channel| {
            let tx = &channel.tx_name;
            let rx = &channel.rx_name;
            let params = channel.params;
            let result = channel.result;

            // if params is None we need to use the type of () as the default
            let params = match params {
                Some(params) => params,
                None => &unit_type,
            };

            let sender_type = match result {
                Some(result) => quote! { tokio::sync::broadcast::Sender<(#params, OneshotResponder<#result>)> },
                None => quote! { tokio::sync::broadcast::Sender<#params> },
            };

            let receiver_type = match result {
                Some(result) => quote! { tokio::sync::broadcast::Receiver<(#params, OneshotResponder<#result>)> },
                None => quote! { tokio::sync::broadcast::Receiver<#params> },
            };

            quote! {
                pub #tx: #sender_type,
                pub #rx: #receiver_type,
            }
        })
        .collect();

    let channel_instantiations: proc_macro2::TokenStream = channels
        .iter()
        .map(|channel| {
            let tx = &channel.tx_name;
            let rx = &channel.rx_name;
            quote! {
                let (#tx, #rx) = tokio::sync::broadcast::channel(100);
            }
        })
        .collect();

    let channel_assignments: proc_macro2::TokenStream = channels
        .iter()
        .map(|channel| {
            let tx = &channel.tx_name;
            let rx = &channel.rx_name;
            quote! {
                #tx,
                #rx,
            }
        })
        .collect();

    let dispatch_functions: proc_macro2::TokenStream = channels
        .iter()
        .map(|channel| {
            let tx = &channel.tx_name;
            // let rx = &channel.rx_name;
            let params = &channel.params;
            let params_type = match params {
                Some(params) => params,
                None => &unit_type,
            };
            let subscriber_name = &channel.subscriber_name;
            let dispatcher_name = &channel.dispatcher_name;
            let dispatcher_result = match channel.result {
                Some(result) => quote!{tokio::sync::oneshot::Receiver<#result>},
                None => quote!{()},
                // Some(result) => quote! { tokio::sync::broadcast::Receiver<(#params, OneshotResponder<tokio::sync::oneshot::Sender<#result>>)> },
                // None => quote! { tokio::sync::broadcast::Receiver<#params> },
            };
            let receiver_type = match channel.result {
                Some(result) => quote! { tokio::sync::broadcast::Receiver<(#params_type, OneshotResponder<#result>)> },
                None => quote! { tokio::sync::broadcast::Receiver<#params_type> },
            };

            let dispatcher_payload = match params {
                Some(_params) => quote! { params },
                None => quote! { () },
            };

            let dispatcher_send_payload = match channel.result {
                Some(result) => quote!{
                    let (tx, rx) = tokio::sync::oneshot::channel::<#result>();
                    let oneshot = OneshotResponder::from(tx);
                    let broadcast = self.#tx.clone();
                    info!("sending oneshot sender: {:?}", #dispatcher_payload);
                    match broadcast.send((#dispatcher_payload, oneshot)) {
                        Ok(_) => info!("sent oneshot sender"),
                        Err(e) => error!("failed to send oneshot sender"),
                    }
                    info!("returning oneshot receiver: {:?}", rx);
                    rx
                },
                None => quote!{
                    match self.#tx.send(#dispatcher_payload) {
                        Ok(_) => info!("sent notification"),
                        Err(e) => error!("failed to send notification: {:?}", e),
                    }
                },
            };

            let dispatcher_fn = match params {
                Some(params) => quote! {
                    pub fn #dispatcher_name(&self, params: #params) -> #dispatcher_result {
                        #dispatcher_send_payload
                    }
                },
                None => quote! {
                    pub fn #dispatcher_name(&self) -> #dispatcher_result {
                        #dispatcher_send_payload
                    }
                },
            };

            let subscriber_fn = match params {
                Some(_params) => quote! {
                    pub fn #subscriber_name(&self) -> #receiver_type {
                        self.#tx.subscribe()
                    }
                },
                None => quote! {
                    pub fn #subscriber_name(&self) -> #receiver_type {
                        self.#tx.subscribe()
                    }
                },
            };

            quote! {
                #dispatcher_fn
                #subscriber_fn
            }
        })
        .collect();

    quote! {
        pub struct LspChannels {
            #channel_declarations
        }

        impl LspChannels {
            pub fn new() -> Self {
                #channel_instantiations
                Self {
                    #channel_assignments
                }
            }
            #dispatch_functions
        }
    }
}
