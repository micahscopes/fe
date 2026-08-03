//! In-memory Fe compilation facade.
//!
//! This first vertical slice deliberately reuses the existing native driver and
//! codegen crates. Its protocol and behavior are pinned before Phase 1/2 split
//! filesystem, resolver, reporter, and non-Wasm backends out of the wasm32
//! dependency graph.

use codegen::{BackendKind, OptLevel, layout_for};
use common::{
    InputDb,
    diagnostics::{CompleteDiagnostic, LabelStyle, Severity},
};
use compiler_db::DriverDataBase;
use fe_compiler_protocol::{
    Artifact, ArtifactKind, CompileRequest, CompileResult, CompileTarget, CompilerIdentity,
    Diagnostic, DiagnosticLabel, DiagnosticSeverity, InterfaceFunction, InterfaceManifest,
    ProtocolError, ProtocolVersion, SOURCE_DEPENDENCY_INVENTORY_VERSION, SourceDependency,
    SourceDependencyInventory, sha256_hex, source_set_sha256,
};
use url::Url;

#[derive(Debug)]
pub enum CompileFacadeError {
    Protocol(ProtocolError),
    InvalidSourceUrl { url: String, detail: String },
    RootUnavailable(String),
    Backend(String),
    Artifact(String),
    UnsupportedTarget(CompileTarget),
}

impl std::fmt::Display for CompileFacadeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for CompileFacadeError {}

impl From<ProtocolError> for CompileFacadeError {
    fn from(error: ProtocolError) -> Self {
        Self::Protocol(error)
    }
}

/// Compile virtual Fe sources without project discovery or filesystem access.
///
/// Only the Wasm target is wired in this initial facade slice. Other semantic
/// targets are protocol-valid but fail explicitly until their artifact
/// adapters land.
pub fn compile(request: &CompileRequest) -> Result<CompileResult, CompileFacadeError> {
    request.validate()?;
    if request.target != CompileTarget::Wasm {
        return Err(CompileFacadeError::UnsupportedTarget(request.target));
    }

    let mut db = DriverDataBase::default();
    for source in &request.sources {
        let url = parse_url(&source.url)?;
        db.workspace()
            .touch(&mut db, url, Some(source.text.clone()));
    }
    let root_url = parse_url(&request.root)?;
    let root_file = db
        .workspace()
        .get(&db, &root_url)
        .ok_or_else(|| CompileFacadeError::RootUnavailable(request.root.clone()))?;
    let source_dependencies = source_dependencies(request, &db, root_file);
    let top_mod = db.top_mod(root_file);
    let complete = db.run_on_top_mod(top_mod).complete(&db);
    let diagnostics = complete
        .iter()
        .map(|diagnostic| protocol_diagnostic(&db, diagnostic))
        .collect::<Vec<_>>();

    if complete
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return Ok(result(
            request,
            diagnostics,
            Vec::new(),
            InterfaceManifest::default(),
            source_dependencies,
        ));
    }

    let output = BackendKind::Wasm
        .create()
        .compile(
            &db,
            top_mod,
            layout_for(BackendKind::Wasm),
            match request.options.optimization {
                fe_compiler_protocol::OptimizationLevel::None => OptLevel::O0,
                fe_compiler_protocol::OptimizationLevel::Size
                | fe_compiler_protocol::OptimizationLevel::Speed => OptLevel::O1,
            },
        )
        .map_err(|error| CompileFacadeError::Backend(error.to_string()))?;
    let bytes = output
        .into_bytecode()
        .ok_or_else(|| CompileFacadeError::Artifact("Wasm backend returned no bytes".to_owned()))?;
    wasmparser::validate(&bytes)
        .map_err(|error| CompileFacadeError::Artifact(format!("invalid Wasm: {error}")))?;
    let interface = wasm_interface(&bytes)?;
    let artifacts = vec![Artifact::new(
        "module.wasm",
        ArtifactKind::WasmModule,
        "application/wasm",
        bytes,
    )];
    Ok(result(
        request,
        diagnostics,
        artifacts,
        interface,
        source_dependencies,
    ))
}

fn parse_url(value: &str) -> Result<Url, CompileFacadeError> {
    Url::parse(value).map_err(|error| CompileFacadeError::InvalidSourceUrl {
        url: value.to_owned(),
        detail: error.to_string(),
    })
}

fn result(
    request: &CompileRequest,
    diagnostics: Vec<Diagnostic>,
    artifacts: Vec<Artifact>,
    interface: InterfaceManifest,
    source_dependencies: SourceDependencyInventory,
) -> CompileResult {
    CompileResult {
        protocol: ProtocolVersion::CURRENT,
        compiler: CompilerIdentity {
            name: "fe".to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            build: "workspace".to_owned(),
        },
        target: request.target,
        source_set_sha256: source_set_sha256(&request.sources),
        source_dependencies: Some(source_dependencies),
        diagnostics,
        artifacts,
        interface,
    }
}

fn source_dependencies(
    request: &CompileRequest,
    db: &DriverDataBase,
    root_file: common::file::File,
) -> SourceDependencyInventory {
    let supplied = request
        .sources
        .iter()
        .map(|source| (source.url.as_str(), source))
        .collect::<std::collections::BTreeMap<_, _>>();
    let sources = db
        .source_dependency_urls(root_file)
        .into_iter()
        .filter_map(|url| {
            let source = supplied.get(url.as_str())?;
            Some(SourceDependency {
                url: source.url.clone(),
                sha256: source
                    .sha256
                    .clone()
                    .unwrap_or_else(|| sha256_hex(source.text.as_bytes())),
            })
        })
        .collect();
    SourceDependencyInventory {
        version: SOURCE_DEPENDENCY_INVENTORY_VERSION,
        root: request.root.clone(),
        sources,
    }
}

fn protocol_diagnostic(db: &DriverDataBase, diagnostic: &CompleteDiagnostic) -> Diagnostic {
    Diagnostic {
        severity: match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::Error,
            Severity::Warning => DiagnosticSeverity::Warning,
            Severity::Note => DiagnosticSeverity::Note,
        },
        code: Some(diagnostic.error_code.to_string()),
        message: diagnostic.message.clone(),
        labels: diagnostic
            .sub_diagnostics
            .iter()
            .filter_map(|label| {
                let span = label.span.as_ref()?;
                Some(DiagnosticLabel {
                    source_url: span.file.url(db)?.to_string(),
                    start: u32::from(span.range.start()),
                    end: u32::from(span.range.end()),
                    message: (!label.message.is_empty()).then(|| label.message.clone()),
                    primary: label.style == LabelStyle::Primary,
                })
            })
            .collect(),
        notes: diagnostic.notes.clone(),
    }
}

fn wasm_interface(bytes: &[u8]) -> Result<InterfaceManifest, CompileFacadeError> {
    use wasmparser::{ExternalKind, Payload, TypeRef};

    let mut imports = Vec::new();
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        match payload.map_err(|error| CompileFacadeError::Artifact(error.to_string()))? {
            Payload::ImportSection(section) => {
                for import in section.into_imports() {
                    let import =
                        import.map_err(|error| CompileFacadeError::Artifact(error.to_string()))?;
                    if matches!(import.ty, TypeRef::Func(_)) {
                        imports.push(InterfaceFunction {
                            module: import.module.to_owned(),
                            name: import.name.to_owned(),
                            signature_complete: false,
                            params: Vec::new(),
                            results: Vec::new(),
                        });
                    }
                }
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export =
                        export.map_err(|error| CompileFacadeError::Artifact(error.to_string()))?;
                    if export.kind == ExternalKind::Func {
                        exports.push(InterfaceFunction {
                            module: String::new(),
                            name: export.name.to_owned(),
                            signature_complete: false,
                            params: Vec::new(),
                            results: Vec::new(),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    imports.sort_by(|left, right| (&left.module, &left.name).cmp(&(&right.module, &right.name)));
    exports.sort_by(|left, right| (&left.module, &left.name).cmp(&(&right.module, &right.name)));
    let interface = InterfaceManifest {
        host_world: None,
        imports,
        exports,
        resources: Vec::new(),
    };
    interface.validate()?;
    Ok(interface)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fe_compiler_protocol::{CompileOptions, VirtualSource};

    fn request(source: &str) -> CompileRequest {
        CompileRequest {
            protocol: ProtocolVersion::CURRENT,
            root: "fe-memory:///app.fe".to_owned(),
            sources: vec![VirtualSource::new("fe-memory:///app.fe", source)],
            target: CompileTarget::Wasm,
            entries: vec!["main".to_owned()],
            options: CompileOptions::default(),
        }
    }

    #[test]
    fn compiles_virtual_source_to_valid_executable_wasm() {
        let result = compile(&request("pub fn main() -> u32 { 42 }")).unwrap();
        result.validate().unwrap();
        assert!(result.diagnostics.is_empty());
        let wasm = result
            .artifacts
            .iter()
            .find(|artifact| artifact.kind == ArtifactKind::WasmModule)
            .unwrap();
        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &wasm.bytes).unwrap();
        let mut store = wasmtime::Store::new(&engine, ());
        let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
        let main = instance
            .get_typed_func::<(), u32>(&mut store, "main")
            .unwrap();
        assert_eq!(main.call(&mut store, ()).unwrap(), 42);
        assert!(
            result
                .interface
                .exports
                .iter()
                .any(|export| export.name == "main")
        );
        let dependencies = result.source_dependencies.as_ref().unwrap();
        dependencies.validate().unwrap();
        assert_eq!(dependencies.root, "fe-memory:///app.fe");
        assert_eq!(dependencies.sources.len(), 1);
        assert_eq!(dependencies.sources[0].url, "fe-memory:///app.fe");
    }

    #[test]
    fn invalid_source_returns_structured_diagnostics_without_artifacts() {
        let result = compile(&request("pub fn main() -> Missing { 42 }")).unwrap();
        assert!(result.artifacts.is_empty());
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
        );
        assert!(
            result
                .diagnostics
                .iter()
                .flat_map(|d| &d.labels)
                .any(|label| label.source_url == "fe-memory:///app.fe" && label.end > label.start)
        );
    }

    #[test]
    fn unsupported_semantic_target_fails_explicitly() {
        let mut request = request("pub fn main() -> u32 { 42 }");
        request.target = CompileTarget::Native;
        assert!(matches!(
            compile(&request),
            Err(CompileFacadeError::UnsupportedTarget(CompileTarget::Native))
        ));
    }

    #[test]
    fn dependency_inventory_excludes_supplied_but_unrelated_sources() {
        let mut request = request("pub fn main() -> u32 { 42 }");
        request.sources.push(VirtualSource::new(
            "fe-memory:///unused.fe",
            "pub fn unused() -> u32 { 7 }",
        ));
        let result = compile(&request).unwrap();
        let dependencies = result.source_dependencies.unwrap();
        assert_eq!(
            dependencies
                .sources
                .iter()
                .map(|source| source.url.as_str())
                .collect::<Vec<_>>(),
            ["fe-memory:///app.fe"]
        );
    }

    #[test]
    fn dependency_inventory_includes_supplied_ingot_module_tree() {
        let request = CompileRequest {
            protocol: ProtocolVersion::CURRENT,
            root: "fe-memory:///app/src/lib.fe".to_owned(),
            sources: vec![
                VirtualSource::new(
                    "fe-memory:///app/fe.toml",
                    "[ingot]\nname = \"app\"\nversion = \"0.0.1\"\n",
                ),
                VirtualSource::new(
                    "fe-memory:///app/src/helper.fe",
                    "pub fn helper() -> u32 { 7 }",
                ),
                VirtualSource::new("fe-memory:///app/src/lib.fe", "pub fn main() -> u32 { 42 }"),
            ],
            target: CompileTarget::Wasm,
            entries: vec!["main".to_owned()],
            options: CompileOptions::default(),
        };

        let result = compile(&request).unwrap();
        let dependencies = result.source_dependencies.unwrap();
        assert_eq!(
            dependencies
                .sources
                .iter()
                .map(|source| source.url.as_str())
                .collect::<Vec<_>>(),
            [
                "fe-memory:///app/src/helper.fe",
                "fe-memory:///app/src/lib.fe"
            ]
        );
    }
}
