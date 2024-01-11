use anyhow::{Error, Result};
use fxhash::FxHashMap;
// use log::info;
use serde::Deserialize;

use crate::{
    backend::Backend,
    // state::ServerState,
    util::diag_to_lsp,
    workspace::{IngotFileContext, SyncableIngotFileContext, SyncableInputFile},
};

#[cfg(target_arch = "wasm32")]
use crate::util::DummyFilePathConversion;

fn run_diagnostics(
    state: &Backend,
    path: &str,
) -> Vec<common::diagnostics::CompleteDiagnostic> {
    let db = &mut *state.db.lock().unwrap();
    let workspace = &mut *state.workspace.lock().unwrap();
    let file_path = path;
    let top_mod = workspace.top_mod_from_file_path(db, file_path).unwrap();
    db.analyze_top_mod(top_mod);
    db.finalize_diags()
}

pub fn get_diagnostics(
    state: &Backend,
    uri: lsp_types::Url,
) -> Result<FxHashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>>, Error> {
    let diags = run_diagnostics(state, uri.to_file_path().unwrap().to_str().unwrap());
    let db = &mut *state.db.lock().unwrap();

    let diagnostics = diags
        .into_iter()
        .flat_map(|diag| diag_to_lsp(diag, db).clone());

    // we need to reduce the diagnostics to a map from URL to Vec<Diagnostic>
    let mut result = FxHashMap::<lsp_types::Url, Vec<lsp_types::Diagnostic>>::default();

    // add a null diagnostic to the result for the given URL
    let _ = result.entry(uri.clone()).or_insert_with(Vec::new);

    diagnostics.for_each(|(uri, more_diags)| {
        let diags = result.entry(uri).or_insert_with(Vec::new);
        diags.extend(more_diags);
    });

    Ok(result)
}

pub fn handle_document_did_open(
    state: &mut Backend,
    note: lsp_server::Notification,
) -> Result<(), Error> {
    let params = lsp_types::DidOpenTextDocumentParams::deserialize(note.params)?;
    {
        let db = &mut *state.db.lock().unwrap();
        let workspace = &mut *state.workspace.lock().unwrap();
        let input = workspace
            .input_from_file_path(
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
    let diagnostics = get_diagnostics(state, params.text_document.uri.clone())?;
    send_diagnostics(state, diagnostics)
}

// Currently this is used to handle document renaming since the "document open" handler is called
// before the "document was renamed" handler.
//
// The fix: handle document renaming more explicitly in the "will rename" flow, along with the document
// rename refactor.
pub fn handle_document_did_close(
    state: &mut Backend,
    note: lsp_server::Notification,
) -> Result<(), Error> {
    let params = lsp_types::DidCloseTextDocumentParams::deserialize(note.params)?;
    let db = &mut *state.db.lock().unwrap();
    let workspace = &mut *state.workspace.lock().unwrap();
    let input = workspace
        .input_from_file_path(
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
    input.sync(db, None)
}

pub fn handle_document_did_change(
    state: &mut Backend,
    note: lsp_server::Notification,
) -> Result<(), Error> {
    let params = lsp_types::DidChangeTextDocumentParams::deserialize(note.params)?;
    {
        let db = &mut *state.db.lock().unwrap();
        let workspace = &mut *state.workspace.lock().unwrap();
        let input = workspace
            .input_from_file_path(
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
        let _ = input.sync(db, Some(params.content_changes[0].text.clone()));
    }
    let diagnostics = get_diagnostics(state, params.text_document.uri.clone())?;
    // info!("sending diagnostics... {:?}", diagnostics);
    send_diagnostics(state, diagnostics)
}

pub fn send_diagnostics(
    _state: &mut Backend,
    diagnostics: FxHashMap<lsp_types::Url, Vec<lsp_types::Diagnostic>>,
) -> Result<(), Error> {
    let _results = diagnostics.into_iter().map(|(uri, diags)| {
        let result = lsp_types::PublishDiagnosticsParams {
            uri,
            diagnostics: diags,
            version: None,
        };
        lsp_server::Message::Notification(lsp_server::Notification {
            method: String::from("textDocument/publishDiagnostics"),
            params: serde_json::to_value(result).unwrap(),
        })
    });

    // results.for_each(|result| {
    //     let sender = state.client;
    //     let _ = sender.send(result);
    // });

    Ok(())
}

pub fn handle_watched_file_changes(
    state: &mut Backend,
    note: lsp_server::Notification,
) -> Result<(), Error> {
    let params = lsp_types::DidChangeWatchedFilesParams::deserialize(note.params)?;
    let changes = params.changes;
    let mut diagnostics = FxHashMap::<lsp_types::Url, Vec<lsp_types::Diagnostic>>::default();
    for change in changes {
        let uri = change.uri;
        let path = uri.to_file_path().unwrap();

        // TODO: sort out the mutable/immutable borrow issues here
        {
            let db = &mut state.db.lock().unwrap();
            let workspace = &mut state.workspace.lock().unwrap();
            match change.typ {
                lsp_types::FileChangeType::CREATED => {
                    // TODO: handle this more carefully!
                    // this is inefficient, a hack for now
                    // let db = state.db();
                    // let db = &mut state.db.lock().unwrap();
                    let _ = workspace.sync(db);
                    let input = workspace
                        .input_from_file_path(db, path.to_str().unwrap())
                        .unwrap();
                    let _ = input.sync(db, None);
                }
                lsp_types::FileChangeType::CHANGED => {
                    let input = workspace
                        .input_from_file_path(db, path.to_str().unwrap())
                        .unwrap();
                    let _ = input.sync(db, None);
                }
                lsp_types::FileChangeType::DELETED => {
                    // TODO: handle this more carefully!
                    // this is inefficient, a hack for now
                    let _ = workspace.sync(db);
                }
                _ => {}
            }
        }
        // collect diagnostics for the file
        if change.typ != lsp_types::FileChangeType::DELETED {
            let diags = get_diagnostics(state, uri.clone())?;
            for (uri, more_diags) in diags {
                let diags = diagnostics.entry(uri).or_insert_with(Vec::new);
                diags.extend(more_diags);
            }
        }
    }
    // info!("sending diagnostics... {:?}", diagnostics);
    send_diagnostics(state, diagnostics)
    // Ok(())
}
