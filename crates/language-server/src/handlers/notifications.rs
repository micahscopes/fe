// use anyhow::{Error, Result};
// use fxhash::FxHashMap;
// // use log::info;
// use serde::Deserialize;

// use crate::{
//     db::LanguageServerDatabase,
//     diagnostics::get_diagnostics,
//     workspace::{IngotFileContext, SyncableIngotFileContext, SyncableInputFile, Workspace},
// };


// Currently this is used to handle document renaming since the "document open" handler is called
// before the "document was renamed" handler.
//
// The fix: handle document renaming more explicitly in the "will rename" flow, along with the document
// rename refactor.

// #[cfg(target_arch = "wasm32")]
// use crate::util::DummyFilePathConversion;

// pub fn handle_watched_file_changes(
//     db: &mut LanguageServerDatabase,
//     workspace: &mut Workspace,
//     note: lsp_server::Notification,
// ) -> Result<(), Error> {
//     let params = lsp_types::DidChangeWatchedFilesParams::deserialize(note.params)?;
//     let changes = params.changes;
//     let mut diagnostics = FxHashMap::<lsp_types::Url, Vec<lsp_types::Diagnostic>>::default();
//     for change in changes {
//         let uri = change.uri;
//         let path = uri.to_file_path().unwrap();

//         // TODO: sort out the mutable/immutable borrow issues here
//         {
//             // let db = &mut state.db.lock().unwrap();
//             // let workspace = &mut state.workspace.lock().unwrap();
//             match change.typ {
//                 lsp_types::FileChangeType::CREATED => {
//                     // TODO: handle this more carefully!
//                     // this is inefficient, a hack for now
//                     // let db = state.db.lock().unwrap();
//                     // let db = &mut state.db.lock().unwrap();
//                     let _ = workspace.sync(db);
//                     let input = workspace
//                         .input_from_file_path(db, path.to_str().unwrap())
//                         .unwrap();
//                     let _ = input.sync(db, None);
//                 }
//                 lsp_types::FileChangeType::CHANGED => {
//                     let input = workspace
//                         .input_from_file_path(db, path.to_str().unwrap())
//                         .unwrap();
//                     let _ = input.sync(db, None);
//                 }
//                 lsp_types::FileChangeType::DELETED => {
//                     // TODO: handle this more carefully!
//                     // this is inefficient, a hack for now
//                     let _ = workspace.sync(db);
//                 }
//                 _ => {}
//             }
//         }
//         // collect diagnostics for the file
//         if change.typ != lsp_types::FileChangeType::DELETED {
//             let diags = get_diagnostics(db, workspace, uri.clone())?;
//             for (uri, more_diags) in diags {
//                 let diags = diagnostics.entry(uri).or_insert_with(Vec::new);
//                 diags.extend(more_diags);
//             }
//         }
//     }
//     Ok(())
// }
