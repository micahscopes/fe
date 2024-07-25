mod actor;
mod attach_stream_to_actor;
mod backend;
mod functionality;
mod lsp_actor;
mod lsp_actor_service;
mod lsp_streaming_layer;
mod lsp_streams;
mod server;
mod streaming_router;
mod util;

use futures::stream::StreamExt;
use lsp_actor_service::LspActorService;
use serde_json::Value;
use std::{ops::ControlFlow, sync::Arc, time::Duration};
use tokio::sync::{Mutex, RwLock};

use actor::Actor;
use async_lsp::{
    can_handle::CanHandle,
    lsp_types::{
        notification::Initialized,
        request::{HoverRequest, Initialize, Request},
        Hover, InitializeParams, InitializeResult,
    },
    router::Router,
    steer::{self, FirstComeFirstServe, LspPicker, LspSteer},
    util::BoxLspService,
    AnyEvent, AnyNotification, AnyRequest, ClientSocket, LspService, ResponseError,
};
use backend::{db::Jar, Backend};
use functionality::{handlers, streams::setup_streams};
use lsp_actor::{ActOnNotification, ActOnRequest};
use lsp_streams::RouterStreams;
use tower::{layer::layer_fn, util::BoxService, Service, ServiceBuilder};
struct TickEvent;

impl<M> CanHandle<M> for LspActorService {
    fn can_handle(&self, msg: &M) -> bool {
        true
    }
}

#[tokio_macros::main]
async fn main() {
    let (server, _) = async_lsp::MainLoop::new_server(|client| {
        let mut backend = Backend::new(client.clone());
        let (mut actor, actor_ref) = Actor::new(backend);

        actor.register_request_handler(handlers::initialize);

        let actor_service = lsp_actor_service::LspActorService::new(actor_ref.clone());

        let mut streaming_router = Router::new(());
        let initialize_stream = streaming_router.request_stream::<Initialize>();
        let initialized_stream = streaming_router.notification_stream::<Initialized>();

        let services: Vec<BoxLspService<serde_json::Value, ResponseError>> = vec![
            BoxLspService::new(streaming_router),
            BoxLspService::new(actor_service),
        ];

        // let picker = FirstComeFirstServe::<BoxLspService<Value, ResponseError>>::default();
        let steering_router = LspSteer::new(services, FirstComeFirstServe);
        steering_router
    });

    #[cfg(unix)]
    let (stdin, stdout) = (
        async_lsp::stdio::PipeStdin::lock_tokio().unwrap(),
        async_lsp::stdio::PipeStdout::lock_tokio().unwrap(),
    );
    // Fallback to spawn blocking read/write otherwise.
    #[cfg(not(unix))]
    let (stdin, stdout) = (
        tokio_util::compat::TokioAsyncReadCompatExt::compat(tokio::io::stdin()),
        tokio_util::compat::TokioAsyncWriteCompatExt::compat_write(tokio::io::stdout()),
    );

    server.run_buffered(stdin, stdout).await.unwrap();
}
