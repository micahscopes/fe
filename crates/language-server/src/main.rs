mod backend;
mod capabilities;
mod db;
mod diagnostics;
mod globals;
mod goto;
mod language_server;
mod logger;
mod oneshot_responder;
mod util;
mod workspace;

use backend::Backend;
use db::Jar;

use language_server::Server;

use crate::logger::{handle_log_messages, setup_logger};

mod handlers {
    pub mod request;
}

#[tokio_macros::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = tower_lsp::LspService::build(Server::new).finish();
    let server = service.inner();

    let client = server.client.clone();
    let messaging = server.messaging.clone();
    let backend = Backend::new(client, messaging);

    let rx = setup_logger(log::Level::Info).unwrap();

    // separate runtime for the backend
    let backend_runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();

    backend_runtime.spawn(backend.handle_streams());

    tokio::select! {
        // setup logging
        _ = handle_log_messages(rx, server.client.clone()) => {},
        // start the server
        _ = tower_lsp::Server::new(stdin, stdout, socket)
            .serve(service) => {}
    }
}
