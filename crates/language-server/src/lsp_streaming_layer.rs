use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_lsp::lsp_types::{notification, request};
use async_lsp::{AnyNotification, AnyRequest, LspService, ResponseError};
use futures::{Future, Stream};
use serde_json::Value as JsonValue;
use tokio::sync::{mpsc, oneshot};
use tower::{Layer, Service};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

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

pub struct StreamingRouter<Inner> {
    inner: Inner,
    req_handlers: HashMap<
        &'static str,
        Box<dyn Fn(AnyRequest) -> BoxFuture<Result<JsonValue, ResponseError>> + Send + Sync>,
    >,
    notif_handlers:
        HashMap<&'static str, Box<dyn Fn(AnyNotification) -> BoxFuture<()> + Send + Sync>>,
}

impl<Inner> StreamingRouter<Inner> {
    pub fn new(inner: Inner) -> Self {
        Self {
            inner,
            req_handlers: HashMap::new(),
            notif_handlers: HashMap::new(),
        }
    }

    pub fn request<R: request::Request>(&mut self) -> RequestStream<R::Params, R::Result>
    where
        R::Params: serde::de::DeserializeOwned + Send + 'static,
        R::Result: serde::Serialize + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(100);
        let handler = Arc::new(move |req: AnyRequest| {
            let tx = tx.clone();
            Box::pin(async move {
                let params: R::Params = serde_json::from_value(req.params).map_err(|e| {
                    ResponseError::new(async_lsp::ErrorCode::PARSE_ERROR, e.to_string())
                })?;
                let (response_tx, response_rx) = oneshot::channel();
                tx.send((params, response_tx)).await.map_err(|_| {
                    ResponseError::new(
                        async_lsp::ErrorCode::INTERNAL_ERROR,
                        "Failed to send request".to_string(),
                    )
                })?;
                let result = response_rx.await.map_err(|_| {
                    ResponseError::new(
                        async_lsp::ErrorCode::INTERNAL_ERROR,
                        "Failed to receive response".to_string(),
                    )
                })??;
                Ok(serde_json::to_value(result).map_err(|e| {
                    ResponseError::new(async_lsp::ErrorCode::INTERNAL_ERROR, e.to_string())
                })?)
            }) as BoxFuture<Result<JsonValue, ResponseError>>
        });
        self.req_handlers
            .insert(R::METHOD, Box::new(move |req| handler(req)));
        RequestStream { receiver: rx }
    }

    pub fn notification<N: notification::Notification>(&mut self) -> NotificationStream<N::Params>
    where
        N::Params: serde::de::DeserializeOwned + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(100);
        let handler = Arc::new(move |notif: AnyNotification| {
            let tx = tx.clone();
            Box::pin(async move {
                let params: N::Params = serde_json::from_value(notif.params)
                    .map_err(|e| {
                        eprintln!("Failed to parse notification params: {}", e);
                        return;
                    })
                    .unwrap_or_else(|_| panic!("Failed to create default params"));
                if let Err(e) = tx.send(params).await {
                    eprintln!("Failed to send notification: {}", e);
                }
            }) as BoxFuture<()>
        });
        self.notif_handlers
            .insert(N::METHOD, Box::new(move |notif| handler(notif)));
        NotificationStream { receiver: rx }
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
        if let Some(handler) = self.req_handlers.get(&*req.method) {
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
        if let Some(handler) = self.notif_handlers.get(&*notif.method) {
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

pub struct StreamingLayer;

impl StreamingLayer {
    pub fn new() -> Self {
        Self
    }

    pub fn request<R: request::Request>(&self) -> RequestStream<R::Params, R::Result>
    where
        R::Params: serde::de::DeserializeOwned + Send + 'static,
        R::Result: serde::Serialize + Send + 'static,
    {
        let (_, rx) = mpsc::channel(100);
        RequestStream { receiver: rx }
    }

    pub fn notification<N: notification::Notification>(&self) -> NotificationStream<N::Params>
    where
        N::Params: serde::de::DeserializeOwned + Send + 'static,
    {
        let (_, rx) = mpsc::channel(100);
        NotificationStream { receiver: rx }
    }
}

impl<S> Layer<S> for StreamingLayer
where
    S: Service<AnyRequest, Response = JsonValue, Error = ResponseError> + LspService,
    S::Future: Send + 'static,
{
    type Service = StreamingRouter<S>;

    fn layer(&self, inner: S) -> Self::Service {
        StreamingRouter::new(inner)
    }
}
