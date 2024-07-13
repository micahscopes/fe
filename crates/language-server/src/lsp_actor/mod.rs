use async_lsp::lsp_types::notification::Notification;
use async_lsp::lsp_types::{notification, request};
use async_lsp::router::Router;
use async_lsp::ResponseError;

use crate::functionality::actor::{ActorRef, Message, Request};

impl<N: Notification> Message for N {
    type Contents = N::Params;
}

impl<R: request::Request> Request for R {
    type Contents = R::Params;
    type Reply = Result<R::Result, ResponseError>;
}

pub trait ActOnRequest {
    fn act_on_request<R>(&mut self, actor_ref: &ActorRef)
    where
        R: request::Request + 'static;
}

pub trait ActOnNotification {
    fn act_on_notification<N>(&mut self, actor_ref: &ActorRef)
    where
        N: notification::Notification;
}

impl<State> ActOnRequest for Router<State> {
    fn act_on_request<R>(&mut self, actor_ref: &ActorRef)
    where
        R: request::Request,
    {
        let actor_ref = actor_ref.clone();
        self.request::<R, _>(move |_, params| {
            let actor_ref = actor_ref.clone();
            async move { actor_ref.ask::<R>(params).await }
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
            actor_ref.tell::<N>(params);
            std::ops::ControlFlow::Continue(())
        });
    }
}
