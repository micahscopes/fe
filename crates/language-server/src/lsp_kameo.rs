use async_lsp::router::Router;
use async_lsp::lsp_types::{notification, request};
use async_lsp::ResponseError;
use kameo::actor::{ActorRef, Mailbox, BoundedMailbox, UnboundedMailbox};
use kameo::message::Message;
use kameo::request::AskRequest;
use kameo::{Actor, Reply};
use std::fmt::Display;

pub trait AskRequestSend<A: Actor, T, M: Message<T>> {
    fn send(self) -> impl std::future::Future<Output = Result<<A as Message<M>>::Reply, kameo::error::SendError<M, <A as Message<M>>::Reply>>> + Send;
}

impl<A, M> AskRequestSend<A, M> for AskRequest<A, BoundedMailbox<A>, M>
where
    A: Actor<Mailbox = BoundedMailbox<A>> + Message<M>,
    M: Message,
{
    fn send(self) -> impl std::future::Future<Output = Result<<A as Message<M>>::Reply, kameo::error::SendError<M, <A as Message<M>>::Reply>>> + Send {
        self.send()
    }
}

impl<A, M> AskRequestSend<A, M> for AskRequest<A, UnboundedMailbox<A>, M>
where
    A: Actor<Mailbox = UnboundedMailbox<A>> + Message<M>,
    M: Message,
{
    fn send(self) -> impl std::future::Future<Output = Result<<A as Message<M>>::Reply, kameo::error::SendError<M, <A as Message<M>>::Reply>>> + Send {
        self.send()
    }
}

pub trait RouterActors<A: Actor> 
where
    A::Mailbox: Mailbox<A>,
{
    fn request_to_actor<R>(&mut self, actor: ActorRef<A>)
    where
        R: request::Request,
        A: Message<R::Params>,
        AskRequest<A, A::Mailbox, R::Params>: AskRequestSend<A, R::Params>,
        <<A as Message<R::Params>>::Reply as Reply>::Ok: Into<R::Result>,
        <<A as Message<R::Params>>::Reply as Reply>::Error: Display;

    fn notification_to_actor<N>(&mut self, actor: ActorRef<A>)
    where
        N: notification::Notification,
        A: Message<N::Params>;
}

impl<A: Actor> RouterActors<A> for Router<ActorRef<A>>
where
    A::Mailbox: Mailbox<A>,
{
    fn request_to_actor<R>(&mut self, actor: ActorRef<A>)
    where
        R: request::Request,
        A: Message<R::Params>,
        AskRequest<A, A::Mailbox, R::Params>: AskRequestSend<A, R::Params>,
        <<A as Message<R::Params>>::Reply as Reply>::Ok: Into<R::Result>,
        <<A as Message<R::Params>>::Reply as Reply>::Error: Display,
    {
        self.request::<R, _>(move |_, params| {
            let actor = actor.clone();
            async move {
                AskRequestSend::send(actor.ask(params)).await
                    .map_err(|e| ResponseError::new(async_lsp::ErrorCode::INTERNAL_ERROR, e.to_string()))
                    .map(|reply| reply.into())
            }
        });
    }

    fn notification_to_actor<N>(&mut self, actor: ActorRef<A>)
    where
        N: notification::Notification,
        A: Message<N::Params>,
    {
        self.notification::<N>(move |_, params| {
            let actor = actor.clone();
            actor.tell(params);
            std::ops::ControlFlow::Continue(())
        });
    }
}

// Example usage
// pub fn setup_lsp_server<A>(router: &mut Router<ActorRef<A>>, backend: ActorRef<A>)
// where
//     A: Actor + Message<lsp_types::InitializeParams> + Message<lsp_types::DidOpenTextDocumentParams>,
//     A::Mailbox: Mailbox<A>,
//     AskRequest<A, A::Mailbox, lsp_types::InitializeParams>: AskRequestSend<A, lsp_types::InitializeParams>,
//     <<A as Message<lsp_types::InitializeParams>>::Reply as Reply>::Ok: Into<lsp_types::InitializeResult>,
//     <<A as Message<lsp_types::InitializeParams>>::Reply as Reply>::Error: Display,
// {
//     router.request_to_actor::<request::Initialize, _>(backend.clone());
//     router.notification_to_actor::<notification::DidOpenTextDocument, _>(backend.clone());
    
//     // Add more request and notification handlers as needed
// }
