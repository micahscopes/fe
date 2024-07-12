//! # async-lsp-streams
//!
//! This crate provides an extension to the `async-lsp` library, allowing easy creation of
//! stream-based handlers for LSP requests and notifications.

use async_lsp::router::Router;
use async_lsp::{lsp_types::*, ResponseError};
use futures::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::{mpsc, oneshot};

/// A stream of LSP request messages with their response channels.
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

/// A stream of LSP notification messages.
pub struct NotificationStream<N: notification::Notification> {
    receiver: mpsc::Receiver<N::Params>,
}

impl<N: notification::Notification> Stream for NotificationStream<N> {
    type Item = N::Params;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx)
    }
}

/// An extension trait for `RouterBuilder` to add stream-based handlers.
pub trait RouterStreams {
    /// Creates a stream for handling a specific LSP request.
    fn request_stream<R>(&mut self) -> RequestStream<R::Params, R::Result>
    where
        R: request::Request;

    /// Creates a stream for handling a specific LSP notification.
    fn notification_stream<N>(&mut self) -> NotificationStream<N>
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

    fn notification_stream<N>(&mut self) -> NotificationStream<N>
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

/// A helper function to spawn a task for handling a request stream.
pub fn spawn_request_handler<R, F, Fut>(mut stream: RequestStream<R::Params, R::Result>, f: F)
where
    R: request::Request,
    F: Fn(R::Params) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = std::result::Result<R::Result, ResponseError>>
        + Send
        + 'static,
{
    tokio::spawn(async move {
        while let Some((params, response_tx)) = stream.receiver.recv().await {
            let result = f(params).await;
            let _ = response_tx.send(result);
        }
    });
}

/// A helper function to spawn a task for handling a notification stream.
pub fn spawn_notification_handler<N, F, Fut>(mut stream: NotificationStream<N>, f: F)
where
    N: notification::Notification,
    F: Fn(<N as notification::Notification>::Params) -> Fut + Send + 'static, // Change here
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(async move {
        while let Some(params) = stream.receiver.recv().await {
            f(params).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_lsp::MainLoop;

    #[tokio::test]
    async fn test_request_stream() {
        use tower::ServiceBuilder;

        let (server, _) = MainLoop::new_server(|_client| {
            let mut router = Router::new(()); // Create a router
            let stream = router.request_stream::<request::Initialize>();

            let service = ServiceBuilder::new().service(router); // Add the router to the service
                                                                 // .concurrency(Concurrency::new(())); // Add concurrency to the service

            spawn_request_handler::<request::Initialize, _, _>(stream, |_params| async {
                Ok(InitializeResult {
                    capabilities: ServerCapabilities::default(),
                    server_info: None,
                })
            });

            service // Return the service
        });

        // You would typically set up a client and send requests here to test
        // For a complete test, you'd need to set up a mock client
    }
    #[tokio::test]
    async fn test_notification_stream() {
        use tower::ServiceBuilder;

        let (server, _) = MainLoop::new_server(|_client| {
            let mut router = Router::new(()); // Create a router
            let stream = router.notification_stream::<notification::DidOpenTextDocument>();

            let service = ServiceBuilder::new().service(router); // Add the router to the service

            spawn_notification_handler::<notification::DidOpenTextDocument, _, _>(
                stream,
                |_params| async {
                    // Handle the DidOpenTextDocument notification
                },
            );

            service // Return the service
        });

        // You would typically set up a client and send notifications here to test
        // For a complete test, you'd need to set up a mock client
    }
}
