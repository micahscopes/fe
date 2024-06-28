use std::{ops::ControlFlow, time::Duration};

use crate::backend::Backend;
// use crate::server::MessageReceivers;
use anyhow::Error;
use async_lsp::{router::Router, ClientSocket, MainLoop};
use tower::ServiceBuilder;

pub struct FileChange {
    pub uri: url::Url,
    pub kind: ChangeKind,
}
pub enum ChangeKind {
    Open(String),
    Create,
    Edit(Option<String>),
    Delete,
}

struct TickEvent;

// pub fn handle_lsp_events() -> MainLoop<Router<Backend>> {
//     let (server, _) = async_lsp::MainLoop::new_server(|client| {
//         tokio::spawn({
//             let client = client.clone();
//             async move {
//                 let mut interval = tokio::time::interval(Duration::from_secs(1));
//                 loop {
//                     interval.tick().await;
//                     if client.emit(TickEvent).is_err() {
//                         break;
//                     }
//                 }
//             }
//         });

//         let backend = Backend::new(client);
//         let mut router = Router::new(backend);

//         router
//             .request::<async_lsp::lsp_types::request::Initialize, _>(|st, params| async move {
//                 st.handle_initialized(params)
//             })
//             .event::<TickEvent>(|st, _| {
//                 // info!("tick");
//                 // st.counter += 1;
//                 ControlFlow::Continue(())
//             });
//         let service = ServiceBuilder::new().service(router);
//         service
//     });
//     server
// }
