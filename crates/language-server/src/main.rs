mod backend;
mod functionality;
// mod logger;
mod lsp_actor;
mod lsp_streams;
mod server;
mod util;
// mod lsp_kameo;

use std::{ops::ControlFlow, time::Duration};

use async_lsp::{lsp_types::request::Initialize, router::Router, ClientSocket};
use backend::{db::Jar, Backend};
use functionality::{actor::Actor, streams::setup_streams};
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
        let (mut actor, actor_ref) = Actor::new(backend);

        actor.register_request_handler::<Initialize>();

        let mut router = Router::new(actor_ref.clone());

        // let backend = Backend::new(client.clone());

        tokio::spawn({
            let client = client.clone();
            async move {
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                loop {
                    // interval.tick().await;
                    if client.emit(TickEvent).is_err() {
                        break;
                    }
                }
            }
        });

        router
            // .request::<async_lsp::lsp_types::request::Initialize, _>(|st, params| async move {
            //     st.ask(params).send()
            // })
            .event::<TickEvent>(|st, _| {
                // info!("tick");
                // st.counter += 1;
                ControlFlow::Continue(())
            });
        setup_streams(&mut router, actor_ref.clone());
        let service = ServiceBuilder::new().service(router);
        service
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
