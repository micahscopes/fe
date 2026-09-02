use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::{Instant, UNIX_EPOCH},
};

use camino::{Utf8Path, Utf8PathBuf};
use codegen::{
    WebAuthoredSourceKind, WebBuildOptions, WebBundle, WebBundleMode, WebSourceProvenance,
    resolve_web_entry,
};
use common::InputDb;
use driver::{
    DriverDataBase,
    cli_target::{CliTarget, resolve_cli_target},
};
use fe_compiler_facade::{PageProjectionResult, ResidentComponentCompileResult};
use fe_compiler_protocol::{
    SOURCE_DEPENDENCY_INVENTORY_VERSION, SourceDependency, SourceDependencyInventory, sha256_hex,
};
use fe_html_precompile::{RenderBundleArtifact, RenderShaderArtifact, RenderSupportArtifact};
use hir::hir_def::HirIngot;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{WebCanonicalPolicy, WebMode, dependency_diagnostics::DependencyIssues};

#[derive(Debug, Clone)]
pub struct CompileRequest {
    pub path: Utf8PathBuf,
    /// Explicit terminal entry, or `None` to derive it from the module's
    /// `actor` declaration (its render surface or final compute behavior).
    pub entry: Option<String>,
    /// Explicit mode, or `None` to derive it from the `actor` declaration.
    pub mode: Option<WebMode>,
    pub workgroup: [Option<u32>; 3],
    pub source_id: Option<String>,
    pub canonical: WebCanonicalPolicy,
    pub canonical_entries: Vec<String>,
}

fn to_bundle_mode(mode: WebMode) -> WebBundleMode {
    match mode {
        WebMode::Render => WebBundleMode::Render,
        WebMode::Grid => WebBundleMode::Grid,
        WebMode::Compute => WebBundleMode::Compute,
    }
}

fn from_bundle_mode(mode: WebBundleMode) -> WebMode {
    match mode {
        WebBundleMode::Render => WebMode::Render,
        WebBundleMode::Grid => WebMode::Grid,
        WebBundleMode::Compute => WebMode::Compute,
    }
}

/// Reconcile the resolved mode against the `--workgroup-*` flags. Render takes no
/// workgroup; grid requires all three non-zero.
fn validate_workgroup(
    mode: WebMode,
    workgroup: [Option<u32>; 3],
) -> Result<Option<[u32; 3]>, String> {
    match (mode, workgroup) {
        (WebMode::Render, [None, None, None]) => Ok(None),
        (WebMode::Render, _) => {
            Err("workgroup flags are only valid with `--mode grid`".to_string())
        }
        (WebMode::Compute, [None, None, None]) => Ok(None),
        (WebMode::Compute, _) => Err(
            "compute actor workgroup and dispatch geometry are authored in Fe; command-line workgroup flags are not accepted"
                .to_string(),
        ),
        (WebMode::Grid, [Some(x), Some(y), Some(z)]) if x > 0 && y > 0 && z > 0 => {
            Ok(Some([x, y, z]))
        }
        (WebMode::Grid, _) => Err(
            "grid mode requires non-zero `--workgroup-x`, `--workgroup-y`, and `--workgroup-z`"
                .to_string(),
        ),
    }
}

pub fn build(request: &CompileRequest, out: &Utf8PathBuf) -> Result<(), String> {
    let bundle = compile(request)?;
    bundle
        .write_atomic(out.as_std_path())
        .map_err(|error| error.to_string())?;
    println!("wrote web bundle: {out}");
    Ok(())
}

fn load_resource_assets(root: &Utf8Path) -> Result<Vec<Vec<u8>>, String> {
    let directory = root.join("assets/sha256");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    if !directory.is_dir() {
        return Err(format!(
            "content-addressed resource path `{directory}` is not a directory"
        ));
    }
    let mut entries = std::fs::read_dir(directory.as_std_path())
        .map_err(|error| format!("failed to read resource assets `{directory}`: {error}"))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("failed to enumerate resource assets `{directory}`: {error}"))?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut assets = Vec::with_capacity(entries.len());
    for entry in entries {
        let path = entry.path();
        if !entry
            .file_type()
            .map_err(|error| format!("failed to inspect `{}`: {error}", path.display()))?
            .is_file()
        {
            return Err(format!(
                "resource asset directory contains non-file `{}`",
                path.display()
            ));
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return Err(format!(
                "resource asset path is not valid UTF-8: `{}`",
                path.display()
            ));
        };
        let Some(digest) = name.strip_suffix(".bin") else {
            return Err(format!(
                "resource asset `{}` must be named <sha256>.bin",
                path.display()
            ));
        };
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(format!(
                "resource asset `{}` does not carry a lowercase SHA-256 filename",
                path.display()
            ));
        }
        let bytes = std::fs::read(&path).map_err(|error| {
            format!(
                "failed to read resource asset `{}`: {error}",
                path.display()
            )
        })?;
        let actual = sha256_hex(&bytes);
        if actual != digest {
            return Err(format!(
                "resource asset `{}` hashes to {actual}, not its filename {digest}",
                path.display()
            ));
        }
        assets.push(bytes);
    }
    Ok(assets)
}

pub fn compile(request: &CompileRequest) -> Result<WebBundle, String> {
    let compile_started = Instant::now();
    let CompileRequest {
        path,
        entry,
        mode,
        workgroup,
        source_id,
        canonical,
        canonical_entries,
    } = request;
    tracing::info!(
        target: "fe_web",
        phase = "compile",
        source = %path,
        entry = entry.as_deref().unwrap_or("<derived>"),
        "starting web compilation"
    );
    if matches!(entry.as_deref(), Some("")) {
        return Err("`--entry` must not be empty".to_string());
    }
    match (*canonical, canonical_entries.is_empty()) {
        (WebCanonicalPolicy::Disabled, false) => {
            return Err(
                "`--canonical-entry` is only valid with `--canonical optional|required`"
                    .to_string(),
            );
        }
        (_, _) if canonical_entries.iter().any(String::is_empty) => {
            return Err("`--canonical-entry` must not be empty".to_string());
        }
        _ => {}
    }
    // When the mode is given explicitly, reconcile it with the workgroup flags
    // before any I/O; a derived mode is re-checked after the actor is resolved.
    if let Some(mode) = mode {
        validate_workgroup(*mode, *workgroup)?;
    }

    let mut db = DriverDataBase::default();
    let phase_started = Instant::now();
    let target = resolve_cli_target(&mut db, path, false)?;
    let (top_mod, ingot_target, resource_root) = match target {
        CliTarget::StandaloneFile(file_path) => {
            let canonical = file_path
                .canonicalize_utf8()
                .map_err(|error| format!("cannot canonicalize `{file_path}`: {error}"))?;
            let url = Url::from_file_path(canonical.as_std_path())
                .map_err(|_| format!("invalid source path `{file_path}`"))?;
            let source = std::fs::read_to_string(file_path.as_std_path())
                .map_err(|error| format!("failed to read `{file_path}`: {error}"))?;
            db.workspace().touch(&mut db, url.clone(), Some(source));
            let file = db
                .workspace()
                .get(&db, &url)
                .ok_or_else(|| format!("failed to load `{file_path}`"))?;
            let resource_root = canonical
                .parent()
                .map(Utf8Path::to_owned)
                .ok_or_else(|| format!("source path `{canonical}` has no parent directory"))?;
            (db.top_mod(file), None, resource_root)
        }
        CliTarget::Directory(dir_path) => {
            let canonical = dir_path
                .canonicalize_utf8()
                .map_err(|error| format!("cannot canonicalize `{dir_path}`: {error}"))?;
            let url = Url::from_directory_path(canonical.as_std_path())
                .map_err(|_| format!("invalid ingot path `{dir_path}`"))?;
            if driver::init_ingot(&mut db, &url) {
                return Err(format!("failed to initialize ingot `{dir_path}`"));
            }
            let ingot = db
                .workspace()
                .containing_ingot(&db, url.clone())
                .ok_or_else(|| {
                    format!(
                        "`{dir_path}` did not resolve to one ingot; target an ingot directory explicitly"
                    )
                })?;
            (ingot.root_mod(&db), Some((url, ingot)), canonical)
        }
    };
    tracing::info!(
        target: "fe_web",
        phase = "target",
        source = %path,
        elapsed_ms = phase_started.elapsed().as_millis() as u64,
        "resolved compilation target"
    );

    let phase_started = Instant::now();
    let diagnostics = match ingot_target {
        Some((_, ingot)) => db.run_on_ingot(ingot),
        None => db.run_on_top_mod(top_mod),
    };
    if !diagnostics.is_empty() {
        tracing::warn!(
            target: "fe_web",
            phase = "diagnostics",
            source = %path,
            elapsed_ms = phase_started.elapsed().as_millis() as u64,
            "source diagnostics prevent web build"
        );
        return Err(format!(
            "source diagnostics prevent web build:\n{}",
            diagnostics.format_diags(&db)
        ));
    }
    tracing::info!(
        target: "fe_web",
        phase = "diagnostics",
        source = %path,
        count = 0,
        elapsed_ms = phase_started.elapsed().as_millis() as u64,
        "source diagnostics clean"
    );
    if let Some((ingot_url, _)) = ingot_target {
        let phase_started = Instant::now();
        let mut seen = HashSet::from([ingot_url.clone()]);
        let dependency_issues = DependencyIssues::collect(&db, &ingot_url, &mut seen);
        let dependency_stats = dependency_issues.stats();
        if !dependency_issues.is_empty() {
            tracing::warn!(
                target: "fe_web",
                phase = "dependency_diagnostics",
                source = %path,
                analyzed = dependency_stats.analyzed,
                reused = dependency_stats.reused,
                elapsed_ms = phase_started.elapsed().as_millis() as u64,
                "dependency diagnostics prevent web build"
            );
            return Err(format!(
                "dependency diagnostics prevent web build:\n{}",
                dependency_issues.format(&db)
            ));
        }
        tracing::info!(
            target: "fe_web",
            phase = "dependency_diagnostics",
            source = %path,
            ingots = seen.len(),
            analyzed = dependency_stats.analyzed,
            reused = dependency_stats.reused,
            elapsed_ms = phase_started.elapsed().as_millis() as u64,
            "dependency diagnostics clean"
        );
    }
    // Derive the terminal entry and mode from the module's `actor` declaration when
    // not given explicitly; when supplied, they are reconciled against the
    // declaration (a mismatch errors, naming both sources).
    let phase_started = Instant::now();
    let (entry, mode) =
        resolve_web_entry(&db, top_mod, (*entry).clone(), (*mode).map(to_bundle_mode))
            .map_err(|error| error.to_string())?;
    let mode = from_bundle_mode(mode);
    let workgroup = validate_workgroup(mode, *workgroup)?;

    let mut options = match mode {
        WebMode::Render => WebBuildOptions::render(&entry, source_id.clone()),
        WebMode::Grid => WebBuildOptions::grid(&entry, workgroup.unwrap(), source_id.clone()),
        WebMode::Compute => WebBuildOptions::compute(&entry, source_id.clone()),
    }
    .with_canonical_policy(match canonical {
        WebCanonicalPolicy::Disabled => codegen::WebCanonicalPolicy::Disabled,
        WebCanonicalPolicy::Optional => codegen::WebCanonicalPolicy::Optional,
        WebCanonicalPolicy::Required => codegen::WebCanonicalPolicy::Required,
    });
    options = options.with_canonical_entries(canonical_entries.iter().cloned());
    for asset in load_resource_assets(&resource_root)? {
        options = options.with_resource_asset(asset);
    }
    tracing::info!(
        target: "fe_web",
        phase = "entry",
        source = %path,
        entry = %entry,
        mode = ?mode,
        elapsed_ms = phase_started.elapsed().as_millis() as u64,
        "resolved web entry"
    );
    let phase_started = Instant::now();
    let bundle = WebBundle::compile(&db, top_mod, options).map_err(|error| error.to_string())?;
    tracing::info!(
        target: "fe_web",
        phase = "lowering",
        source = %path,
        passes = bundle.pass_wgsl.len(),
        wasm_bytes = bundle.wasm.len(),
        wgsl_bytes = bundle.wgsl.len()
            + bundle.pass_wgsl.iter().map(|shader| shader.source.len()).sum::<usize>(),
        elapsed_ms = phase_started.elapsed().as_millis() as u64,
        total_elapsed_ms = compile_started.elapsed().as_millis() as u64,
        "finished web compilation"
    );
    Ok(bundle)
}

const RENDER_CACHE_FORMAT: u16 = 4;

#[derive(Debug, Serialize, Deserialize)]
struct RenderCacheMetadata {
    format: u16,
    source_dependencies: SourceDependencyInventory,
    has_wasm: bool,
    pass_shaders: Vec<CachedRenderShader>,
    support_files: Vec<CachedRenderSupport>,
    resource_files: Vec<CachedRenderSupport>,
    scoped_task_files: Vec<CachedRenderSupport>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedRenderShader {
    path: String,
    file: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedRenderSupport {
    path: String,
    file: String,
}

fn render_cache_root() -> Option<PathBuf> {
    if std::env::var("FE_WEB_CACHE")
        .is_ok_and(|value| matches!(value.as_str(), "0" | "false" | "off"))
    {
        return None;
    }
    Some(
        std::env::var_os("FE_WEB_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/fe-web-cache")),
    )
}

fn executable_cache_identity(path: &Path) -> String {
    std::fs::read(path)
        .map(|bytes| format!("sha256:{}", sha256_hex(&bytes)))
        .unwrap_or_else(|_| {
            std::fs::metadata(path)
                .map(|metadata| {
                    let modified = metadata
                        .modified()
                        .ok()
                        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                        .map(|duration| duration.as_nanos())
                        .unwrap_or_default();
                    format!("metadata:{}:{modified}", metadata.len())
                })
                .unwrap_or_else(|_| "no-executable-identity".to_owned())
        })
}

fn compiler_cache_identity() -> &'static str {
    static IDENTITY: OnceLock<String> = OnceLock::new();
    IDENTITY
        .get_or_init(|| {
            let git = option_env!("FE_GIT_HASH").unwrap_or("unknown");
            let executable = std::env::current_exe()
                .ok()
                .map(|path| executable_cache_identity(&path))
                .unwrap_or_else(|| "no-executable-identity".to_owned());
            format!(
                "render-cache-v{RENDER_CACHE_FORMAT}:{}:{git}:{executable}",
                env!("CARGO_PKG_VERSION")
            )
        })
        .as_str()
}

fn render_cache_key(
    dependencies: &SourceDependencyInventory,
    non_fe_authored_sources: &[WebSourceProvenance],
    entry: Option<&str>,
) -> Option<String> {
    serde_json::to_vec(&(
        compiler_cache_identity(),
        entry.unwrap_or_default(),
        dependencies,
        non_fe_authored_sources,
    ))
    .ok()
    .map(|bytes| sha256_hex(&bytes))
}

fn load_render_cache(
    root: &Path,
    key: &str,
    dependencies: &SourceDependencyInventory,
) -> Option<RenderBundleArtifact> {
    let directory = root.join(key);
    let metadata: RenderCacheMetadata =
        serde_json::from_slice(&std::fs::read(directory.join("metadata.json")).ok()?).ok()?;
    if metadata.format != RENDER_CACHE_FORMAT || metadata.source_dependencies != *dependencies {
        return None;
    }
    let pass_wgsl = metadata
        .pass_shaders
        .into_iter()
        .map(|shader| {
            Some(RenderShaderArtifact {
                path: shader.path,
                bytes: std::fs::read(directory.join(shader.file)).ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let wasm = if metadata.has_wasm {
        Some(std::fs::read(directory.join("module.wasm")).ok()?)
    } else {
        None
    };
    let support_files = metadata
        .support_files
        .into_iter()
        .map(|support| {
            Some(RenderSupportArtifact {
                path: support.path,
                bytes: std::fs::read(directory.join(support.file)).ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let resource_files = metadata
        .resource_files
        .into_iter()
        .map(|support| {
            Some(RenderSupportArtifact {
                path: support.path,
                bytes: std::fs::read(directory.join(support.file)).ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let scoped_task_files = metadata
        .scoped_task_files
        .into_iter()
        .map(|support| {
            Some(RenderSupportArtifact {
                path: support.path,
                bytes: std::fs::read(directory.join(support.file)).ok()?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(RenderBundleArtifact {
        wasm,
        wgsl: std::fs::read(directory.join("shader.wgsl")).ok()?,
        pass_wgsl,
        support_files,
        resource_files,
        scoped_task_files,
        manifest_json: std::fs::read(directory.join("manifest.json")).ok()?,
        source_dependencies: Some(metadata.source_dependencies),
    })
}

fn store_render_cache(
    root: &Path,
    key: &str,
    artifact: &RenderBundleArtifact,
) -> Result<(), String> {
    let Some(source_dependencies) = artifact.source_dependencies.clone() else {
        return Ok(());
    };
    let directory = root.join(key);
    std::fs::create_dir_all(&directory).map_err(|error| {
        format!(
            "failed to create render cache {}: {error}",
            directory.display()
        )
    })?;
    std::fs::write(directory.join("shader.wgsl"), &artifact.wgsl)
        .map_err(|error| format!("failed to cache shader.wgsl: {error}"))?;
    std::fs::write(directory.join("manifest.json"), &artifact.manifest_json)
        .map_err(|error| format!("failed to cache manifest.json: {error}"))?;
    if let Some(wasm) = &artifact.wasm {
        std::fs::write(directory.join("module.wasm"), wasm)
            .map_err(|error| format!("failed to cache module.wasm: {error}"))?;
    }
    let mut pass_shaders = Vec::with_capacity(artifact.pass_wgsl.len());
    for (index, shader) in artifact.pass_wgsl.iter().enumerate() {
        let file = format!("pass-{index}.wgsl");
        std::fs::write(directory.join(&file), &shader.bytes)
            .map_err(|error| format!("failed to cache {file}: {error}"))?;
        pass_shaders.push(CachedRenderShader {
            path: shader.path.clone(),
            file,
        });
    }
    let mut support_files = Vec::with_capacity(artifact.support_files.len());
    for (index, support) in artifact.support_files.iter().enumerate() {
        let file = format!("support-{index}");
        std::fs::write(directory.join(&file), &support.bytes)
            .map_err(|error| format!("failed to cache {file}: {error}"))?;
        support_files.push(CachedRenderSupport {
            path: support.path.clone(),
            file,
        });
    }
    let mut resource_files = Vec::with_capacity(artifact.resource_files.len());
    for (index, support) in artifact.resource_files.iter().enumerate() {
        let file = format!("resource-{index}");
        std::fs::write(directory.join(&file), &support.bytes)
            .map_err(|error| format!("failed to cache {file}: {error}"))?;
        resource_files.push(CachedRenderSupport {
            path: support.path.clone(),
            file,
        });
    }
    let mut scoped_task_files = Vec::with_capacity(artifact.scoped_task_files.len());
    for (index, support) in artifact.scoped_task_files.iter().enumerate() {
        let file = format!("task-support-{index}");
        std::fs::write(directory.join(&file), &support.bytes)
            .map_err(|error| format!("failed to cache {file}: {error}"))?;
        scoped_task_files.push(CachedRenderSupport {
            path: support.path.clone(),
            file,
        });
    }
    let metadata = serde_json::to_vec_pretty(&RenderCacheMetadata {
        format: RENDER_CACHE_FORMAT,
        source_dependencies,
        has_wasm: artifact.wasm.is_some(),
        pass_shaders,
        support_files,
        resource_files,
        scoped_task_files,
    })
    .map_err(|error| format!("failed to serialize render cache metadata: {error}"))?;
    // Metadata is written last. An interrupted population is therefore a
    // harmless miss on the next invocation, never a partially accepted hit.
    std::fs::write(directory.join("metadata.json"), metadata)
        .map_err(|error| format!("failed to cache metadata.json: {error}"))
}

fn compile_render_bundle_with_dependencies(
    path: &Utf8PathBuf,
    entry: Option<&str>,
    source_audit: Option<IngotSourceAudit>,
) -> Result<RenderBundleArtifact, String> {
    let mut bundle = compile(&CompileRequest {
        path: path.clone(),
        entry: entry.map(str::to_owned),
        mode: None,
        workgroup: [None, None, None],
        source_id: None,
        canonical: WebCanonicalPolicy::Disabled,
        canonical_entries: Vec::new(),
    })?;
    let dependencies = source_audit.map(|audit| {
        bundle.manifest.provenance.source_id = Some(audit.source_id);
        bundle.manifest.provenance.authored_sources = audit.authored_sources;
        bundle.manifest.provenance.non_fe_authored_sources = audit.non_fe_authored_sources;
        audit.dependencies
    });
    let manifest_json = bundle.manifest_json().map_err(|error| error.to_string())?;
    let materialized_files = bundle
        .materialized_files()
        .map_err(|error| error.to_string())?;
    let support_files = materialized_files
        .iter()
        .filter(|file| {
            file.path() == "interface.js"
                || file.path() == "interface.d.ts"
                || file.path().starts_with("runtime/")
        })
        .map(|file| RenderSupportArtifact {
            path: file.path().to_owned(),
            bytes: file.bytes().to_vec(),
        })
        .collect::<Vec<_>>();
    let resource_files = materialized_files
        .iter()
        .filter(|file| file.path().starts_with("resources/"))
        .map(|file| RenderSupportArtifact {
            path: file.path().to_owned(),
            bytes: file.bytes().to_vec(),
        })
        .collect::<Vec<_>>();
    let scoped_task_files = materialized_files
        .iter()
        .filter_map(|file| {
            file.path()
                .strip_prefix("tasks/")
                .map(|path| RenderSupportArtifact {
                    path: path.to_owned(),
                    bytes: file.bytes().to_vec(),
                })
        })
        .collect::<Vec<_>>();
    let wasm = (!bundle.wasm.is_empty()).then_some(bundle.wasm);
    let pass_wgsl = bundle
        .pass_wgsl
        .into_iter()
        .map(|shader| RenderShaderArtifact {
            path: shader.path,
            bytes: shader.source.into_bytes(),
        })
        .collect();
    Ok(RenderBundleArtifact {
        wasm,
        wgsl: bundle.wgsl.into_bytes(),
        pass_wgsl,
        support_files,
        resource_files,
        scoped_task_files,
        manifest_json,
        source_dependencies: dependencies,
    })
}

/// The `render_compile` closure `fe web dev`/`fe web precompile` hand to
/// `fe_html_precompile`'s render lane
/// (`precompile_html_with_render_lane`/`DevelopmentPrecompiler::build_with_render_lane`):
/// a `data-fe-src` that resolves to a filesystem DIRECTORY is an ingot,
/// compiled through [`compile_render_bundle`] above; anything else (a single
/// `.fe` file, a non-file URL, a path that does not exist) returns `Ok(None)`
/// and falls through to the unchanged wasm-only facade lane. This is what
/// keeps `data-fe-src="sketches/cga3d"` from ever reaching a plain
/// `fs::read_to_string`, which cannot read a directory.
pub fn render_compile(
    url: &Url,
    entry: Option<&str>,
) -> Result<Option<RenderBundleArtifact>, String> {
    let Ok(path) = url.to_file_path() else {
        return Ok(None);
    };
    let Ok(path) = Utf8PathBuf::from_path_buf(path) else {
        return Ok(None);
    };
    if !path.is_dir() {
        return Ok(None);
    }
    let source_audit = ingot_source_audit(&path);
    let dependencies = source_audit.as_ref().map(|audit| &audit.dependencies);
    let non_fe_authored_sources = source_audit
        .as_ref()
        .map(|audit| audit.non_fe_authored_sources.as_slice())
        .unwrap_or_default();
    let cache = render_cache_root().and_then(|root| {
        dependencies
            .and_then(|dependencies| render_cache_key(dependencies, non_fe_authored_sources, entry))
            .map(|key| (root, key))
    });
    if let (Some((root, key)), Some(dependencies)) = (&cache, dependencies)
        && let Some(artifact) = load_render_cache(root, key, dependencies)
    {
        tracing::info!(
            target: "fe_web",
            phase = "render_bundle",
            cache = "hit",
            ingot = %path,
            entry = entry.unwrap_or("<derived>"),
            "reused compiled render bundle"
        );
        return Ok(Some(artifact));
    }

    let started = Instant::now();
    tracing::info!(
        target: "fe_web",
        phase = "render_bundle",
        cache = if cache.is_some() { "miss" } else { "disabled" },
        ingot = %path,
        entry = entry.unwrap_or("<derived>"),
        "compiling render bundle"
    );
    let artifact = compile_render_bundle_with_dependencies(&path, entry, source_audit)?;
    let emitted_bytes = artifact.wgsl.len()
        + artifact.manifest_json.len()
        + artifact.wasm.as_ref().map_or(0, Vec::len)
        + artifact
            .pass_wgsl
            .iter()
            .map(|shader| shader.bytes.len())
            .sum::<usize>();
    tracing::info!(
        target: "fe_web",
        phase = "render_bundle",
        cache = "populated",
        ingot = %path,
        elapsed_ms = started.elapsed().as_millis() as u64,
        emitted_bytes,
        "compiled render bundle"
    );
    if let Some((root, key)) = cache
        && let Err(error) = store_render_cache(&root, &key, &artifact)
    {
        tracing::warn!(
            target: "fe_web",
            phase = "render_cache",
            ingot = %path,
            %error,
            "could not populate render cache"
        );
    }
    Ok(Some(artifact))
}

/// Project an external page source through its real initialized ingot when one
/// exists. This is the native counterpart to the protocol-only page facade:
/// local dependencies are resolved by the ordinary Fe workspace machinery,
/// while standalone/virtual sources return `Ok(None)` and retain the portable
/// single-source path.
pub fn page_compile(url: &Url) -> Result<Option<PageProjectionResult>, String> {
    let Some((db, root_file, _)) = initialized_source_ingot(url, "page")? else {
        return Ok(None);
    };
    fe_compiler_facade::project_page_in_db(&db, root_file)
        .map(Some)
        .map_err(|error| error.to_string())
}

/// Compile an external resident component through its real initialized ingot.
/// This is the component counterpart to [`page_compile`]: local Fe dependencies
/// retain their ordinary identities, while standalone and virtual sources keep
/// using the portable supplied-source facade.
pub fn component_compile(url: &Url) -> Result<Option<ResidentComponentCompileResult>, String> {
    let Some((db, root_file, ingot_dir)) = initialized_source_ingot(url, "component")? else {
        return Ok(None);
    };
    let needs_initialized_ingot = component_ingot_needs_initialized_db(&ingot_dir);
    if !needs_initialized_ingot {
        return Ok(None);
    }
    fe_compiler_facade::compile_resident_component_in_db(&db, root_file)
        .map(Some)
        .map_err(|error| error.to_string())
}

fn component_ingot_needs_initialized_db(ingot_dir: &Utf8Path) -> bool {
    let has_explicit_dependency = std::fs::read_to_string(ingot_dir.join("fe.toml"))
        .ok()
        .and_then(|content| common::config::Config::parse(&content).ok())
        .is_some_and(|config| match config {
            common::config::Config::Ingot(config) => !config.dependency_entries.is_empty(),
            common::config::Config::Workspace(_) => false,
        });
    has_explicit_dependency
        || walkdir::WalkDir::new(ingot_dir.as_std_path())
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_type().is_file()
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "fe")
            })
            .take(2)
            .count()
            > 1
}

fn initialized_source_ingot(
    url: &Url,
    purpose: &str,
) -> Result<Option<(DriverDataBase, common::file::File, Utf8PathBuf)>, String> {
    let Ok(path) = url.to_file_path() else {
        return Ok(None);
    };
    let Ok(path) = Utf8PathBuf::from_path_buf(path) else {
        return Ok(None);
    };
    if !path.is_file() {
        return Ok(None);
    }
    let canonical = path
        .canonicalize_utf8()
        .map_err(|error| format!("cannot canonicalize {purpose} source `{path}`: {error}"))?;
    let Some(ingot_dir) = canonical
        .ancestors()
        .skip(1)
        .find(|directory| directory.join("fe.toml").is_file())
        .map(Utf8Path::to_path_buf)
    else {
        return Ok(None);
    };
    let ingot_url = Url::from_directory_path(ingot_dir.as_std_path())
        .map_err(|_| format!("invalid {purpose} ingot path `{ingot_dir}`"))?;
    let source_url = Url::from_file_path(canonical.as_std_path())
        .map_err(|_| format!("invalid {purpose} source path `{canonical}`"))?;
    let mut db = DriverDataBase::default();
    if driver::init_ingot(&mut db, &ingot_url) {
        return Err(format!(
            "failed to initialize {purpose} ingot `{ingot_dir}`"
        ));
    }
    let ingot = db
        .workspace()
        .containing_ingot(&db, ingot_url.clone())
        .ok_or_else(|| format!("{purpose} source `{canonical}` is not in an initialized ingot"))?;
    let mut seen = HashSet::from([ingot_url]);
    let dependency_issues = DependencyIssues::collect(&db, &ingot.base(&db), &mut seen);
    if !dependency_issues.is_empty() {
        return Err(format!(
            "dependency diagnostics prevent {purpose} compilation:\n{}",
            dependency_issues.format(&db)
        ));
    }
    drop(dependency_issues);
    let root_file = db
        .workspace()
        .get(&db, &source_url)
        .ok_or_else(|| format!("{purpose} source `{canonical}` was not loaded by its ingot"))?;
    Ok(Some((db, root_file, ingot_dir)))
}

#[derive(Debug, Clone)]
struct IngotSourceAudit {
    source_id: String,
    dependencies: SourceDependencyInventory,
    authored_sources: Vec<WebSourceProvenance>,
    non_fe_authored_sources: Vec<WebSourceProvenance>,
}

/// The structural dependency inventory used for rebuilds plus the ownership
/// ledger published in a render manifest. Unlike the watch graph, the ledger
/// also records non-Fe files under the root ingot and its local dependencies so
/// the canonical-gallery policy can reject application JS/Rust/WGSL/Wasm
/// without relying on a filename search outside the build.
fn ingot_source_audit(ingot_dir: &Utf8PathBuf) -> Option<IngotSourceAudit> {
    let root_dir = ingot_dir.canonicalize_utf8().ok()?;
    let mut visited = BTreeSet::new();
    let mut sources = BTreeMap::new();
    let mut non_fe_sources = BTreeMap::new();
    collect_ingot_sources(&root_dir, &mut visited, &mut sources, &mut non_fe_sources);
    let root = sources.keys().next()?.clone();
    let inventory = SourceDependencyInventory {
        version: SOURCE_DEPENDENCY_INVENTORY_VERSION,
        root,
        sources: sources
            .into_iter()
            .map(|(url, sha256)| SourceDependency { url, sha256 })
            .collect(),
    };
    inventory.validate().ok()?;
    let logical_base = provenance_logical_base(&root_dir, &inventory.sources);
    let authored_sources = inventory
        .sources
        .iter()
        .filter_map(|source| source_provenance(&logical_base, &source.url, &source.sha256, None))
        .collect();
    let non_fe_authored_sources = non_fe_sources
        .into_iter()
        .filter_map(|(url, sha256)| source_provenance(&logical_base, &url, &sha256, None))
        .collect();
    Some(IngotSourceAudit {
        source_id: root_dir.file_name().unwrap_or(root_dir.as_str()).to_owned(),
        dependencies: inventory,
        authored_sources,
        non_fe_authored_sources,
    })
}

/// Choose a stable namespace that contains both the root sketch and every
/// recursively discovered local Fe dependency. Starting at the sketch's
/// parent preserves compact `sketch/file.fe` identities for the common case;
/// shared ingots outside that directory widen the base to their common
/// ancestor (normally the repository root), never to an ambient absolute id.
fn provenance_logical_base(root_dir: &Utf8Path, sources: &[SourceDependency]) -> Utf8PathBuf {
    let mut base = root_dir.parent().unwrap_or(root_dir).to_owned();
    for source in sources {
        let Some(path) = Url::parse(&source.url)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .and_then(|path| Utf8PathBuf::from_path_buf(path).ok())
        else {
            continue;
        };
        while !path.starts_with(&base) {
            let Some(parent) = base.parent() else {
                break;
            };
            base = parent.to_owned();
        }
    }
    base
}

fn collect_ingot_sources(
    dir: &Utf8PathBuf,
    visited: &mut BTreeSet<Utf8PathBuf>,
    sources: &mut BTreeMap<String, String>,
    non_fe_sources: &mut BTreeMap<String, String>,
) {
    let Ok(canonical) = dir.canonicalize_utf8() else {
        return;
    };
    if !visited.insert(canonical.clone()) {
        return;
    }
    for entry in walkdir::WalkDir::new(canonical.as_std_path())
        .into_iter()
        .filter_entry(|entry| {
            entry.depth() == 0
                || !matches!(
                    entry.file_name().to_str(),
                    Some(".git" | "target" | "node_modules")
                )
        })
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let is_source = entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "fe")
            || entry.file_name() == "fe.toml";
        let (Ok(path), Ok(bytes)) = (
            Utf8PathBuf::from_path_buf(entry.path().to_path_buf()),
            std::fs::read(entry.path()),
        ) else {
            continue;
        };
        if let Ok(url) = Url::from_file_path(path.as_std_path()) {
            let target = if is_source {
                &mut *sources
            } else {
                &mut *non_fe_sources
            };
            target.insert(url.to_string(), sha256_hex(&bytes));
        }
    }

    let Ok(content) = std::fs::read_to_string(canonical.join("fe.toml")) else {
        return;
    };
    let Ok(common::config::Config::Ingot(ingot_config)) = common::config::Config::parse(&content)
    else {
        return;
    };
    let Ok(base_url) = Url::from_directory_path(canonical.as_str()) else {
        return;
    };
    let (dependencies, _diagnostics) = ingot_config.dependencies(&base_url);
    for dependency in dependencies {
        if let common::dependencies::DependencyLocation::Local(local) = &dependency.location
            && let Ok(dependency_path) = local.url.to_file_path()
            && let Ok(dependency_path) = Utf8PathBuf::from_path_buf(dependency_path)
        {
            collect_ingot_sources(&dependency_path, visited, sources, non_fe_sources);
        }
    }
}

fn source_provenance(
    logical_base: &Utf8Path,
    url: &str,
    sha256: &str,
    kind: Option<WebAuthoredSourceKind>,
) -> Option<WebSourceProvenance> {
    let path = Url::parse(url).ok()?.to_file_path().ok()?;
    let path = Utf8PathBuf::from_path_buf(path).ok()?;
    let id = path
        .strip_prefix(logical_base)
        .unwrap_or(&path)
        .as_str()
        .replace('\\', "/");
    Some(WebSourceProvenance {
        kind: kind.unwrap_or_else(|| authored_source_kind(&path)),
        id,
        sha256: sha256.to_owned(),
    })
}

fn authored_source_kind(path: &Utf8PathBuf) -> WebAuthoredSourceKind {
    if path.file_name() == Some("fe.toml") {
        return WebAuthoredSourceKind::FeManifest;
    }
    match path
        .extension()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "fe" => WebAuthoredSourceKind::Fe,
        "html" | "htm" => WebAuthoredSourceKind::Html,
        "css" => WebAuthoredSourceKind::Css,
        "js" | "mjs" | "cjs" => WebAuthoredSourceKind::JavaScript,
        "rs" => WebAuthoredSourceKind::Rust,
        "wgsl" => WebAuthoredSourceKind::Wgsl,
        "wasm" => WebAuthoredSourceKind::Wasm,
        "json" => WebAuthoredSourceKind::Json,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "avif" | "ico" | "bin" => {
            WebAuthoredSourceKind::Asset
        }
        _ => WebAuthoredSourceKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_dependencies(contents: &str) -> SourceDependencyInventory {
        let url = "file:///cache-test/src/lib.fe".to_owned();
        SourceDependencyInventory {
            version: SOURCE_DEPENDENCY_INVENTORY_VERSION,
            root: url.clone(),
            sources: vec![SourceDependency {
                url,
                sha256: sha256_hex(contents.as_bytes()),
            }],
        }
    }

    fn cache_artifact(dependencies: SourceDependencyInventory) -> RenderBundleArtifact {
        RenderBundleArtifact {
            wasm: Some(vec![0, 97, 115, 109]),
            wgsl: b"@fragment fn main() {}".to_vec(),
            pass_wgsl: vec![RenderShaderArtifact {
                path: "pass-0.wgsl".to_owned(),
                bytes: b"@compute @workgroup_size(1) fn main() {}".to_vec(),
            }],
            support_files: Vec::new(),
            resource_files: Vec::new(),
            scoped_task_files: vec![RenderSupportArtifact {
                path: "tasks.js".to_owned(),
                bytes: b"export const task = true;\n".to_vec(),
            }],
            manifest_json: br#"{"protocol":"fe-web-bundle"}"#.to_vec(),
            source_dependencies: Some(dependencies),
        }
    }

    #[test]
    fn native_page_projection_resolves_its_real_ingot_dependencies() {
        let source = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../demos/sketches/gallery_page/src/lib.fe")
            .canonicalize_utf8()
            .unwrap();
        let url = Url::from_file_path(source.as_std_path()).unwrap();
        let projected = page_compile(&url)
            .expect("native page projection")
            .expect("the gallery page belongs to an ingot");
        assert!(projected.diagnostics.is_empty());
        assert!(projected.page.is_some());
        assert!(projected.source_dependencies.sources.iter().any(|source| {
            source
                .url
                .ends_with("/demos/sketches/source_inspector/src/lib.fe")
        }));
    }

    fn request(
        path: &str,
        entry: &str,
        mode: WebMode,
        workgroup: [Option<u32>; 3],
        canonical: WebCanonicalPolicy,
        canonical_entries: &[&str],
    ) -> CompileRequest {
        CompileRequest {
            path: path.into(),
            entry: Some(entry.to_owned()),
            mode: Some(mode),
            workgroup,
            source_id: None,
            canonical,
            canonical_entries: canonical_entries
                .iter()
                .map(|entry| (*entry).to_owned())
                .collect(),
        }
    }

    #[test]
    fn mode_requires_explicit_consistent_workgroup() {
        let missing = build(
            &request(
                "missing.fe",
                "shade",
                WebMode::Grid,
                [Some(8), None, Some(1)],
                WebCanonicalPolicy::Disabled,
                &[],
            ),
            &"out".into(),
        )
        .unwrap_err();
        assert!(missing.contains("requires non-zero"), "{missing}");

        let render = build(
            &request(
                "missing.fe",
                "shade",
                WebMode::Render,
                [Some(8), Some(4), Some(1)],
                WebCanonicalPolicy::Disabled,
                &[],
            ),
            &"out".into(),
        )
        .unwrap_err();
        assert!(render.contains("only valid"), "{render}");

        let compute = build(
            &request(
                "missing.fe",
                "classify",
                WebMode::Compute,
                [Some(1), Some(1), Some(1)],
                WebCanonicalPolicy::Disabled,
                &[],
            ),
            &"out".into(),
        )
        .unwrap_err();
        assert!(compute.contains("authored in Fe"), "{compute}");
    }

    #[test]
    fn compute_only_actor_is_derived_by_the_public_web_build_path() {
        let fixture = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../codegen/tests/fixtures/actor_compute_only")
            .canonicalize_utf8()
            .unwrap();
        let bundle = compile(&CompileRequest {
            path: fixture,
            entry: None,
            mode: None,
            workgroup: [None, None, None],
            source_id: None,
            canonical: WebCanonicalPolicy::Disabled,
            canonical_entries: Vec::new(),
        })
        .expect("derive compute-only actor through fe web build");

        assert_eq!(bundle.manifest.layout.mode, WebBundleMode::Compute);
        assert_eq!(bundle.manifest.source_entry, "write_receipt");
        assert_eq!(bundle.manifest.passes.len(), 2);
        assert_eq!(bundle.manifest.passes[0].source_entry, "seed");
        assert_eq!(bundle.manifest.passes[1].source_entry, "write_receipt");
        assert!(
            bundle
                .manifest
                .passes
                .iter()
                .all(|pass| pass.dispatch == Some([1, 1, 1]))
        );
        assert_eq!(bundle.manifest.surface, None);
    }

    #[test]
    fn web_compile_discovers_and_publishes_verified_resource_assets() {
        let fixture = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../codegen/tests/fixtures/actor_content_addressed_resource")
            .canonicalize_utf8()
            .unwrap();
        let bundle = compile(&request(
            fixture.as_str(),
            "paint",
            WebMode::Render,
            [None, None, None],
            WebCanonicalPolicy::Disabled,
            &[],
        ))
        .expect("content-addressed CLI bundle");
        let artifact = bundle.manifest.resources[0]
            .artifact
            .as_ref()
            .expect("resource artifact");
        let materialized = bundle.materialized_files().unwrap();
        assert_eq!(
            materialized
                .iter()
                .find(|file| file.path() == artifact.path)
                .expect("published resource bytes")
                .bytes(),
            b"0123456789abcde\n"
        );
    }

    #[test]
    fn resource_asset_filename_cannot_forge_content_identity() {
        let temp = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(temp.path()).unwrap();
        let directory = root.join("assets/sha256");
        std::fs::create_dir_all(directory.as_std_path()).unwrap();
        std::fs::write(
            directory.join(format!("{}.bin", "0".repeat(64))),
            b"different bytes",
        )
        .unwrap();
        let error = load_resource_assets(root).unwrap_err();
        assert!(
            error.contains("hashes to") && error.contains("not its filename"),
            "unexpected resource identity error: {error}"
        );
    }

    #[test]
    fn render_cache_key_covers_sources_and_entry() {
        let first = cache_dependencies("pub fn first() {}");
        let second = cache_dependencies("pub fn second() {}");
        let authored_js = [WebSourceProvenance {
            id: "demo/host.js".to_owned(),
            sha256: "11".repeat(32),
            kind: WebAuthoredSourceKind::JavaScript,
        }];
        let first_key = render_cache_key(&first, &[], Some("shade")).unwrap();
        assert_eq!(
            first_key,
            render_cache_key(&first, &[], Some("shade")).unwrap()
        );
        assert_ne!(
            first_key,
            render_cache_key(&first, &[], Some("other")).unwrap()
        );
        assert_ne!(
            first_key,
            render_cache_key(&second, &[], Some("shade")).unwrap()
        );
        assert_ne!(
            first_key,
            render_cache_key(&first, &authored_js, Some("shade")).unwrap()
        );
    }

    #[test]
    fn provenance_namespace_widens_for_shared_local_ingots() {
        let sources = [
            SourceDependency {
                url: "file:///repo/demos/sketches/cga3d/src/lib.fe".to_owned(),
                sha256: "00".repeat(32),
            },
            SourceDependency {
                url: "file:///repo/ingots/sparse_clifford/src/lib.fe".to_owned(),
                sha256: "11".repeat(32),
            },
        ];
        let base = provenance_logical_base(Utf8Path::new("/repo/demos/sketches/cga3d"), &sources);
        assert_eq!(base, Utf8Path::new("/repo"));
        let source = source_provenance(&base, &sources[1].url, &sources[1].sha256, None).unwrap();
        assert_eq!(source.id, "ingots/sparse_clifford/src/lib.fe");
        assert_eq!(source.kind, WebAuthoredSourceKind::Fe);
    }

    #[test]
    fn ingot_source_audit_classifies_and_digests_non_fe_inputs() {
        let temp = tempfile::tempdir().unwrap();
        let app = temp.path().join("demo");
        std::fs::create_dir_all(app.join("src")).unwrap();
        std::fs::write(app.join("fe.toml"), "[ingot]\nname = \"demo\"\n").unwrap();
        std::fs::write(app.join("src/lib.fe"), "pub fn shade() -> u32 { 0 }\n").unwrap();
        std::fs::write(app.join("host.js"), "export const hiddenPolicy = true;\n").unwrap();
        let assets = app.join("assets/sha256");
        std::fs::create_dir_all(&assets).unwrap();
        let resource_bytes = b"immutable resource";
        let resource_digest = sha256_hex(resource_bytes);
        std::fs::write(
            assets.join(format!("{resource_digest}.bin")),
            resource_bytes,
        )
        .unwrap();

        let app = Utf8PathBuf::from_path_buf(app).unwrap();
        let audit = ingot_source_audit(&app).unwrap();
        assert!(audit.authored_sources.iter().any(
            |source| source.id == "demo/src/lib.fe" && source.kind == WebAuthoredSourceKind::Fe
        ));
        let host = audit
            .non_fe_authored_sources
            .iter()
            .find(|source| source.id == "demo/host.js")
            .unwrap();
        assert_eq!(host.kind, WebAuthoredSourceKind::JavaScript);
        assert_eq!(
            host.sha256,
            sha256_hex(b"export const hiddenPolicy = true;\n")
        );
        let resource = audit
            .non_fe_authored_sources
            .iter()
            .find(|source| source.id.ends_with(&format!("{resource_digest}.bin")))
            .unwrap();
        assert_eq!(resource.kind, WebAuthoredSourceKind::Asset);
        assert_eq!(resource.sha256, resource_digest);

        let first_cache_key = render_cache_key(
            &audit.dependencies,
            &audit.non_fe_authored_sources,
            Some("shade"),
        )
        .unwrap();
        std::fs::write(
            app.join(format!("assets/sha256/{resource_digest}.bin")),
            b"changed resource",
        )
        .unwrap();
        let changed = ingot_source_audit(&app).unwrap();
        let changed_cache_key = render_cache_key(
            &changed.dependencies,
            &changed.non_fe_authored_sources,
            Some("shade"),
        )
        .unwrap();
        assert_ne!(first_cache_key, changed_cache_key);
    }

    #[test]
    fn executable_cache_identity_ignores_timestamp_only_rewrites() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("fe-test-compiler");
        std::fs::write(&executable, b"same compiler bytes").unwrap();
        let first = executable_cache_identity(&executable);
        std::fs::write(&executable, b"same compiler bytes").unwrap();
        assert_eq!(first, executable_cache_identity(&executable));

        std::fs::write(&executable, b"changed compiler bytes").unwrap();
        assert_ne!(first, executable_cache_identity(&executable));
    }

    #[test]
    fn render_cache_round_trips_only_matching_dependencies() {
        let temp = tempfile::tempdir().unwrap();
        let dependencies = cache_dependencies("pub fn shade() {}");
        let artifact = cache_artifact(dependencies.clone());
        store_render_cache(temp.path(), "key", &artifact).unwrap();

        let loaded = load_render_cache(temp.path(), "key", &dependencies).unwrap();
        assert_eq!(loaded.wasm, artifact.wasm);
        assert_eq!(loaded.wgsl, artifact.wgsl);
        assert_eq!(loaded.manifest_json, artifact.manifest_json);
        assert_eq!(loaded.pass_wgsl.len(), 1);
        assert_eq!(loaded.pass_wgsl[0].path, artifact.pass_wgsl[0].path);
        assert_eq!(loaded.pass_wgsl[0].bytes, artifact.pass_wgsl[0].bytes);
        assert_eq!(loaded.scoped_task_files, artifact.scoped_task_files);
        assert_eq!(loaded.source_dependencies, artifact.source_dependencies);

        assert!(
            load_render_cache(
                temp.path(),
                "key",
                &cache_dependencies("pub fn changed() {}")
            )
            .is_none()
        );
    }

    #[test]
    fn explicit_canonical_entries_require_an_enabled_policy() {
        let disabled = build(
            &request(
                "missing.fe",
                "shade",
                WebMode::Render,
                [None, None, None],
                WebCanonicalPolicy::Disabled,
                &["update"],
            ),
            &"out".into(),
        )
        .unwrap_err();
        assert!(disabled.contains("only valid"), "{disabled}");
    }

    #[test]
    fn ingot_web_build_rejects_diagnostics_in_unused_dependency() {
        let temp = tempfile::tempdir().unwrap();
        let app = temp.path().join("app");
        let dependency = temp.path().join("checked_dependency");
        let transitive = temp.path().join("broken_dependency");
        std::fs::create_dir_all(app.join("src")).unwrap();
        std::fs::create_dir_all(dependency.join("src")).unwrap();
        std::fs::create_dir_all(transitive.join("src")).unwrap();
        std::fs::write(
            app.join("fe.toml"),
            "[ingot]\nname = \"web_app\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\nchecked = { path = \"../checked_dependency\" }\n",
        )
        .unwrap();
        std::fs::write(
            app.join("src/lib.fe"),
            "pub fn shade(x: u32, y: u32) -> u32 { x + y }\n",
        )
        .unwrap();
        std::fs::write(
            dependency.join("fe.toml"),
            "[ingot]\nname = \"checked_dependency\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\nbroken = { path = \"../broken_dependency\" }\n",
        )
        .unwrap();
        std::fs::write(
            dependency.join("src/lib.fe"),
            "pub fn unused_wrapper() -> i32 { 7 }\n",
        )
        .unwrap();
        std::fs::write(
            transitive.join("fe.toml"),
            "[ingot]\nname = \"broken_dependency\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            transitive.join("src/lib.fe"),
            "pub fn unused_but_valid() -> i32 { 42 }\n",
        )
        .unwrap();

        let app = Utf8PathBuf::from_path_buf(app).unwrap();
        let dependency_url = Url::from_directory_path(
            dependency
                .canonicalize()
                .expect("dependency path must canonicalize"),
        )
        .expect("dependency path must be a file URL");
        let transitive_url = Url::from_directory_path(
            transitive
                .canonicalize()
                .expect("transitive dependency path must canonicalize"),
        )
        .expect("transitive dependency path must be a file URL");
        let compile_app = || {
            compile(&CompileRequest {
                path: app.clone(),
                entry: Some("shade".to_owned()),
                mode: Some(WebMode::Render),
                workgroup: [None, None, None],
                source_id: None,
                canonical: WebCanonicalPolicy::Disabled,
                canonical_entries: Vec::new(),
            })
        };

        compile_app().expect("initial clean dependency must compile");
        assert_eq!(
            crate::dependency_diagnostics::dependency_analysis_count(&dependency_url),
            1,
            "the direct local dependency should be analyzed on the first build"
        );
        assert_eq!(
            crate::dependency_diagnostics::dependency_analysis_count(&transitive_url),
            1,
            "the transitive local dependency should be analyzed on the first build"
        );
        compile_app().expect("unchanged clean dependency must compile from its proof");
        assert_eq!(
            crate::dependency_diagnostics::dependency_analysis_count(&dependency_url),
            1,
            "an unchanged clean dependency should be reused across fresh databases"
        );
        assert_eq!(
            crate::dependency_diagnostics::dependency_analysis_count(&transitive_url),
            1,
            "an unchanged transitive dependency should also be reused"
        );

        std::fs::write(
            transitive.join("src/lib.fe"),
            "pub fn unused_but_invalid() -> i32 { missing_value }\n",
        )
        .unwrap();
        let error = compile_app().unwrap_err();
        assert!(
            error.contains("dependency diagnostics prevent web build")
                && error.contains("broken_dependency")
                && error.contains("missing_value"),
            "{error}"
        );
        assert_eq!(
            crate::dependency_diagnostics::dependency_analysis_count(&dependency_url),
            2,
            "changing transitive content must invalidate its parent's clean proof"
        );
        assert_eq!(
            crate::dependency_diagnostics::dependency_analysis_count(&transitive_url),
            2,
            "changing transitive content must invalidate its own clean proof"
        );

        let repeated_error = compile_app().unwrap_err();
        assert!(repeated_error.contains("missing_value"), "{repeated_error}");
        assert_eq!(
            crate::dependency_diagnostics::dependency_analysis_count(&dependency_url),
            2,
            "a clean parent at the changed closure may be reused"
        );
        assert_eq!(
            crate::dependency_diagnostics::dependency_analysis_count(&transitive_url),
            3,
            "a failed dependency analysis must never be cached as clean"
        );
    }

    #[test]
    fn one_command_builds_separate_gpu_and_required_canonical_entries() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("canonical.fe");
        std::fs::write(
            &source,
            r#"
struct Request { value: u32 }
struct Response { value: u32 }
struct VerifyRequest { sample: i32 }
struct VerifyResponse { accepted: i32 }
pub fn update(request: Request) -> Response {
    Response { value: request.value + 1 }
}
pub fn verify(request: VerifyRequest) -> VerifyResponse {
    VerifyResponse { accepted: request.sample }
}
pub fn shade(x: u32, y: u32) -> u32 {
    x + y
}
"#,
        )
        .unwrap();
        let source = Utf8PathBuf::from_path_buf(source).unwrap();
        let out = Utf8PathBuf::from_path_buf(temp.path().join("bundle")).unwrap();
        let request = CompileRequest {
            path: source,
            entry: Some("shade".to_owned()),
            mode: Some(WebMode::Render),
            workgroup: [None, None, None],
            source_id: None,
            canonical: WebCanonicalPolicy::Required,
            canonical_entries: vec![
                "verify".to_owned(),
                "update".to_owned(),
                "verify".to_owned(),
            ],
        };
        build(&request, &out).unwrap();
        assert!(out.join("module.wasm").exists());
        assert!(out.join("shader.wgsl").exists());
        let manifest = std::fs::read_to_string(out.join("manifest.json")).unwrap();
        assert!(manifest.contains("\"source_entry\": \"shade\""));
        assert!(manifest.contains("\"export\": \"fe_cabi_update\""));
        assert!(manifest.contains("\"export\": \"fe_cabi_verify\""));
        assert!(
            manifest.find("\"name\": \"verify\"").unwrap()
                < manifest.find("\"name\": \"update\"").unwrap()
        );
        assert!(manifest.contains("\"embedded\": true"));
    }

    #[test]
    fn required_canonical_build_discovers_fe_marked_lanes_without_cli_names() {
        let temp = tempfile::tempdir().unwrap();
        let app = temp.path().join("auto_actor");
        std::fs::create_dir_all(app.join("src")).unwrap();
        std::fs::write(
            app.join("fe.toml"),
            "[ingot]\nname = \"auto_actor\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            app.join("src/lib.fe"),
            r#"
#[host_placement(worker)]
pub trait Worker {}
struct Request { value: u32 }
struct Response { value: u32 }
pub fn update(request: Request) -> Response uses (Worker) {
    Response { value: request.value + 1 }
}
pub fn shade(x: u32, y: u32) -> u32 { x + y }
"#,
        )
        .unwrap();
        let out = Utf8PathBuf::from_path_buf(temp.path().join("bundle")).unwrap();
        build(
            &CompileRequest {
                path: Utf8PathBuf::from_path_buf(app).unwrap(),
                entry: Some("shade".to_owned()),
                mode: Some(WebMode::Render),
                workgroup: [None, None, None],
                source_id: None,
                canonical: WebCanonicalPolicy::Required,
                canonical_entries: Vec::new(),
            },
            &out,
        )
        .unwrap();
        let manifest = std::fs::read_to_string(out.join("manifest.json")).unwrap();
        assert!(manifest.contains("\"name\": \"update\""));
        assert!(manifest.contains("\"placement\": \"worker\""));
        assert!(out.join("interface.js").is_file());
        assert!(out.join("runtime/worker-host.js").is_file());
    }
}
