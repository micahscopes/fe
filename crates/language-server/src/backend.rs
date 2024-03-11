use crate::handlers::request::{handle_goto_definition, handle_hover};
use crate::workspace::SyncableIngotFileContext;
use futures::TryStreamExt;
use lsp_types::TextDocumentItem;
use salsa::{ParallelDatabase, Snapshot};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::capabilities::server_capabilities;
use crate::db::LanguageServerDatabase;

use crate::diagnostics::get_diagnostics;
use crate::globals::LANGUAGE_ID;
use crate::language_server::MessageReceivers;
use crate::workspace::{IngotFileContext, SyncableInputFile, Workspace};

use log::info;

use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::StreamExt;
use tower_lsp::Client;

pub struct Backend {
    pub(crate) messaging: MessageReceivers,
    pub(crate) client: Client,
    pub(crate) db: LanguageServerDatabase,
    pub(crate) workspace: Arc<RwLock<Workspace>>,
}

impl Backend {
    pub fn new(client: Client, messaging: MessageReceivers) -> Self {
        let db = LanguageServerDatabase::default();
        let workspace = Arc::new(RwLock::new(Workspace::default()));

        Self {
            messaging,
            client,
            db,
            workspace,
        }
    }
    pub async fn handle_streams(mut self) {
        info!("setting up streams");
        let workspace = self.workspace.clone();
        let db = &mut self.db;

        let client = self.client.clone();
        let messaging = self.messaging;
        // let messaging = self.messaging.clone();
        // let messaging = messaging.read().await;

        let mut initialized_stream = messaging.initialize_stream.fuse();
        let mut shutdown_stream = messaging.shutdown_stream.fuse();
        let did_open_stream = messaging.did_open_stream.fuse();
        let did_change_stream = messaging.did_change_stream.fuse();
        let mut change_stream = tokio_stream::StreamExt::merge(
            did_open_stream.map(|params| TextDocumentItem {
                uri: params.text_document.uri,
                language_id: LANGUAGE_ID.to_string(),
                version: params.text_document.version,
                text: params.text_document.text,
            }),
            did_change_stream.map(|params| TextDocumentItem {
                uri: params.text_document.uri,
                language_id: LANGUAGE_ID.to_string(),
                version: params.text_document.version,
                text: params.content_changes[0].text.clone(),
            }),
        )
        .fuse();
        let mut did_close_stream = messaging.did_close_stream.fuse();
        let mut did_change_watched_files_stream =
            messaging.did_change_watched_files_stream.fuse();

        let mut hover_stream = messaging.hover_stream.fuse();
        let mut goto_definition_stream =
            messaging.goto_definition_stream.fuse();

        info!("streams set up, looping on them now");
        loop {
            tokio::select! {
                Some(result) = initialized_stream.next() => {
                    if let (initialization_params, responder) = result {
                        info!("initializing language server!");
                        // setup workspace
                        // let workspace = self.workspace.clone();
                        let mut workspace = self.workspace.write().await;
                        let _ = workspace.set_workspace_root(
                            db,
                            initialization_params
                                .root_uri
                                .unwrap()
                                .to_file_path()
                                .ok()
                                .unwrap(),
                        );

                        let capabilities = server_capabilities();
                        let initialize_result = lsp_types::InitializeResult {
                            capabilities,
                            server_info: Some(lsp_types::ServerInfo {
                                name: String::from("fe-language-server"),
                                version: Some(String::from(env!("CARGO_PKG_VERSION"))),
                            }),
                        };
                        responder.send(Ok(initialize_result));
                    }
                }
                Some(result) = shutdown_stream.next() => {
                    if let (_, responder) = result {
                        info!("shutting down language server");
                        responder.send(Ok(()));
                    }
                }
                Some(doc) = change_stream.next() => {
                    info!("change detected: {:?}", doc.uri);
                    update_inputs(workspace.clone(), db, doc.clone()).await;

                    let db = db.snapshot();
                    let client = client.clone();
                    let workspace = workspace.clone();
                    tokio::spawn(
                        async move { handle_diagnostics(client, workspace, db, doc.uri).await }
                    );
                }
                Some(params) = did_close_stream.next() => {
                    let workspace = &mut workspace.write().await;
                    let input = workspace
                        .touch_input_from_file_path(
                            db,
                            params
                                .text_document
                                .uri
                                .to_file_path()
                                .unwrap()
                                .to_str()
                                .unwrap(),
                        )
                        .unwrap();
                    let _ = input.sync(db, None);
                }
                Some(params) = did_change_watched_files_stream.next() => {
                    let changes = params.changes;
                    for change in changes {
                        let uri = change.uri;
                        let path = uri.to_file_path().unwrap();

                        match change.typ {
                            lsp_types::FileChangeType::CREATED => {
                                // TODO: handle this more carefully!
                                // this is inefficient, a hack for now
                                let workspace = &mut workspace.write().await;
                                let _ = workspace.sync(db);
                                let input = workspace
                                    .touch_input_from_file_path(db, path.to_str().unwrap())
                                    .unwrap();
                                let _ = input.sync(db, None);
                            }
                            lsp_types::FileChangeType::CHANGED => {
                                let workspace = &mut workspace.write().await;
                                let input = workspace
                                    .touch_input_from_file_path(db, path.to_str().unwrap())
                                    .unwrap();
                                let _ = input.sync(db, None);
                            }
                            lsp_types::FileChangeType::DELETED => {
                                let workspace = &mut workspace.write().await;
                                // TODO: handle this more carefully!
                                // this is inefficient, a hack for now
                                let _ = workspace.sync(db);
                            }
                            _ => {}
                        }
                        // collect diagnostics for the file
                        if change.typ != lsp_types::FileChangeType::DELETED {
                            let text = std::fs::read_to_string(path).unwrap();
                            update_inputs(workspace.clone(), db, TextDocumentItem {
                                uri: uri.clone(),
                                language_id: LANGUAGE_ID.to_string(),
                                version: 0,
                                text: text.clone(),
                            }).await;
                            
                            let client = client.clone();
                            let workspace = workspace.clone();
                            let db = db.snapshot();

                            tokio::spawn(
                                async move {
                                    handle_diagnostics(
                                        client,
                                        workspace,
                                        db,
                                        uri.clone(),
                                    ).await
                                }
                            );
                        }
                    }
                }
                Some((params, responder)) = hover_stream.next() => {
                    let db = db.snapshot();
                    let workspace = workspace.clone();
                    let response = match tokio::spawn(handle_hover(db, workspace, params)).await {
                        Ok(response) => response,
                        Err(e) => {
                            eprintln!("Error handling hover: {:?}", e);
                            Ok(None)
                        }
                    };
                    responder.send(response);
                }
                Some((params, responder)) = goto_definition_stream.next() => {
                    let db = db.snapshot();
                    let workspace = workspace.clone();
                    let response = match handle_goto_definition(db, workspace, params).await {
                        Ok(response) => response,
                        Err(e) => {
                            eprintln!("Error handling goto definition: {:?}", e);
                            None
                        }
                    };
                    responder.send(Ok(response));
                }
            }
        }
    }
}

async fn update_inputs(
    workspace: Arc<RwLock<Workspace>>,
    db: &mut LanguageServerDatabase,
    params: TextDocumentItem,
) {
    let workspace = &mut workspace.write().await;
    let input = workspace
        .touch_input_from_file_path(
            db,
            params
                .uri
                .to_file_path()
                .expect("Failed to convert URI to file path")
                .to_str()
                .expect("Failed to convert file path to string"),
        )
        .unwrap();
    let _ = input.sync(db, Some(params.text.clone()));
}

async fn handle_diagnostics(
    client: Client,
    workspace: Arc<RwLock<Workspace>>,
    db: Snapshot<LanguageServerDatabase>,
    url: lsp_types::Url,
) {
    let workspace = &workspace.read().await;
    let diagnostics = get_diagnostics(&db, workspace, url.clone());

    let client = client.clone();
    let diagnostics = diagnostics
        .unwrap()
        .into_iter()
        .map(|(uri, diags)| async {
            client.publish_diagnostics(uri, diags, None).await
        })
        .collect::<Vec<_>>();


    futures::future::join_all(diagnostics).await;
}
