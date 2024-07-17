use async_lsp::router::{BoxReqFuture, Router};
use async_lsp::{
    lsp_types::{notification, request},
    AnyNotification, AnyRequest, LspService, ResponseError,
};
use futures::{Future, Stream};
use serde_json::Value as JsonValue;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::{mpsc, oneshot};
use tower::{Layer, Service};

pub struct RequestStream<Params, Result> {
    receiver: mpsc::Receiver<(
        Params,
        oneshot::Sender<std::result::Result<Result, ResponseError>>,
    )>,
}

impl<Params, Result> Stream for RequestStream<Params, Result> {
    type Item = (
        Params,
        oneshot::Sender<std::result::Result<Result, ResponseError>>,
    );

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

pub struct NotificationStream<Params> {
    receiver: mpsc::Receiver<Params>,
}

impl<Params> Stream for NotificationStream<Params> {
    type Item = Params;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

pub trait RouterStreams {
    fn request_stream<R>(&mut self) -> RequestStream<R::Params, R::Result>
    where
        R: request::Request;

    fn notification_stream<N>(&mut self) -> NotificationStream<N::Params>
    where
        N: notification::Notification;
}

impl<State> RouterStreams for Router<State> {
    fn request_stream<R>(&mut self) -> RequestStream<R::Params, R::Result>
    where
        R: request::Request,
    {
        let (tx, rx) = mpsc::channel(100);
        self.request::<R, _>(move |_, params| {
            let tx = tx.clone();
            async move {
                let (response_tx, response_rx) = oneshot::channel();
                tx.send((params, response_tx)).await.unwrap();
                response_rx.await.unwrap()
            }
        });
        RequestStream { receiver: rx }
    }

    fn notification_stream<N>(&mut self) -> NotificationStream<N::Params>
    where
        N: notification::Notification,
    {
        let (tx, rx) = mpsc::channel(100);
        self.notification::<N>(move |_, params| {
            let tx = tx.clone();
            tokio::spawn(async move {
                tx.send(params).await.unwrap();
            });
            std::ops::ControlFlow::Continue(())
        });
        NotificationStream { receiver: rx }
    }
}

pub struct StreamingLayer;

impl StreamingLayer {
    pub fn new() -> Self {
        Self
    }
}
// use async_lsp::router::BoxReqFuture;

impl<S> Layer<S> for StreamingLayer
where
    S: LspService + Clone + Send + 'static,
    S: Service<
        AnyRequest,
        Response = JsonValue,
        Error = ResponseError,
        Future = BoxReqFuture<ResponseError>,
    >,
    // S::Future: BoxReqFuture<ResponseError> + Send + 'static,
{
    type Service = Router<(), ResponseError, S>;

    fn layer(&self, inner: S) -> Self::Service {
        Router::with_fallback((), inner)
    }
}
