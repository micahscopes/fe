use async_lsp::lsp_types::notification::Notification;
use async_lsp::lsp_types::request::Request;
use async_lsp::lsp_types::{notification, request};
use async_lsp::router::Router;
use async_lsp::ResponseError;

use crate::functionality::actor::{Actor, ActorRef, MessageHandler, RequestHandler};

// pub trait LspNotificationHandler<N: Notification>: 'static {
//     async fn handle(&self, params: N::Params);
// }

// pub trait LspRequestHandler<R: Request> {
//     async fn handle(&self, params: R::Params) -> Result<R::Result, ResponseError>;
// }

// impl<R, T> RequestHandler<R::Params, Result<R::Result, ResponseError>> for T
// where
//     R: Request,
//     T: LspRequestHandler<R>,
// {
//     async fn handle(&mut self, message: R::Params) -> Result<R::Result, ResponseError> {
//         LspRequestHandler::<R>::handle(self, message).await
//     }
// }

// type Params<R: Request> = R::Params;
pub type LspResult<R: Request> = Result<R::Result, ResponseError>;

impl<S> Actor<S> {
    pub fn register_lsp_notification_handler<N: Notification>(&mut self)
    where
        S: MessageHandler<N::Params>,
    {
        self.register_message_handler::<N::Params>();
    }
    pub fn register_lsp_request_handler<R: Request>(&mut self)
    where
        S: RequestHandler<R::Params, LspResult<R>>,
    {
        self.register_request_handler::<R::Params, LspResult<R>>();
    }
}

pub trait ActOnRequest {
    fn act_on_request<R>(&mut self, actor_ref: &ActorRef)
    where
        R: request::Request + Send + 'static;
}

pub trait ActOnNotification {
    fn act_on_notification<N>(&mut self, actor_ref: &ActorRef)
    where
        N: notification::Notification;
}

impl<State> ActOnRequest for Router<State> {
    fn act_on_request<R>(&mut self, actor_ref: &ActorRef)
    where
        R: request::Request + Send,
    {
        let actor_ref = actor_ref.clone();
        self.request::<R, _>(move |_, params| {
            let actor_ref = actor_ref.clone();
            async move { actor_ref.ask::<R::Params, LspResult<R>>(params).await }
        });
    }
}

impl<State> ActOnNotification for Router<State> {
    fn act_on_notification<N>(&mut self, actor_ref: &ActorRef)
    where
        N: notification::Notification,
    {
        let actor_ref = actor_ref.clone();
        self.notification::<N>(move |_, params| {
            let actor_ref = actor_ref.clone();
            actor_ref.tell::<N::Params>(params);
            std::ops::ControlFlow::Continue(())
        });
    }
}
