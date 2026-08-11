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

pub use codegen::{
    ComponentProjection, PageAttributeKind, PageElement, PageProjection, PageProjectionOp,
    ProjectedPageAttribute, ProjectedPageComponent, ProjectedPageRender,
};

/// In-memory result of projecting a role-selected Fe page. This is a direct
/// typed toolchain API, not a serialized runtime protocol or manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageProjectionResult {
    pub diagnostics: Vec<Diagnostic>,
    pub page: Option<PageProjection>,
    pub source_dependencies: SourceDependencyInventory,
}

/// One resident Wasm compilation and its optional Fe-authored initial DOM
/// fragment, produced from a shared compiler database and diagnostic pass.
/// The view stays typed in memory and is never a runtime manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResidentComponentCompileResult {
    pub compilation: CompileResult,
    pub view: Option<ComponentProjection>,
}

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
/// Dispatches on `request.target`. `Wasm` is always wired. `Webgpu` is wired
/// only when this crate's `webgpu-target` feature is on (the wasm32
/// browser-compiler turns it on; native tools may not); when it is off, and
/// for every other semantic target, `compile` fails explicitly rather than
/// emitting nothing.
pub fn compile(request: &CompileRequest) -> Result<CompileResult, CompileFacadeError> {
    request.validate()?;
    match request.target {
        CompileTarget::Wasm => compile_wasm(request),
        #[cfg(feature = "webgpu-target")]
        CompileTarget::Webgpu => compile_webgpu(request),
        _ => Err(CompileFacadeError::UnsupportedTarget(request.target)),
    }
}

/// CTFE-project one `PageComposition` actor behavior without producing a
/// runtime module. HTML tooling realizes the returned typed operations through
/// its standards parser before normal Fe program discovery.
pub fn project_page(request: &CompileRequest) -> Result<PageProjectionResult, CompileFacadeError> {
    request.validate()?;
    let (db, root_file, diagnostics, has_error, source_dependencies) = compile_prologue(request)?;
    let page = if has_error {
        None
    } else {
        let top_mod = db.top_mod(root_file);
        codegen::project_page(&db, top_mod)
            .map_err(|error| CompileFacadeError::Backend(error.to_string()))?
    };
    Ok(PageProjectionResult {
        diagnostics,
        page,
        source_dependencies,
    })
}

/// Project a page from an already initialized compiler database.
///
/// Native build tooling uses this entry when a page source belongs to a real
/// ingot with local dependencies. The protocol-only [`project_page`] path
/// remains filesystem-free for browser compilers and supplied virtual source
/// graphs; both paths produce the same typed result and structured diagnostics.
pub fn project_page_in_db(
    db: &DriverDataBase,
    root_file: common::file::File,
) -> Result<PageProjectionResult, CompileFacadeError> {
    let top_mod = db.top_mod(root_file);
    let complete = db.run_on_top_mod(top_mod).complete(db);
    let diagnostics = complete
        .iter()
        .map(|diagnostic| protocol_diagnostic(db, diagnostic))
        .collect::<Vec<_>>();
    let has_error = complete
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    let page = if has_error {
        None
    } else {
        codegen::project_page(db, top_mod)
            .map_err(|error| CompileFacadeError::Backend(error.to_string()))?
    };
    Ok(PageProjectionResult {
        diagnostics,
        page,
        source_dependencies: source_dependencies_from_db(db, root_file)?,
    })
}

/// Compile a resident component and CTFE-project its optional
/// `ComponentComposition` behavior without parsing or analyzing the source a
/// second time.
pub fn compile_resident_component(
    request: &CompileRequest,
) -> Result<ResidentComponentCompileResult, CompileFacadeError> {
    request.validate()?;
    if request.target != CompileTarget::Wasm {
        return Err(CompileFacadeError::UnsupportedTarget(request.target));
    }
    compile_wasm_with_component_view(request, true)
}

/// Shared prologue: build the in-memory db from the request's virtual
/// sources, run diagnostics on the root module once, and hand back the
/// pieces both target backends need. `root_file` (not `top_mod`) is returned
/// because `top_mod` borrows `db` for the lifetime of that borrow; callers
/// re-derive it locally with `db.top_mod(root_file)`, which is a cheap
/// re-query, not re-work.
fn compile_prologue(
    request: &CompileRequest,
) -> Result<
    (
        DriverDataBase,
        common::file::File,
        Vec<Diagnostic>,
        bool,
        SourceDependencyInventory,
    ),
    CompileFacadeError,
> {
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
    let has_error = complete
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    Ok((db, root_file, diagnostics, has_error, source_dependencies))
}

fn compile_wasm(request: &CompileRequest) -> Result<CompileResult, CompileFacadeError> {
    Ok(compile_wasm_with_component_view(request, false)?.compilation)
}

fn compile_wasm_with_component_view(
    request: &CompileRequest,
    project_view: bool,
) -> Result<ResidentComponentCompileResult, CompileFacadeError> {
    let (db, root_file, diagnostics, has_error, source_dependencies) = compile_prologue(request)?;
    if has_error {
        return Ok(ResidentComponentCompileResult {
            compilation: result(
                request,
                diagnostics,
                Vec::new(),
                InterfaceManifest::default(),
                source_dependencies,
            ),
            view: None,
        });
    }
    let top_mod = db.top_mod(root_file);
    let view = if project_view {
        codegen::project_component(&db, top_mod)
            .map_err(|error| CompileFacadeError::Backend(error.to_string()))?
    } else {
        None
    };

    // A nominal target-neutral resident actor role selects the fixed actor ABI
    // through the same ordinary Wasm target. This is semantic auto-discovery,
    // not a new JSON request mode, entry-name convention, or web/component
    // compiler special case.
    let optimize = request.options.optimization != fe_compiler_protocol::OptimizationLevel::None;
    let resident = codegen::compile_resident_actor_with_optimization(&db, top_mod, optimize)
        .map_err(|error| CompileFacadeError::Backend(error.to_string()))?;
    let bytes = if let Some(actor) = resident {
        if let Some(view) = &view
            && view.actor != actor.contract.actor
        {
            return Err(CompileFacadeError::Backend(format!(
                "component view actor `{}` does not match resident actor `{}`",
                view.actor, actor.contract.actor
            )));
        }
        actor.wasm
    } else {
        if let Some(view) = &view {
            return Err(CompileFacadeError::Backend(format!(
                "component view actor `{}` has no resident transition",
                view.actor
            )));
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
        output.into_bytecode().ok_or_else(|| {
            CompileFacadeError::Artifact("Wasm backend returned no bytes".to_owned())
        })?
    };
    wasmparser::validate(&bytes)
        .map_err(|error| CompileFacadeError::Artifact(format!("invalid Wasm: {error}")))?;
    let interface = wasm_interface(&bytes)?;
    let artifacts = vec![Artifact::new(
        "module.wasm",
        ArtifactKind::WasmModule,
        "application/wasm",
        bytes,
    )];
    Ok(ResidentComponentCompileResult {
        compilation: result(
            request,
            diagnostics,
            artifacts,
            interface,
            source_dependencies,
        ),
        view,
    })
}

/// `CompileTarget::Webgpu`: lower the requested entry through the render
/// (Fe -> SPIR-V/WGSL) path and emit the WGSL side artifact. There is no
/// wasm import/export table for a shader, so the interface manifest stays
/// the v0 default; a richer WGSL-shaped interface (bind group layout) is a
/// later increment, not a protocol gap.
#[cfg(feature = "webgpu-target")]
fn compile_webgpu(request: &CompileRequest) -> Result<CompileResult, CompileFacadeError> {
    let (db, root_file, diagnostics, has_error, source_dependencies) = compile_prologue(request)?;
    if has_error {
        return Ok(result(
            request,
            diagnostics,
            Vec::new(),
            InterfaceManifest::default(),
            source_dependencies,
        ));
    }
    let entry = request.entries.first().ok_or_else(|| {
        CompileFacadeError::Backend("webgpu target requires at least one entry".to_owned())
    })?;
    let top_mod = db.top_mod(root_file);

    let artifact = codegen::compile_render_wgsl(&db, top_mod, entry)
        .map_err(|error| CompileFacadeError::Backend(error.to_string()))?;
    let wgsl = artifact.wgsl.ok_or_else(|| {
        CompileFacadeError::Artifact("render lowering produced no WGSL".to_owned())
    })?;
    let artifacts = vec![Artifact::new(
        "shader.wgsl",
        ArtifactKind::WgslModule,
        "text/wgsl",
        wgsl.into_bytes(),
    )];
    Ok(result(
        request,
        diagnostics,
        artifacts,
        InterfaceManifest::default(),
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

fn source_dependencies_from_db(
    db: &DriverDataBase,
    root_file: common::file::File,
) -> Result<SourceDependencyInventory, CompileFacadeError> {
    let root = root_file
        .url(db)
        .ok_or_else(|| CompileFacadeError::RootUnavailable("page source has no URL".to_owned()))?
        .to_string();
    let sources = db
        .source_dependency_urls(root_file)
        .into_iter()
        .filter_map(|source_url| {
            let url = Url::parse(&source_url).ok()?;
            let file = db.workspace().get(db, &url)?;
            Some(SourceDependency {
                url: source_url,
                sha256: sha256_hex(file.text(db).as_bytes()),
            })
        })
        .collect();
    Ok(SourceDependencyInventory {
        version: SOURCE_DEPENDENCY_INVENTORY_VERSION,
        root,
        sources,
    })
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
    fn nominal_resident_actor_uses_fixed_stateful_wasm_abi_without_a_new_request_mode() {
        let source = r#"
use core::actor::{InitialState, ProjectState, ResidentTransition}

pub struct Event { pub amount: u32 }
pub struct State { pub count: u32 }
pub struct Patch { pub visible_mask: u32, pub focus_target: u32, pub flags: u32 }

actor App {
    count: u32,

    const fn initial() -> State uses (InitialState) {
        State { count: 3 }
    }

    fn project(self) -> Patch uses (ProjectState) {
        Patch { visible_mask: self.count, focus_target: 0, flags: 0 }
    }

    fn update(self, event: own Event) -> State uses (ResidentTransition) {
        State { count: self.count + event.amount }
    }
}
"#;
        let mut request = request(source);
        request.entries = vec![codegen::RESIDENT_ACTOR_INITIALIZE_EXPORT.to_owned()];
        let result = compile(&request).expect("resident actor facade compilation");
        result.validate().expect("resident actor compile result");
        assert!(result.diagnostics.is_empty());
        let wasm = result
            .artifacts
            .iter()
            .find(|artifact| artifact.kind == ArtifactKind::WasmModule)
            .expect("resident actor Wasm");
        let exports = result
            .interface
            .exports
            .iter()
            .map(|export| export.name.as_str())
            .collect::<Vec<_>>();
        for fixed in [
            codegen::RESIDENT_ACTOR_INITIALIZE_EXPORT,
            codegen::RESIDENT_ACTOR_TRANSITION_EXPORT,
            codegen::RESIDENT_ACTOR_PROJECT_EXPORT,
        ] {
            assert!(exports.contains(&fixed), "missing {fixed}: {exports:?}");
        }
        assert!(
            !exports
                .iter()
                .any(|name| matches!(*name, "initial" | "update" | "project")),
            "authored behavior names leaked into host discovery: {exports:?}"
        );

        let engine = wasmtime::Engine::default();
        let module = wasmtime::Module::new(&engine, &wasm.bytes).expect("resident facade Wasm");
        let mut store = wasmtime::Store::new(&engine, ());
        let instance =
            wasmtime::Instance::new(&mut store, &module, &[]).expect("resident instance");
        let initialize = instance
            .get_typed_func::<(), i32>(&mut store, codegen::RESIDENT_ACTOR_INITIALIZE_EXPORT)
            .expect("initializer");
        let update = instance
            .get_typed_func::<i32, i32>(&mut store, codegen::RESIDENT_ACTOR_TRANSITION_EXPORT)
            .expect("transition");
        let project = instance
            .get_typed_func::<(), (i32, i32, i32)>(
                &mut store,
                codegen::RESIDENT_ACTOR_PROJECT_EXPORT,
            )
            .expect("projection");
        assert_eq!(initialize.call(&mut store, ()).unwrap(), 3);
        assert_eq!(update.call(&mut store, 4).unwrap(), 7);
        assert_eq!(project.call(&mut store, ()).unwrap(), (7, 0, 0));
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
    #[cfg(not(feature = "webgpu-target"))]
    fn webgpu_target_fails_closed_without_feature() {
        let mut request = request("pub fn main() -> u32 { 42 }");
        request.target = CompileTarget::Webgpu;
        assert!(matches!(
            compile(&request),
            Err(CompileFacadeError::UnsupportedTarget(CompileTarget::Webgpu))
        ));
    }

    #[test]
    #[cfg(feature = "webgpu-target")]
    fn compiles_render_entry_to_naga_valid_wgsl() {
        let mut request = request(
            r#"
pub fn shade(x: u32, y: u32) -> u32 {
    4278190080 + x * 65536 + y * 256
}
"#,
        );
        request.target = CompileTarget::Webgpu;
        request.entries = vec!["shade".to_owned()];
        let result = compile(&request).unwrap();
        result.validate().unwrap();
        assert!(result.diagnostics.is_empty());
        let wgsl_artifact = result
            .artifacts
            .iter()
            .find(|artifact| artifact.kind == ArtifactKind::WgslModule)
            .unwrap();
        assert_eq!(wgsl_artifact.name, "shader.wgsl");
        assert!(!wgsl_artifact.bytes.is_empty());
        let wgsl = std::str::from_utf8(&wgsl_artifact.bytes).unwrap();
        naga::front::wgsl::parse_str(wgsl).unwrap();
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
