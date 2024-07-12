use futures::Future;
use std::any::Any;
use std::pin::Pin;
use tokio::sync::{mpsc, oneshot};

pub trait Message: Send + Sync + 'static {
    type Reply: Send + 'static;
}

pub trait Handler<M: Message> {
    fn handle(&mut self, message: M) -> Pin<Box<dyn Future<Output = M::Reply> + Send + '_>>;
}

pub(crate) trait DynMessage<A>: Send + 'static {
    async fn handle_dyn(self: Box<Self>, state: &mut A) -> Box<dyn Any + Send>;
}

impl<A, M> DynMessage<A> for M
where
    A: Actor + Handler<M>,
    M: Message,
{
    fn handle_dyn(
        self: Box<Self>,
        state: &mut A,
    ) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send + '_>> {
        Box::pin(async move {
            let reply = state.handle(*self).await;
            Box::new(reply) as Box<dyn Any + Send>
        })
    }
}

pub trait Actor: Sized + Send + 'static {
    fn start<'a>(&'a mut self) -> (ActorRef<Self>, impl Future<Output = ()> + Send + 'a) {
        let (tx, mut rx) = mpsc::channel::<(
            Box<dyn DynMessage<Self>>,
            oneshot::Sender<Box<dyn Any + Send>>,
        )>(100);

        let tx_clone = tx.clone();

        let fut = async move {
            while let Some((msg, reply_tx)) = rx.recv().await {
                let reply = msg.handle_dyn(self).await;
                let _ = reply_tx.send(reply);
            }
        };

        (ActorRef { sender: tx_clone }, fut)
    }
}

pub struct ActorRef<A> {
    sender: mpsc::Sender<(Box<dyn DynMessage<A>>, oneshot::Sender<Box<dyn Any + Send>>)>,
}

impl<A: Actor> ActorRef<A> {
    pub async fn send<M>(&self, msg: M) -> Result<M::Reply, Box<dyn std::error::Error>>
    where
        M: Message,
        A: Handler<M>,
    {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.sender
            .send((Box::new(msg) as Box<dyn DynMessage<A>>, reply_tx))
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        let reply = reply_rx
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;
        Ok(*reply.downcast().unwrap())
    }
}
