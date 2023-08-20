use std::io::BufRead;

use hir_analysis::name_resolution::EarlyResolvedPath;
use lsp_server::Response;
use serde::Deserialize;

use crate::{state::ServerState, goto::{goto_enclosing_path, Cursor}, util::position_to_offset};

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

    // let cursor: Cursor = params.text_document_position_params.position.into();
    let cursor: Cursor = position_to_offset(params.text_document_position_params.position, file_text.as_str());
    let file_path = std::path::Path::new(file_path);
    let top_mod = state.db.top_mod_from_file(file_path, file_text.as_str());
    let goto_info = goto_enclosing_path(&mut state.db, top_mod, cursor);
    
    let goto_info = match goto_info {
        Some(EarlyResolvedPath::Full(bucket)) => {
            bucket.iter().map(|x| x.pretty_path(&state.db).unwrap()).collect::<Vec<_>>()
            .join("\n")
            
        },
        Some(EarlyResolvedPath::Partial { res, unresolved_from }) => {
            res.pretty_path(&state.db).unwrap()
        },
        None => {
            String::from("No goto info available")
        }
    };

    let result = lsp_types::Hover {
        contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent::from(
            lsp_types::MarkupContent {
                kind: lsp_types::MarkupKind::Markdown,
                value: format!(
                    "### Hovering over:\n```{}```\n\n{}\n\n### Goto Info: \n\n{}",
                    &line,
                    serde_json::to_string_pretty(&params).unwrap(),
                    goto_info,
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
