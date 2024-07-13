use async_trait::async_trait;
use futures::channel::mpsc;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use futures::StreamExt;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub trait Message: 'static {
    type Reply: 'static;
}

#[async_trait(?Send)]
pub trait Handler<M: Message>: 'static {
    async fn handle(&mut self, message: M) -> M::Reply;
}

struct ActorMessage {
    message: Box<dyn Any>,
    responder: futures::channel::oneshot::Sender<Box<dyn Any>>,
}

pub struct Actor<S: 'static> {
    state: S,
    receiver: mpsc::UnboundedReceiver<ActorMessage>,
    handlers: HashMap<
        std::any::TypeId,
        Rc<RefCell<dyn Fn(&mut S, Box<dyn Any>) -> LocalBoxFuture<'static, Box<dyn Any>>>>,
    >,
}

pub struct ActorRef {
    sender: mpsc::UnboundedSender<ActorMessage>,
}

impl<S: 'static> Actor<S> {
    pub fn new(state: S) -> (Self, ActorRef) {
        let (sender, receiver) = mpsc::unbounded();
        (
            Self {
                state,
                receiver,
                handlers: HashMap::new(),
            },
            ActorRef { sender },
        )
    }

    pub async fn run(&mut self) {
        while let Some(ActorMessage { message, responder }) = self.receiver.next().await {
            let reply = self.handle_any(message).await;
            let _ = responder.send(reply);
        }
    }

    async fn handle_any(&mut self, message: Box<dyn Any>) -> Box<dyn Any> {
        let type_id = (*message).type_id();
        if let Some(handler) = self.handlers.get(&type_id) {
            handler.borrow()(&mut self.state, message).await
        } else {
            Box::new(())
        }
    }

    pub fn register<M, H>(&mut self, handler: H)
    where
        M: Message + 'static,
        H: Handler<M> + 'static,
    {
        let handler = Rc::new(RefCell::new(handler));
        self.handlers.insert(
            std::any::TypeId::of::<M>(),
            Rc::new(RefCell::new(move |state: &mut S, message: Box<dyn Any>| {
                let handler = handler.clone();
                async move {
                    if let Ok(message) = message.downcast::<M>() {
                        let reply = handler.borrow_mut().handle(*message).await;
                        Box::new(reply) as Box<dyn Any>
                    } else {
                        Box::new(())
                    }
                }
                .boxed_local()
            })),
        );
    }
}

impl ActorRef {
    pub async fn send<M: Message + 'static>(&self, message: M) -> M::Reply
    where
        M::Reply: 'static,
    {
        let (responder, receiver) = futures::channel::oneshot::channel();
        let actor_message = ActorMessage {
            message: Box::new(message),
            responder,
        };
        self.sender.unbounded_send(actor_message).unwrap();
        let reply = receiver.await.unwrap();
        *reply.downcast().unwrap()
    }
}
