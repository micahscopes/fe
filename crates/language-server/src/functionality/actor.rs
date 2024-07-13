use futures::channel::mpsc;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use futures::StreamExt;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub trait Message {
    type Contents: 'static + Send;
}

pub trait Request {
    type Contents: 'static + Send;
    type Reply: 'static + Send;
}

pub trait MessageReceiver<N: Message>: 'static {
    async fn handle(&mut self, params: N::Contents);
}

pub trait RequestHandler<R: Request>: 'static {
    async fn handle(&mut self, params: R::Contents) -> R::Reply;
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
    notification_handlers: HashMap<
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
                notification_handlers: HashMap::new(),
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
                    if let Some(handler) = self.notification_handlers.get(&type_id) {
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

    pub fn register_message_receiver<M: Message>(&mut self)
    where
        S: MessageReceiver<M>,
    {
        self.notification_handlers.insert(
            std::any::TypeId::of::<M::Contents>(),
            Rc::new(move |state: Rc<RefCell<S>>, params: Box<dyn Any + Send>| {
                let params = params.downcast::<M::Contents>().unwrap();
                async move {
                    state.borrow_mut().handle(*params).await;
                }
                .boxed_local()
            }),
        );
    }

    pub fn register_request_handler<R: Request>(&mut self)
    where
        S: RequestHandler<R>,
    {
        self.request_handlers.insert(
            std::any::TypeId::of::<R::Contents>(),
            Rc::new(move |state: Rc<RefCell<S>>, params: Box<dyn Any + Send>| {
                let params = params.downcast::<R::Contents>().unwrap();
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
    pub async fn ask<R: Request>(&self, params: R::Contents) -> R::Reply {
        let (responder, receiver) = futures::channel::oneshot::channel();
        let message = ActorMessage::Request(Box::new(params), responder);
        self.sender.unbounded_send(message).unwrap();
        let result = receiver.await.unwrap();
        *result.downcast().unwrap()
    }

    pub fn tell<N: Message>(&self, params: N::Contents) {
        let message = ActorMessage::Notification(Box::new(params));
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
