use async_lsp::{lsp_types::{
    DidChangeWatchedFilesParams, DidChangeWatchedFilesRegistrationOptions,
    DidCloseTextDocumentParams, FileSystemWatcher, GlobPattern, InitializeParams, InitializeResult,
    Registration,
}, ClientSocket, Result};
use tracing::{error, info};

// use tower_lsp::{jsonrpc::Result, Client, LanguageServer};

// pub(crate) struct Server {
//     pub(crate) messaging: MessageSenders,
//     pub(crate) client: ClientSocket,
// }

// impl Server {
//     pub(crate) async fn register_watchers(&self) -> Result<()> {
//         let registration = Registration {
//             id: String::from("watch-fe-files"),
//             method: String::from("workspace/didChangeWatchedFiles"),
//             register_options: Some(
//                 serde_json::to_value(DidChangeWatchedFilesRegistrationOptions {
//                     watchers: vec![FileSystemWatcher {
//                         glob_pattern: GlobPattern::String("**/*.fe".to_string()),
//                         kind: None,
//                     }],
//                 })
//                 .unwrap(),
//             ),
//         };
//         self.client.register_capability(vec![registration]).await
//     }

//     pub(crate) fn new(client: ClientSocket, messaging: MessageSenders) -> Self {
//         Self { messaging, client }
//     }
// }

// #[language_server_macros::message_channels]
// #[tower_lsp::async_trait]
// impl LanguageServer for Server {
//     async fn initialize(&self, initialize_params: InitializeParams) -> Result<InitializeResult> {
//         // forward the initialize request to the messaging system
//         let rx = self.messaging.send_initialize(initialize_params);

//         info!("awaiting initialization result");
//         match rx.await {
//             Ok(initialize_result) => initialize_result,
//             Err(e) => {
//                 error!("Failed to initialize: {}", e);
//                 return Err(tower_lsp::jsonrpc::Error::internal_error());
//             }
//         }
//     }

//     async fn initialized(&self, _params: async_lsp::lsp_types::InitializedParams) {
//         info!("initialized... registering file watchers");
//         // register file watchers
//         if let Err(e) = self.register_watchers().await {
//             error!("Failed to register file watchers: {}", e);
//         } else {
//             info!("registered watchers");
//         }
//     }

//     async fn shutdown(&self) -> tower_lsp::jsonrpc::Result<()> {
//         Ok(())
//     }

//     async fn did_open(&self, params: async_lsp::lsp_types::DidOpenTextDocumentParams) {
//         self.messaging.send_did_open(params);
//     }

//     async fn did_change(&self, params: async_lsp::lsp_types::DidChangeTextDocumentParams) {
//         self.messaging.send_did_change(params);
//     }

//     async fn did_close(&self, params: DidCloseTextDocumentParams) {
//         self.messaging.send_did_close(params);
//     }

//     async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
//         self.messaging.send_did_change_watched_files(params);
//     }

//     async fn hover(&self, params: async_lsp::lsp_types::HoverParams) -> Result<Option<async_lsp::lsp_types::Hover>> {
//         let rx = self.messaging.send_hover(params);
//         rx.await.expect("hover response")
//     }

//     async fn goto_definition(
//         &self,
//         params: async_lsp::lsp_types::GotoDefinitionParams,
//     ) -> Result<Option<async_lsp::lsp_types::GotoDefinitionResponse>> {
//         let rx = self.messaging.send_goto_definition(params);
//         rx.await.expect("goto definition response")
//     }
// }
