use std::io::BufRead;

use lsp_server::Response;
use serde::Deserialize;

use hir::HirDb;

use crate::{state::ServerState, goto::{goto_enclosing_path, self, Cursor}};

pub(crate) fn handle_hover(
    state: &mut ServerState,
    req: lsp_server::Request,
) -> Result<(), anyhow::Error> {
    // TODO: get more relevant information for the hover
    let params = lsp_types::HoverParams::deserialize(req.params)?;
    let file_path = &params
            .text_document_position_params
            .text_document
            .uri
            .path();
    let file = std::fs::File::open(file_path)?;
    let reader = std::io::BufReader::new(file);
    let line = reader
        .lines()
        .nth(params.text_document_position_params.position.line as usize)
        .unwrap()
        .unwrap();

    let file_text = std::fs::read_to_string(file_path)?;

    let cursor: Cursor = params.text_document_position_params.position.character.into();
    let file_path = std::path::Path::new(file_path);
    let top_mod = state.db.top_mod_from_file(file_path, file_text.as_str());
    let goto_info = goto_enclosing_path(&mut state.db, top_mod, cursor);
    let (path_id, scope_id) = goto_info.map_or((None, None),
        |(path_id, scope_id)| (Some(path_id), Some(scope_id))
    );

    let result = lsp_types::Hover {
        contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent::from(
            lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: format!(
                    "### Hovering over:\n```{}```\n\n{}\n\n### Goto Info: {}\n",
                    &line,
                    serde_json::to_string_pretty(&params).unwrap(),
                    // scope_id
                    format!("\npath_id: {:?}\nscope_id: {:?}", path_id, scope_id)
                ),
            },
        )),
        range: None,
    };
    let response_message = Response {
        id: req.id,
        result: Some(serde_json::to_value(result)?),
        error: None,
    };

    state.send_response(response_message)?;
    Ok(())
}
