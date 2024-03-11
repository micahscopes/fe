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
use crate::language_server::MessageChannels;
use crate::workspace::{IngotFileContext, SyncableInputFile, Workspace};

use log::info;

use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_lsp::Client;

pub struct Backend {
    pub(crate) messaging: Arc<RwLock<MessageChannels>>,
    pub(crate) client: Arc<RwLock<Client>>,
    pub(crate) db: LanguageServerDatabase,
    pub(crate) workspace: Arc<RwLock<Workspace>>,
}

impl Backend {
    pub fn new(client: Arc<RwLock<Client>>, messaging: Arc<RwLock<MessageChannels>>) -> Self {
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
        let messaging = self.messaging.clone();
        let messaging = messaging.read().await;

        let mut initialized_stream = BroadcastStream::new(messaging.subscribe_initialize()).fuse();
        let mut shutdown_stream = BroadcastStream::new(messaging.subscribe_shutdown()).fuse();
        let did_open_stream = BroadcastStream::new(messaging.subscribe_did_open()).fuse();
        let did_change_stream = BroadcastStream::new(messaging.subscribe_did_change()).fuse();
        let mut change_stream = tokio_stream::StreamExt::merge(
            did_open_stream.map_ok(|params| TextDocumentItem {
                uri: params.text_document.uri,
                language_id: LANGUAGE_ID.to_string(),
                version: params.text_document.version,
                text: params.text_document.text,
            }),
            did_change_stream.map_ok(|params| TextDocumentItem {
                uri: params.text_document.uri,
                language_id: LANGUAGE_ID.to_string(),
                version: params.text_document.version,
                text: params.content_changes[0].text.clone(),
            }),
        )
        .fuse();
        let mut did_close_stream = BroadcastStream::new(messaging.subscribe_did_close()).fuse();
        let mut did_change_watched_files_stream =
            BroadcastStream::new(messaging.subscribe_did_change_watched_files()).fuse();

        let mut hover_stream = BroadcastStream::new(messaging.subscribe_hover()).fuse();
        let mut goto_definition_stream =
            BroadcastStream::new(messaging.subscribe_goto_definition()).fuse();

        // This is very important! We absolutely need to drop the messaging lock here.
        // TODO: make this more ergonomic and foolproof somehow
        std::mem::drop(messaging);

        info!("streams set up, looping on them now");
        loop {
            tokio::select! {
                Some(result) = initialized_stream.next() => {
                    if let Ok((initialization_params, responder)) = result {
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
                        responder.respond(Ok(initialize_result));
                    }
                }
                Some(result) = shutdown_stream.next() => {
                    if let Ok((_, responder)) = result {
                        info!("shutting down language server");
                        responder.respond(Ok(()));
                    }
                }
                Some(Ok(doc)) = change_stream.next() => {
                    info!("change detected: {:?}", doc.uri);
                    update_inputs(workspace.clone(), db, doc.clone()).await;

                    let db = db.snapshot();
                    let client = client.clone();
                    let workspace = workspace.clone();
                    tokio::spawn(
                        async move { handle_diagnostics(client, workspace, db, doc.uri).await }
                    );
                }
                Some(Ok(params)) = did_close_stream.next() => {
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
                Some(Ok(params)) = did_change_watched_files_stream.next() => {
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
                Some(Ok((params, responder))) = hover_stream.next() => {
                    let db = db.snapshot();
                    let workspace = workspace.clone();
                    let response = match tokio::spawn(handle_hover(db, workspace, params)).await {
                        Ok(response) => response,
                        Err(e) => {
                            eprintln!("Error handling hover: {:?}", e);
                            Ok(None)
                        }
                    };
                    responder.respond(response);
                }
                Some(Ok((params, responder))) = goto_definition_stream.next() => {
                    let db = db.snapshot();
                    let workspace = workspace.clone();
                    let response = match handle_goto_definition(db, workspace, params).await {
                        Ok(response) => response,
                        Err(e) => {
                            eprintln!("Error handling goto definition: {:?}", e);
                            None
                        }
                    };
                    responder.respond(Ok(response));
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
    client: Arc<RwLock<Client>>,
    workspace: Arc<RwLock<Workspace>>,
    db: Snapshot<LanguageServerDatabase>,
    url: lsp_types::Url,
) {
    let workspace = &workspace.read().await;
    // let client = &mut client.lock().await;
    let diagnostics = get_diagnostics(&db, workspace, url.clone());

    let diagnostics = diagnostics
        .unwrap()
        .into_iter()
        .map(|(uri, diags)| async {
            let client = client.clone();
            let client = client.read().await;
            client.publish_diagnostics(uri, diags, None).await
        })
        .collect::<Vec<_>>();


    futures::future::join_all(diagnostics).await;
}
