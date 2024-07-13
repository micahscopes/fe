use futures::channel::mpsc;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use futures::StreamExt;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub trait MessageHandler<M: Send>: 'static {
    async fn handle(&mut self, message: M);
}

pub trait RequestHandler<M: Send, R: Send>: 'static {
    async fn handle(&mut self, message: M) -> R;
}

enum ActorMessage {
    Notification(Box<dyn Any + Send>),
    Request(
        Box<dyn Any + Send>,
        futures::channel::oneshot::Sender<Box<dyn Any + Send>>,
    ),
}

pub struct Actor<S: 'static> {
    state: Rc<RefCell<S>>,
    receiver: mpsc::UnboundedReceiver<ActorMessage>,
    message_handlers: HashMap<
        std::any::TypeId,
        Rc<dyn Fn(Rc<RefCell<S>>, Box<dyn Any + Send>) -> LocalBoxFuture<'static, ()>>,
    >,
    request_handlers: HashMap<
        std::any::TypeId,
        Rc<
            dyn Fn(
                Rc<RefCell<S>>,
                Box<dyn Any + Send>,
            ) -> LocalBoxFuture<'static, Box<dyn Any + Send>>,
        >,
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
                state: Rc::new(RefCell::new(state)),
                receiver,
                message_handlers: HashMap::new(),
                request_handlers: HashMap::new(),
            },
            ActorRef { sender },
        )
    }

    pub async fn run(&mut self) {
        while let Some(message) = self.receiver.next().await {
            match message {
                ActorMessage::Notification(params) => {
                    let type_id = (*params).type_id();
                    if let Some(handler) = self.message_handlers.get(&type_id) {
                        handler(self.state.clone(), params).await;
                    }
                }
                ActorMessage::Request(params, responder) => {
                    let type_id = (*params).type_id();
                    if let Some(handler) = self.request_handlers.get(&type_id) {
                        let result = handler(self.state.clone(), params).await;
                        let _ = responder.send(result);
                    }
                }
            }
        }
    }

    pub fn register_message_handler<M: Send + 'static>(&mut self)
    where
        S: MessageHandler<M>,
    {
        self.message_handlers.insert(
            std::any::TypeId::of::<M>(),
            Rc::new(move |state: Rc<RefCell<S>>, params: Box<dyn Any + Send>| {
                let params = params.downcast::<M>().unwrap();
                async move {
                    state.borrow_mut().handle(*params).await;
                }
                .boxed_local()
            }),
        );
    }

    pub fn register_request_handler<C: Send + 'static, R: Send + 'static>(&mut self)
    where
        S: RequestHandler<C, R>,
    {
        self.request_handlers.insert(
            std::any::TypeId::of::<C>(),
            Rc::new(move |state: Rc<RefCell<S>>, params: Box<dyn Any + Send>| {
                let params = params.downcast::<C>().unwrap();
                async move {
                    let result = state.borrow_mut().handle(*params).await;
                    Box::new(result) as Box<dyn Any + Send>
                }
                .boxed_local()
            }),
        );
    }
}

impl ActorRef {
    pub async fn ask<Message: Send + 'static, Reply: Send + 'static>(
        &self,
        message: Message,
    ) -> Reply {
        let (responder, receiver) = futures::channel::oneshot::channel();
        let message = ActorMessage::Request(Box::new(message), responder);
        self.sender.unbounded_send(message).unwrap();
        let result = receiver.await.unwrap();
        *result.downcast().unwrap()
    }

    pub fn tell<Message: Send + 'static>(&self, message: Message) {
        let message = ActorMessage::Notification(Box::new(message));
        self.sender.unbounded_send(message).unwrap();
    }
}

impl Clone for ActorRef {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
        }
    }
}
