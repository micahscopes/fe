use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

use futures::channel::mpsc;
use futures::future::LocalBoxFuture;
use futures::FutureExt;
use futures::StreamExt;

#[derive(Debug)]
pub enum ActorError {
    HandlerNotFound,
    StateAccessError,
    ExecutionError(Box<dyn std::error::Error + Send + Sync>),
    CustomError(Box<dyn std::error::Error + Send + Sync>),
}

impl std::fmt::Display for ActorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActorError::HandlerNotFound => write!(f, "Handler not found"),
            ActorError::StateAccessError => write!(f, "Failed to access actor state"),
            ActorError::ExecutionError(e) => write!(f, "Execution error: {}", e),
            ActorError::CustomError(e) => write!(f, "Custom error: {}", e),
        }
    }
}

impl std::error::Error for ActorError {}

type BoxedAny = Box<dyn Any + Send>;
type StateRef<S> = Rc<RefCell<S>>;
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait AsyncFunc<'a, S, C, R, E>: Fn(&'a mut S, C) -> Self::Fut + Send + Sync
where
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    type Fut: Future<Output = Result<R, E>> + Send;
}

impl<'a, F, S, C, R, E, Fut> AsyncFunc<'a, S, C, R, E> for F
where
    F: Fn(&'a mut S, C) -> Fut + Send + Sync,
    S: 'static,
    C: 'static,
    R: 'static,
    E: std::error::Error + Send + Sync + 'static,
    Fut: Future<Output = Result<R, E>> + Send,
{
    type Fut = Fut;
}

type BoxAsyncFunc<S, C, R> =
    Box<dyn for<'a> Fn(&'a mut S, C) -> BoxFuture<'a, Result<R, ActorError>> + Send + Sync>;

struct AsyncFuncHandler<S, C, R>(BoxAsyncFunc<S, C, R>);

impl<S: 'static, C: 'static, R: 'static> AsyncFuncHandler<S, C, R> {
    fn new<F, E>(f: F) -> Self
    where
        F: for<'a> AsyncFunc<'a, S, C, R, E> + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        AsyncFuncHandler(Box::new(move |s, c| {
            Box::pin(f(s, c).map(|r| r.map_err(|e| ActorError::CustomError(Box::new(e)))))
        }))
    }
}

pub struct Actor<S: 'static> {
    state: StateRef<S>,
    handlers: HashMap<std::any::TypeId, Box<dyn Any + Send + Sync>>,
}

impl<S: 'static> Actor<S> {
    pub fn new(state: S) -> Self {
        Self {
            state: Rc::new(RefCell::new(state)),
            handlers: HashMap::new(),
        }
    }

    pub fn register_request_handler<C, R, E, F>(&mut self, handler: F)
    where
        C: 'static + Send,
        R: 'static + Send,
        E: std::error::Error + Send + Sync + 'static,
        F: for<'a> AsyncFunc<'a, S, C, R, E> + 'static,
    {
        let type_id = std::any::TypeId::of::<C>();
        let handler = AsyncFuncHandler::new(handler);
        self.handlers.insert(type_id, Box::new(handler));
    }

    pub async fn handle<C, R>(&self, params: C) -> Result<R, ActorError>
    where
        C: 'static,
        R: 'static,
    {
        let type_id = std::any::TypeId::of::<C>();
        if let Some(handler) = self.handlers.get(&type_id) {
            let handler = handler.downcast_ref::<AsyncFuncHandler<S, C, R>>().unwrap();
            let mut state = self.state.borrow_mut();
            handler.0(&mut *state, params).await
        } else {
            Err(ActorError::HandlerNotFound)
        }
    }
}
