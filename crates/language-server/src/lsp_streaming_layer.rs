use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use async_lsp::lsp_types::{notification, request};
use async_lsp::{AnyNotification, AnyRequest, LspService, ResponseError};
use futures::{Future, Stream, StreamExt};
use serde_json::Value as JsonValue;
use tokio::sync::{mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use tower::{Layer, Service};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

pub struct RequestStream<Params, Result> {
    receiver: Pin<
        Box<
            dyn Stream<
                    Item = (
                        Params,
                        oneshot::Sender<std::result::Result<Result, ResponseError>>,
                    ),
                > + Send,
        >,
    >,
}

impl<Params, Result> Stream for RequestStream<Params, Result> {
    type Item = (
        Params,
        oneshot::Sender<std::result::Result<Result, ResponseError>>,
    );

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.as_mut().poll_next(cx)
    }
}

pub struct NotificationStream<Params> {
    receiver: Pin<Box<dyn Stream<Item = Params> + Send>>,
}

impl<Params> Stream for NotificationStream<Params> {
    type Item = Params;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.as_mut().poll_next(cx)
    }
}

pub struct StreamingRouter<Inner> {
    inner: Inner,
    req_handlers: HashMap<
        &'static str,
        (
            Box<dyn Fn(AnyRequest) -> BoxFuture<Result<JsonValue, ResponseError>> + Send + Sync>,
            mpsc::Sender<(JsonValue, oneshot::Sender<Result<JsonValue, ResponseError>>)>,
        ),
    >,
    notif_handlers: HashMap<
        &'static str,
        (
            Box<dyn Fn(AnyNotification) -> BoxFuture<()> + Send + Sync>,
            mpsc::Sender<JsonValue>,
        ),
    >,
}

impl<Inner> StreamingRouter<Inner> {
    pub fn new(
        inner: Inner,
        req_streams: HashMap<
            &'static str,
            mpsc::Sender<(JsonValue, oneshot::Sender<Result<JsonValue, ResponseError>>)>,
        >,
        notif_streams: HashMap<&'static str, mpsc::Sender<JsonValue>>,
    ) -> Self {
        let req_handlers = req_streams
            .into_iter()
            .map(|(method, tx)| {
                let handler = Arc::new(move |req: AnyRequest| {
                    let tx = tx.clone();
                    let tx_return = tx.clone();
                    Box::pin(async move {
                        let (response_tx, response_rx) = oneshot::channel();
                        tx.send((req.params, response_tx)).await.map_err(|_| {
                            ResponseError::new(
                                async_lsp::ErrorCode::INTERNAL_ERROR,
                                "Failed to send request".to_string(),
                            )
                        })?;
                        response_rx.await.map_err(|_| {
                            ResponseError::new(
                                async_lsp::ErrorCode::INTERNAL_ERROR,
                                "Failed to receive response".to_string(),
                            )
                        })?
                    }) as BoxFuture<Result<JsonValue, ResponseError>>
                });
                (
                    method,
                    (
                        Box::new(move |req| handler(req))
                            as Box<
                                dyn Fn(AnyRequest) -> BoxFuture<Result<JsonValue, ResponseError>>
                                    + Send
                                    + Sync,
                            >,
                        tx_handler,
                    ),
                )
            })
            .collect();

        let notif_handlers = notif_streams
            .into_iter()
            .map(|(method, tx)| {
                let handler = Arc::new(move |notif: AnyNotification| {
                    let tx = tx.clone();
                    Box::pin(async move {
                        if let Err(e) = tx.send(notif.params).await {
                            eprintln!("Failed to send notification: {}", e);
                        }
                    }) as BoxFuture<()>
                });
                (
                    method,
                    (
                        Box::new(move |notif| handler(notif))
                            as Box<dyn Fn(AnyNotification) -> BoxFuture<()> + Send + Sync>,
                        tx,
                    ),
                )
            })
            .collect();

        Self {
            inner,
            req_handlers,
            notif_handlers,
        }
    }
}

impl<Inner> Service<AnyRequest> for StreamingRouter<Inner>
where
    Inner: Service<AnyRequest, Response = JsonValue, Error = ResponseError>,
    Inner::Future: Send + 'static,
{
    type Response = JsonValue;
    type Error = ResponseError;
    type Future = BoxFuture<Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: AnyRequest) -> Self::Future {
        if let Some((handler, _)) = self.req_handlers.get(&*req.method) {
            handler(req)
        } else {
            Box::pin(self.inner.call(req))
        }
    }
}

impl<Inner> LspService for StreamingRouter<Inner>
where
    Inner: LspService + Service<AnyRequest, Response = JsonValue, Error = ResponseError>,
    Inner::Future: Send + 'static,
{
    fn notify(&mut self, notif: AnyNotification) -> std::ops::ControlFlow<async_lsp::Result<()>> {
        if let Some((handler, _)) = self.notif_handlers.get(&*notif.method) {
            tokio::spawn(handler(notif));
            std::ops::ControlFlow::Continue(())
        } else {
            self.inner.notify(notif)
        }
    }

    fn emit(&mut self, event: async_lsp::AnyEvent) -> std::ops::ControlFlow<async_lsp::Result<()>> {
        self.inner.emit(event)
    }
}

pub struct StreamingLayer {
    req_streams: Arc<
        Mutex<
            HashMap<
                &'static str,
                mpsc::Sender<(JsonValue, oneshot::Sender<Result<JsonValue, ResponseError>>)>,
            >,
        >,
    >,
    notif_streams: Arc<Mutex<HashMap<&'static str, mpsc::Sender<JsonValue>>>>,
}

impl StreamingLayer {
    pub fn new() -> Self {
        Self {
            req_streams: Arc::new(Mutex::new(HashMap::new())),
            notif_streams: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn request<R: request::Request>(&self) -> RequestStream<R::Params, R::Result>
    where
        R::Params: serde::de::DeserializeOwned + Send + 'static,
        R::Result: serde::Serialize + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(100);
        self.req_streams.lock().unwrap().insert(R::METHOD, tx);

        RequestStream {
            receiver: Box::pin(ReceiverStream::new(rx).map(|(params, sender)| {
                let params = serde_json::from_value(params).unwrap();
                let sender =
                    oneshot::Sender::new(|res| res.map(|v| serde_json::from_value(v).unwrap()));
                (params, sender)
            })),
        }
    }

    pub fn notification<N: notification::Notification>(&self) -> NotificationStream<N::Params>
    where
        N::Params: serde::de::DeserializeOwned + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(100);
        self.notif_streams.lock().unwrap().insert(N::METHOD, tx);

        NotificationStream {
            receiver: Box::pin(
                ReceiverStream::new(rx).map(|params| serde_json::from_value(params).unwrap()),
            ),
        }
    }
}

impl<S> Layer<S> for StreamingLayer
where
    S: Service<AnyRequest, Response = JsonValue, Error = ResponseError> + LspService,
    S::Future: Send + 'static,
{
    type Service = StreamingRouter<S>;

    fn layer(&self, inner: S) -> Self::Service {
        StreamingRouter::new(
            inner,
            self.req_streams.lock().unwrap().clone(),
            self.notif_streams.lock().unwrap().clone(),
        )
    }
}
