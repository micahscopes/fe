//! Compile one real actor shader through the production interface derivation.
//! Use existing FE_BLOAT_* capture/intervention controls in separate processes
//! for matched experiments. The output is not a standalone runnable actor.
use std::{error::Error, fs, path::PathBuf, time::Instant};

use common::InputDb;
use driver::DriverDataBase;
use hir::hir_def::HirIngot;
use sha2::{Digest, Sha256};
use url::Url;

fn main() -> Result<(), Box<dyn Error>> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("usage: bloat_actor_stage INGOT_DIRECTORY STAGE OUTPUT_DIRECTORY".into());
    }
    let path = PathBuf::from(&args[0]).canonicalize()?;
    let stage = args[1].to_str().ok_or("stage name must be UTF-8")?;
    let output = PathBuf::from(&args[2]);
    fs::create_dir(&output)?; // Refuse to overwrite prior evidence.
    let url = Url::from_directory_path(&path).map_err(|_| "invalid ingot directory")?;
    let mut db = DriverDataBase::default();
    let started = Instant::now();
    if driver::init_ingot(&mut db, &url) {
        return Err("ingot initialization failed".into());
    }
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .ok_or("ingot not found")?;
    let diagnostics = db.run_on_ingot(ingot).format_diags(&db);
    if !diagnostics.is_empty() {
        return Err(diagnostics.into());
    }
    let frontend_ms = started.elapsed().as_millis();
    let started = Instant::now();
    let artifact = fe_codegen::compile_actor_shader_stage(&db, ingot.root_mod(&db), stage)?;
    let compile_ms = started.elapsed().as_millis();
    let wgsl = artifact.wgsl.ok_or("no WGSL emitted")?;
    let spirv = artifact
        .words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    fs::write(output.join("shader.wgsl"), wgsl.as_bytes())?;
    fs::write(output.join("shader.spv"), &spirv)?;
    let summary = serde_json::json!({
        "schema": "fe-actor-stage-probe/1",
        "ingot": path, "stage": stage,
        "frontend_ms": frontend_ms, "compile_ms": compile_ms,
        "wgsl_bytes": wgsl.len(), "spirv_bytes": spirv.len(),
        "wgsl_sha256": hex::encode(Sha256::digest(wgsl.as_bytes())),
        "spirv_sha256": hex::encode(Sha256::digest(&spirv)),
        "claim_limit": "Single shader compilation, not graph execution, generation time or runtime equivalence."
    });
    let text = serde_json::to_string_pretty(&summary)?;
    fs::write(output.join("summary.json"), &text)?;
    println!("{text}");
    Ok(())
}
