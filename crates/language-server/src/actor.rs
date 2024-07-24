use async_lsp::can_handle::CanHandle;
use async_lsp::AnyRequest;
use futures::channel::mpsc;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use futures::StreamExt;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

type BoxedAny = Box<dyn Any + Send>;
type StateRef<S> = Rc<RefCell<S>>;
type RequestHandler<S> = Box<dyn Fn(StateRef<S>, BoxedAny) -> LocalBoxFuture<'static, BoxedAny>>;
type SyncMessageHandler<S> = Box<dyn Fn(StateRef<S>, BoxedAny) -> ()>;
type AsyncMessageHandler<S> = Box<dyn Fn(StateRef<S>, BoxedAny) -> LocalBoxFuture<'static, ()>>;

enum NotificationHandler<S> {
    Sync(SyncMessageHandler<S>),
    Async(AsyncMessageHandler<S>),
}

#[derive(Debug)]
pub enum ActorError {
    HandlerNotFound,
    SendError,
}

pub enum Message {
    Notification(BoxedAny),
    Request(BoxedAny, futures::channel::oneshot::Sender<BoxedAny>),
}

pub struct Actor<S: 'static> {
    state: StateRef<S>,
    receiver: mpsc::UnboundedReceiver<Message>,
    notification_handlers: HashMap<std::any::TypeId, NotificationHandler<S>>,
    request_handlers: HashMap<std::any::TypeId, RequestHandler<S>>,
    handler_types: Arc<HandlerTypes>,
}

pub struct ActorRef {
    sender: mpsc::UnboundedSender<Message>,
    handler_types: Arc<HandlerTypes>,
}

struct HandlerTypes {
    notification_handlers: std::sync::RwLock<HashMap<std::any::TypeId, ()>>,
    request_handlers: std::sync::RwLock<HashMap<std::any::TypeId, ()>>,
}

impl<S: 'static> Actor<S> {
    pub fn new(state: S) -> (Self, ActorRef) {
        let (sender, receiver) = mpsc::unbounded();
        let handler_types = Arc::new(HandlerTypes {
            notification_handlers: std::sync::RwLock::new(HashMap::new()),
            request_handlers: std::sync::RwLock::new(HashMap::new()),
        });

        (
            Self {
                state: Rc::new(RefCell::new(state)),
                receiver,
                notification_handlers: HashMap::new(),
                request_handlers: HashMap::new(),
                handler_types: handler_types.clone(),
            },
            ActorRef {
                sender,
                handler_types,
            },
        )
    }

    pub async fn run(&mut self) {
        while let Some(message) = self.receiver.next().await {
            match message {
                Message::Notification(params) => {
                    let type_id = (*params).type_id();
                    if let Some(handler) = self.notification_handlers.get(&type_id) {
                        match handler {
                            NotificationHandler::Sync(handler) => {
                                handler(self.state.clone(), params)
                            }
                            NotificationHandler::Async(handler) => {
                                handler(self.state.clone(), params).await
                            }
                        }
                    }
                }
                Message::Request(params, responder) => {
                    let type_id = (*params).type_id();
                    if let Some(handler) = self.request_handlers.get(&type_id) {
                        let result = handler(self.state.clone(), params).await;
                        let _ = responder.send(result);
                    }
                }
            }
        }
    }

    pub fn register_async_notification_handler<M: Send + 'static, F, Fut>(&mut self, handler: F)
    where
        F: Fn(StateRef<S>, M) -> Fut + 'static,
        Fut: std::future::Future<Output = ()> + 'static,
    {
        let type_id = std::any::TypeId::of::<M>();
        self.notification_handlers.insert(
            type_id,
            NotificationHandler::Async(Box::new(move |state, params| {
                let params = params.downcast::<M>().unwrap();
                handler(state, *params).boxed_local()
            })),
        );
        self.handler_types
            .notification_handlers
            .write()
            .unwrap()
            .insert(type_id, ());
    }

    pub fn register_notification_handler<M: Send + 'static, F>(&mut self, handler: F)
    where
        F: Fn(StateRef<S>, M) -> () + 'static,
    {
        let type_id = std::any::TypeId::of::<M>();
        self.notification_handlers.insert(
            type_id,
            NotificationHandler::Sync(Box::new(move |state, params| {
                let params = params.downcast::<M>().unwrap();
                handler(state, *params);
            })),
        );
        self.handler_types
            .notification_handlers
            .write()
            .unwrap()
            .insert(type_id, ());
    }

    pub fn register_request_handler<C: Send + 'static, R: Send + 'static, F, Fut>(
        &mut self,
        handler: F,
    ) where
        F: Fn(StateRef<S>, C) -> Fut + 'static,
        Fut: std::future::Future<Output = R> + 'static,
    {
        let type_id = std::any::TypeId::of::<C>();
        let handler = Rc::new(RefCell::new(handler));
        self.request_handlers.insert(
            type_id,
            Box::new(move |state, params| {
                let handler = handler.clone();
                let params = params.downcast::<C>().unwrap();
                async move {
                    let result = handler.borrow()(state, *params).await;
                    Box::new(result) as BoxedAny
                }
                .boxed_local()
            }),
        );
        self.handler_types
            .request_handlers
            .write()
            .unwrap()
            .insert(type_id, ());
    }
}

impl ActorRef {
    fn has_message_handler<M: 'static>(&self) -> bool {
        self.handler_types
            .notification_handlers
            .read()
            .unwrap()
            .contains_key(&std::any::TypeId::of::<M>())
    }

    fn has_request_handler<M: 'static>(&self) -> bool {
        self.handler_types
            .request_handlers
            .read()
            .unwrap()
            .contains_key(&std::any::TypeId::of::<M>())
    }

    pub async fn ask<M: Send + 'static, R: Send + 'static>(
        &self,
        message: M,
    ) -> Result<R, ActorError> {
        if !self.has_request_handler::<M>() {
            return Err(ActorError::HandlerNotFound);
        }

        let (responder, receiver) = futures::channel::oneshot::channel();
        let message = Message::Request(Box::new(message), responder);
        self.sender
            .unbounded_send(message)
            .map_err(|_| ActorError::SendError)?;

        receiver
            .await
            .map_err(|_| ActorError::SendError)
            .and_then(|result| Ok(*result.downcast().unwrap()))
    }

    pub fn tell<M: Send + 'static>(&self, message: M) -> Result<(), ActorError> {
        if !self.has_message_handler::<M>() {
            return Err(ActorError::HandlerNotFound);
        }

        let message = Message::Notification(Box::new(message));
        self.sender
            .unbounded_send(message)
            .map_err(|_| ActorError::SendError)
    }
}

impl Clone for ActorRef {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            handler_types: self.handler_types.clone(),
        }
    }
}
