use futures::Future;
use std::any::Any;
use tokio::sync::{mpsc, oneshot};

pub trait Message<T> {
    type Reply: Send + 'static;
    fn handle(&mut self, message: T) -> Self::Reply;
}

pub(crate) trait DynMessage<A> {
    fn handle_dyn(self: Box<Self>, state: &mut A) -> Box<dyn Any + Send + 'static>;
}

impl<A, T> DynMessage<A> for T
where
    A: Actor + Message<T>,
    T: Send + 'static,
    A::Reply: Send + 'static,
{
    fn handle_dyn(self: Box<Self>, state: &mut A) -> Box<dyn Any + Send + 'static> {
        Box::new(state.handle(*self))
    }
}

// Actor trait
pub trait Actor: Sized + Send + 'static {
    fn start(&mut self) -> (ActorRef<Self>, impl Future<Output = ()> + Send + 'static) {
        let (tx, mut rx) = mpsc::channel::<(
            Box<dyn DynMessage<Self>>,
            oneshot::Sender<Box<dyn Any + Send + 'static>>,
        )>(100);

        let tx_clone = tx.clone();

        let fut = async move {
            while let Some((msg, reply_tx)) = rx.recv().await {
                let reply = msg.handle_dyn(self);
                let _ = reply_tx.send(reply);
            }
        };

        (ActorRef { sender: tx_clone }, fut)
    }
}

pub struct ActorRef<A> {
    sender: mpsc::Sender<(
        Box<dyn DynMessage<A>>,
        oneshot::Sender<Box<dyn Any + Send + 'static>>,
    )>,
}

impl<A: Actor> ActorRef<A> {
    pub async fn send<M>(&self, msg: M) -> Result<M::Reply, Box<dyn std::error::Error>>
    where
        M: Message<A> + Send + 'static,
        A: Message<M>,
        M::Reply: Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send((Box::new(msg), reply_tx))
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        let reply = reply_rx
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(*reply.downcast().unwrap())
    }
}
