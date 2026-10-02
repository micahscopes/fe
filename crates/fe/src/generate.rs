//! Explicit file-based entry to the existing checked scalar authoring protocol.
use std::io::Write;

use camino::Utf8PathBuf;
use clap::Args;
use common::{
    InputDb,
    stdlib::{HasBuiltinCore, HasBuiltinStd},
};
use driver::{
    DriverDataBase,
    generation::{
        FunctionEmissionGrant, FunctionEmissionRequest, FunctionTemplate, GenerationBudget,
        GenerationSession,
    },
};
use hir::hir_def::ItemKind;
use url::Url;

#[derive(Debug, Clone, Args)]
pub struct GenerateArgs {
    /// Standalone provider .fe file. Project configuration and custom dependencies are not loaded.
    pub provider: Utf8PathBuf,
    /// Safe nullary const function returning FunctionEmission<ScalarBody<T, N>>.
    #[arg(long, value_name = "NAME")]
    pub provider_function: String,
    /// Standalone target .fe file containing the complete declaration with an empty body.
    #[arg(long, value_name = "FILE")]
    pub template: Utf8PathBuf,
    /// Top-level function whose body this invocation authorizes filling.
    #[arg(long, value_name = "NAME")]
    pub target_function: String,
    /// Request-local routing ID granted to the provider.
    #[arg(long, value_name = "ID")]
    pub target: u64,
    /// New .fe source artifact. Existing files are never overwritten; its parent must exist.
    #[arg(long, value_name = "FILE")]
    pub output: Utf8PathBuf,
}

fn read_source(path: &Utf8PathBuf) -> Result<(Url, String), String> {
    if path.extension() != Some("fe") {
        return Err(format!("expected a standalone .fe file: {path}"));
    }
    let canonical = std::fs::canonicalize(path).map_err(|error| format!("{path}: {error}"))?;
    let source = std::fs::read_to_string(&canonical).map_err(|error| format!("{path}: {error}"))?;
    let url = Url::from_file_path(canonical).map_err(|_| format!("invalid file path: {path}"))?;
    Ok((url, source))
}

pub fn generate(args: &GenerateArgs) -> Result<(), String> {
    if args.output.extension() != Some("fe") {
        return Err("output must be a new .fe source file".into());
    }
    let (provider_url, provider_source) = read_source(&args.provider)?;
    let (template_url, template_source) = read_source(&args.template)?;
    let mut db = DriverDataBase::default();
    db.initialize_builtin_core();
    db.initialize_builtin_std();
    let file = db
        .workspace()
        .update(&mut db, provider_url, provider_source);
    let provider = db
        .top_mod(file)
        .children_non_nested(&db)
        .find_map(|item| {
            let ItemKind::Func(func) = item else {
                return None;
            };
            func.name(&db)
                .to_opt()
                .is_some_and(|name| name.data(&db) == &args.provider_function)
                .then_some(func)
        })
        .ok_or_else(|| {
            format!(
                "provider has no top-level function `{}`",
                args.provider_function
            )
        })?;
    let artifact =
        GenerationSession::new("cli-generate".into(), GenerationBudget::new(1, 64 * 1024))
            .emit_scalar_body(
                &db,
                provider,
                FunctionEmissionRequest {
                    key: "output".into(),
                    grants: vec![FunctionEmissionGrant {
                        target: args.target,
                        template: FunctionTemplate {
                            url: template_url,
                            source: template_source,
                            function_name: args.target_function.clone(),
                        },
                    }],
                },
            )
            .map_err(|error| format!("generation {:?}: {}", error.kind, error.message))?;

    // Publish only a fully checked artifact, without truncating an input or an
    // existing output. A sibling temporary keeps publication on the same filesystem.
    let parent = args
        .output
        .parent()
        .filter(|path| !path.as_str().is_empty())
        .unwrap_or(camino::Utf8Path::new("."));
    let mut pending = tempfile::Builder::new()
        .prefix(".fe-generate-")
        .tempfile_in(parent)
        .map_err(|error| format!("{}: {error}", args.output))?;
    pending
        .write_all(artifact.source().as_bytes())
        .map_err(|error| format!("{}: {error}", args.output))?;
    pending
        .persist_noclobber(&args.output)
        .map_err(|error| format!("{}: {}", args.output, error.error))?;
    Ok(())
}
