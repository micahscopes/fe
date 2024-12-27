use common::{
    input::{IngotKind, Version},
    InputFile, InputIngot,
};
use tracing::info;

use super::db::LanguageServerDatabase;

#[derive(rust_embed::RustEmbed)]
#[folder = "../library/std"]
struct StdLib;

pub fn get_std_ingot(db: &mut LanguageServerDatabase) -> InputIngot {
    let std_ingot = InputIngot::new(
        db,
        "/std/", // Use a canonical path for std
        IngotKind::Std,
        Version::new(0, 0, 1),
        Default::default(),
    );

    info!("Loading std lib...");

    // First collect all files and create the InputFiles
    let mut std_files = Vec::new();
    let mut root_file = None;

    for path in StdLib::iter() {
        let path_str = path.as_ref();
        info!("Loading stdlib file: {}", path_str);
        if let Some(file) = StdLib::get(path_str) {
            if let Ok(contents) = String::from_utf8(file.data.as_ref().to_vec()) {
                // Create InputFile with paths relative to std root
                let input_file = InputFile::new(db, std_ingot, path_str.into(), contents);

                // Identify the root file (probably src/lib.fe or similar)
                if path_str == "src/lib.fe" {
                    root_file = Some(input_file);
                }

                std_files.push(input_file);
            }
        }
    }

    // Set up the ingot structure
    if let Some(root) = root_file {
        std_ingot.set_root_file(db, root);
    }

    assert!(root_file.is_some(), "std library must have a root file");

    // Add all files to the ingot
    std_ingot.set_files(db, std_files.into_iter().collect());

    std_ingot
}
