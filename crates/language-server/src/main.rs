mod backend;
mod capabilities;
mod db;
mod diagnostics;
mod globals;
mod goto;
mod language_server;
mod logger;
mod util;
mod workspace;

use backend::Backend;
use db::Jar;
mod handlers {
    pub mod notifications;
    pub mod request;
}


// #[cfg(feature = "runtime-agnostic")]
// use tokio::stdio::{stdin, stdout};

#[cfg(feature = "runtime-agnostic")]

#[cfg(target_arch = "wasm32")]
struct DummyAsyncRead;

#[cfg(target_arch = "wasm32")]
impl tokio::io::AsyncRead for DummyAsyncRead {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        _buf.clear();
        std::task::Poll::Ready(Ok(()))
    }
}

#[cfg(target_arch = "wasm32")]
struct DummyAsyncWrite;

#[cfg(target_arch = "wasm32")]
impl tokio::io::AsyncWrite for DummyAsyncWrite {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        _buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::task::Poll::Ready(Ok(0))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}




// #[tokio_macros::main]
// see here: https://qiita-com.translate.goog/Nanai10a/items/0bbd60bf8215c006f3e4?_x_tr_sl=es&_x_tr_tl=en&_x_tr_hl=en&_x_tr_pto=wapp
// or: https://qiita.com/Nanai10a/items/0bbd60bf8215c006f3e4
#[tokio::main(flavor = "current_thread")]
async fn main() {

    #[cfg(not(feature = "runtime-agnostic"))]
    let stdin = tokio::io::stdin();
    // #[cfg(feature = "runtime-agnostic")]
    // let stdin = stdin();

    #[cfg(not(feature = "runtime-agnostic"))]
    let stdout = tokio::io::stdout();
    // #[cfg(feature = "runtime-agnostic")]
    // let stdout = stdout();

    #[cfg(target_arch = "wasm32")]
    let stdin = DummyAsyncRead;

    #[cfg(target_arch = "wasm32")]
    let stdout = DummyAsyncWrite;

    let (service, socket) = tower_lsp::LspService::build(Backend::new).finish();
    #[cfg(not(feature = "runtime-agnostic"))]
    tower_lsp::Server::new(stdin, stdout, socket)
        .serve(service)
        .await;


    #[cfg(feature = "runtime-agnostic")]
    tower_lsp::Server::new(stdin, stdout, socket)
        .serve(service)
        .await;
    // tower_lsp::Server::new(stdin, stdout, socket)
    //     .serve(service)
    //     .await;
}
