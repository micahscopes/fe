use std::future::Future;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use act_locally::actor::ActorRef;
use act_locally::types::ActorError;
use futures::TryFutureExt;
use serde_json::Value;
use tracing::info;

use async_lsp::can_handle::CanHandle;
use async_lsp::{AnyEvent, AnyNotification, AnyRequest, Error, LspService, ResponseError};
use tower::Service;

use crate::lsp_actor::LspDispatcher;

pub struct LspActorService<S> {
    actor_ref: ActorRef<S>,
    dispatcher: Arc<LspDispatcher>,
}

impl<S> LspActorService<S> {
    pub fn new(actor_ref: ActorRef<S>, dispatcher: LspDispatcher) -> Self {
        Self {
            actor_ref,
            dispatcher: Arc::new(dispatcher),
        }
    }
}

type BoxReqFuture<Error> = Pin<Box<dyn Future<Output = Result<Value, Error>> + Send>>;
impl<S: 'static> Service<AnyRequest> for LspActorService<S> {
    type Response = serde_json::Value;
    type Error = ResponseError;
    // type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
    type Future = BoxReqFuture<Self::Error>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: AnyRequest) -> Self::Future {
        let method = req.method.clone();
        info!("got LSP request: {method:?}");
        let actor_ref = self.actor_ref.clone();
        let dispatcher = self.dispatcher.clone();
        let method_log = method.clone().to_owned();
        let result = Box::pin(async move {
            let dispatcher = dispatcher.as_ref();
            let ask = actor_ref.ask::<_, Self::Response, _>(dispatcher, req);
            let lsp_result: Result<Self::Response, _> = ask.await.map_err(|e| match e {
                ActorError::HandlerNotFound => ResponseError::new(
                    async_lsp::ErrorCode::METHOD_NOT_FOUND,
                    "Method not found".to_string(),
                ),
                _ => ResponseError::new(
                    async_lsp::ErrorCode::INTERNAL_ERROR,
                    format!("There was an internal error... {:?}", e),
                ),
            });
            info!("Prepared LSP response for: {method_log:?}");
            lsp_result
        });
        info!("Prepared future for LSP request: {method:?}");
        result
    }
}

impl<S: 'static> LspService for LspActorService<S> {
    fn notify(&mut self, notif: AnyNotification) -> ControlFlow<async_lsp::Result<()>> {
        let method = notif.method.clone();
        let dispatcher = self.dispatcher.clone();
        match self.actor_ref.tell(dispatcher.as_ref(), notif) {
            Ok(()) => ControlFlow::Continue(()),
            Err(ActorError::HandlerNotFound) => {
                tracing::warn!("Method not found for notification `{}`", method);
                ControlFlow::Continue(())
            }
            Err(e) => ControlFlow::Break(Err(Error::Response(ResponseError::new(
                async_lsp::ErrorCode::INTERNAL_ERROR,
                format!(
                    "Failed to send notification: {:?} for notification `{}`",
                    e, method
                ),
            )))),
        }
    }

    fn emit(&mut self, event: AnyEvent) -> ControlFlow<async_lsp::Result<()>> {
        let type_name = event.type_name();
        let dispatcher = self.dispatcher.clone();
        match self.actor_ref.tell(dispatcher.as_ref(), event) {
            Ok(()) => ControlFlow::Continue(()),
            Err(ActorError::HandlerNotFound) => {
                tracing::warn!("Method not found for event: {:?}", type_name);
                ControlFlow::Continue(())
            }
            Err(e) => ControlFlow::Break(Err(Error::Response(ResponseError::new(
                async_lsp::ErrorCode::INTERNAL_ERROR,
                format!("Failed to emit event: {:?}", e),
            )))),
        }
    }
}

impl<S> CanHandle<AnyRequest> for LspActorService<S> {
    fn can_handle(&self, req: &AnyRequest) -> bool {
        self.dispatcher.wrappers.contains_key(&req.method)
    }
}

impl<S> CanHandle<AnyNotification> for LspActorService<S> {
    fn can_handle(&self, notif: &AnyNotification) -> bool {
        self.dispatcher.wrappers.contains_key(&notif.method)
    }
}

impl<S> CanHandle<AnyEvent> for LspActorService<S> {
    fn can_handle(&self, _: &AnyEvent) -> bool {
        false
    }
}
