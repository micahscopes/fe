use crate::backend::Backend;

use crate::backend::workspace::SyncableIngotFileContext;

use async_lsp::{
    lsp_types::{notification::Exit, Diagnostic, InitializeParams, InitializeResult},
    ClientSocket, Error, LanguageClient, ResponseError,
};
use common::InputDb;
use futures::TryFutureExt;
use fxhash::FxHashSet;

use salsa::ParallelDatabase;

use super::{
    // actor::Message,
    capabilities::server_capabilities,
    hover::hover_helper,
    streams::{ChangeKind, FileChange},
};

use crate::backend::workspace::IngotFileContext;

use tracing::{error, info};

// impl Backend {

impl Backend {
    fn update_input_file_text(&mut self, path: &str, contents: String) {
        let input = self
            .workspace
            .touch_input_for_file_path(&mut self.db, path)
            .unwrap();
        input.set_text(&mut self.db).to(contents);
    }
}

use super::actor::{Actor, Message};

impl Actor for Backend {}

impl Message<InitializeParams> for Backend {
    type Reply = Result<InitializeResult, ResponseError>;

    fn handle(&mut self, message: InitializeParams) -> Self::Reply {
        info!("initializing language server!");

        let root = message.root_uri.unwrap().to_file_path().ok().unwrap();

        let _ = self.workspace.set_workspace_root(&mut self.db, &root);
        let _ = self.workspace.load_std_lib(&mut self.db, &root);
        let _ = self.workspace.sync(&mut self.db);

        let capabilities = server_capabilities();
        let initialize_result = InitializeResult {
            capabilities,
            server_info: Some(async_lsp::lsp_types::ServerInfo {
                name: String::from("fe-language-server"),
                version: Some(String::from(env!("CARGO_PKG_VERSION"))),
            }),
        };
        Ok(initialize_result)
    }
}

// impl Message<Exit> for Backend {
//     type Reply = ();

//     async fn handle(
//         &mut self,
//         _message: Exit,
//         _ctx: Context<'_, Self, Self::Reply>,
//     ) -> Self::Reply {
//         info!("shutting down language server");
//     }
// }

impl Message<FileChange> for Backend {
    type Reply = ();

    async fn handle(&mut self, message: FileChange) -> Self::Reply {
        let path = message
            .uri
            .to_file_path()
            .unwrap_or_else(|_| panic!("Failed to convert URI to path: {:?}", message.uri));

        let path = path.to_str().unwrap();

        match message.kind {
            ChangeKind::Open(contents) => {
                info!("file opened: {:?}", &path);
                self.update_input_file_text(path, contents);
            }
            ChangeKind::Create => {
                info!("file created: {:?}", &path);
                let contents = tokio::fs::read_to_string(&path).await.unwrap();
                self.update_input_file_text(path, contents)
            }
            ChangeKind::Edit(contents) => {
                info!("file edited: {:?}", &path);
                let contents = if let Some(text) = contents {
                    text
                } else {
                    tokio::fs::read_to_string(&path).await.unwrap()
                };
                self.update_input_file_text(path, contents);
            }
            ChangeKind::Delete => {
                info!("file deleted: {:?}", path);
                self.workspace
                    .remove_input_for_file_path(&mut self.db, path)
                    .unwrap();
            }
        }
        // self.tx_needs_diagnostics.send(path.to_string()).unwrap();
    }
}

// pub type FilesNeedDiagnostics = Vec<String>;
// impl Message<FilesNeedDiagnostics> for Backend {
//     type Reply = ();

//     async fn handle(
//         &mut self,
//         message: FilesNeedDiagnostics,
//         _ctx: Context<'_, Self, Self::Reply>,
//     ) -> Self::Reply {
//         let client = self.client.clone();
//         let ingot_files_need_diagnostics: FxHashSet<_> = message
//             .into_iter()
//             .filter_map(|file| self.workspace.get_ingot_for_file_path(&file))
//             .flat_map(|ingot| ingot.files(self.db.as_input_db()))
//             .cloned()
//             .collect();

//         let db = self.db.snapshot();
//         let compute_and_send_diagnostics = self
//             .workers
//             .spawn_blocking(move || {
//                 db.get_lsp_diagnostics(ingot_files_need_diagnostics.into_iter().collect())
//             })
//             .and_then(|diagnostics| async move {
//                 futures::future::join_all(diagnostics.into_iter().map(|(path, diagnostic)| {
//                     let diagnostics_params = async_lsp::lsp_types::PublishDiagnosticsParams {
//                         uri: path,
//                         diagnostics: diagnostic,
//                         version: None,
//                     };
//                     let mut client = client.clone();
//                     async move { client.publish_diagnostics(diagnostics_params) }
//                 }))
//                 .await;
//                 Ok(())
//             });
//         tokio::spawn(compute_and_send_diagnostics);
//     }
// }

// impl Message<async_lsp::lsp_types::HoverParams> for Backend {
//     type Reply = Result<Option<async_lsp::lsp_types::Hover>, ResponseError>;

//     async fn handle(
//         &mut self,
//         message: async_lsp::lsp_types::HoverParams,
//         _ctx: Context<'_, Self, Self::Reply>,
//     ) -> Self::Reply {
//         let file = self.workspace.get_input_for_file_path(
//             message
//                 .text_document_position_params
//                 .text_document
//                 .uri
//                 .path(),
//         );

//         let response = file.and_then(|file| {
//             hover_helper(&self.db, file, message).unwrap_or_else(|e| {
//                 error!("Error handling hover: {:?}", e);
//                 None
//             })
//         });

//         Ok(response)
//     }
// }
