mod actor;
mod backend;
mod functionality;
// mod logger;
mod attach_stream_to_actor;
mod lsp_actor;
mod lsp_streams;
mod server;
mod util;
// mod lsp_kameo;
mod lsp_actor_service;
mod lsp_streaming_layer;
mod router_layer;

use futures::stream::StreamExt;
use std::{ops::ControlFlow, time::Duration};

use actor::Actor;
use async_lsp::{
    lsp_types::{
        notification::Initialized,
        request::{HoverRequest, Initialize},
        Hover, InitializeParams,
    },
    router::Router,
    ClientSocket,
};
use backend::{db::Jar, Backend};
use functionality::streams::setup_streams;
use lsp_actor::{ActOnNotification, ActOnRequest};
// use functionality::streams::{setup_streams, StreamHandler};
use tower::ServiceBuilder;
// use functionality::streams::handle_lsp_events;
struct TickEvent;

#[tokio_macros::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    // let rx = setup_logger(Level::INFO).unwrap();

    let (server, _) = async_lsp::MainLoop::new_server(|client| {
        let mut backend = Backend::new(client.clone());
        let (actor, actor_ref) = Actor::new(backend);
        let streaming_layer = lsp_streaming_layer::StreamingLayer::new();
        let initialized_stream = streaming_layer.notification_stream::<Initialized>().fuse();
        let initialize_stream = streaming_layer.request_stream::<Initialize>().fuse();
        let hover_stream = streaming_layer.request_stream::<HoverRequest>().fuse();

        let actor_service = lsp_actor_service::LspActorService::new(actor_ref.clone());

        // let router = Router::new(actor_ref.clone());

        ServiceBuilder::new()
            .layer(streaming_layer)
            .service(actor_service)
        // ServiceBuilder::new().layer(streaming_layer).service(router)
    });

    // let (message_senders, message_receivers) = server::setup_message_channels();
    // let (service, socket) =
    //     tower_lsp::LspService::build(|client| Server::new(client, message_senders)).finish();
    // let server = service.inner();

    // let client = server.client.clone();
    // let mut backend = Backend::new(client);

    // separate runtime for the backend
    // let backend_runtime = tokio::runtime::Builder::new_multi_thread()
    //     .worker_threads(4)
    //     .enable_all()
    //     .build()
    //     .unwrap();

    // backend_runtime.spawn(backend.handle_streams());

    // tokio::select! {
    // setup logging
    // _ = handle_log_messages(rx, server.client.clone()) => {},
    // start the server
    // _ = tower_lsp::Server::new(stdin, stdout, socket)
    //     .serve(service) => {}
    // backend
    // _ = functionality::streams::setup_streams(&mut backend, message_receivers) => {}
    // }
}
