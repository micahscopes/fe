use std::collections::{BTreeMap, BTreeSet, HashSet};

use camino::Utf8PathBuf;
use codegen::{WebBuildOptions, WebBundle, WebBundleMode, resolve_web_entry};
use common::InputDb;
use driver::{
    DriverDataBase,
    cli_target::{CliTarget, resolve_cli_target},
};
use fe_compiler_protocol::{SOURCE_DEPENDENCY_INVENTORY_VERSION, SourceDependency, sha256_hex};
use fe_html_precompile::RenderBundleArtifact;
use hir::hir_def::HirIngot;
use url::Url;

use crate::{WebCanonicalPolicy, WebMode, dependency_diagnostics::DependencyIssues};

#[derive(Debug, Clone)]
pub struct CompileRequest {
    pub path: Utf8PathBuf,
    /// Explicit render entry, or `None` to derive it from the module's `actor`
    /// declaration (its single `FragmentSurface` behavior).
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
    }
}

fn from_bundle_mode(mode: WebBundleMode) -> WebMode {
    match mode {
        WebBundleMode::Render => WebMode::Render,
        WebBundleMode::Grid => WebMode::Grid,
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

pub fn compile(request: &CompileRequest) -> Result<WebBundle, String> {
    let CompileRequest {
        path,
        entry,
        mode,
        workgroup,
        source_id,
        canonical,
        canonical_entries,
    } = request;
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
        (WebCanonicalPolicy::Optional | WebCanonicalPolicy::Required, true) => {
            return Err(
                "`--canonical-entry NAME` is required with `--canonical optional|required`"
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
    let target = resolve_cli_target(&mut db, path, false)?;
    let (top_mod, ingot_target) = match target {
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
            (db.top_mod(file), None)
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
            (ingot.root_mod(&db), Some((url, ingot)))
        }
    };

    let diagnostics = match ingot_target {
        Some((_, ingot)) => db.run_on_ingot(ingot),
        None => db.run_on_top_mod(top_mod),
    };
    if !diagnostics.is_empty() {
        return Err(format!(
            "source diagnostics prevent web build:\n{}",
            diagnostics.format_diags(&db)
        ));
    }
    if let Some((ingot_url, _)) = ingot_target {
        let mut seen = HashSet::from([ingot_url.clone()]);
        let dependency_issues = DependencyIssues::collect(&db, &ingot_url, &mut seen);
        if !dependency_issues.is_empty() {
            return Err(format!(
                "dependency diagnostics prevent web build:\n{}",
                dependency_issues.format(&db)
            ));
        }
    }
    // Derive the render entry and mode from the module's `actor` declaration when
    // not given explicitly; when supplied, they are reconciled against the
    // declaration (a mismatch errors, naming both sources).
    let (entry, mode) =
        resolve_web_entry(&db, top_mod, (*entry).clone(), (*mode).map(to_bundle_mode))
            .map_err(|error| error.to_string())?;
    let mode = from_bundle_mode(mode);
    let workgroup = validate_workgroup(mode, *workgroup)?;

    let mut options = match mode {
        WebMode::Render => WebBuildOptions::render(&entry, source_id.clone()),
        WebMode::Grid => WebBuildOptions::grid(&entry, workgroup.unwrap(), source_id.clone()),
    }
    .with_canonical_policy(match canonical {
        WebCanonicalPolicy::Disabled => codegen::WebCanonicalPolicy::Disabled,
        WebCanonicalPolicy::Optional => codegen::WebCanonicalPolicy::Optional,
        WebCanonicalPolicy::Required => codegen::WebCanonicalPolicy::Required,
    });
    options = options.with_canonical_entries(canonical_entries.iter().cloned());
    WebBundle::compile(&db, top_mod, options).map_err(|error| error.to_string())
}

/// The native bundle-lane compiler for the standards `fe web dev`/`fe web
/// precompile` render lane (`fe_html_precompile::precompile_html_with_render_lane`'s
/// `render_compile` closure): the SAME seam `fe web build` uses
/// (`resolve_web_entry` + `WebBundle::compile`, via [`compile`] above), for a
/// `data-fe-src` naming a GpuProgram-actor ingot DIRECTORY. Callers dispatch
/// on `path.is_dir()` before calling this; a standalone `.fe` file stays on
/// the wasm-only facade lane untouched.
pub fn compile_render_bundle(
    path: &Utf8PathBuf,
    entry: Option<&str>,
) -> Result<RenderBundleArtifact, String> {
    let bundle = compile(&CompileRequest {
        path: path.clone(),
        entry: entry.map(str::to_owned),
        mode: None,
        workgroup: [None, None, None],
        source_id: None,
        canonical: WebCanonicalPolicy::Disabled,
        canonical_entries: Vec::new(),
    })?;
    let manifest_json = bundle.manifest_json().map_err(|error| error.to_string())?;
    Ok(RenderBundleArtifact {
        wasm: bundle.wasm,
        wgsl: bundle.wgsl.into_bytes(),
        manifest_json,
        source_dependencies: ingot_source_dependencies(path),
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
    compile_render_bundle(&path, entry).map(Some)
}

/// Best-effort structural dependency inventory for the bundle lane's watch
/// graph: every `.fe` file and `fe.toml` under the ingot directory, plus the
/// same under every LOCAL path dependency it declares (recursively), so
/// editing a shared library ingot (e.g. `demos/sketches/fmath`) is proven to
/// affect every sketch that depends on it. Remote/registry dependencies are
/// not walked (their sources are not local files to watch). `None` when the
/// directory has no readable sources or would not validate as an inventory;
/// callers treat this as "no extra watch entries," not a hard failure.
fn ingot_source_dependencies(
    ingot_dir: &Utf8PathBuf,
) -> Option<fe_compiler_protocol::SourceDependencyInventory> {
    let root_dir = ingot_dir.canonicalize_utf8().ok()?;
    let mut visited = BTreeSet::new();
    let mut sources = BTreeMap::new();
    collect_ingot_sources(&root_dir, &mut visited, &mut sources);
    let root = sources.keys().next()?.clone();
    let inventory = fe_compiler_protocol::SourceDependencyInventory {
        version: SOURCE_DEPENDENCY_INVENTORY_VERSION,
        root,
        sources: sources
            .into_iter()
            .map(|(url, sha256)| SourceDependency { url, sha256 })
            .collect(),
    };
    inventory.validate().ok()?;
    Some(inventory)
}

fn collect_ingot_sources(
    dir: &Utf8PathBuf,
    visited: &mut BTreeSet<Utf8PathBuf>,
    sources: &mut BTreeMap<String, String>,
) {
    let Ok(canonical) = dir.canonicalize_utf8() else {
        return;
    };
    if !visited.insert(canonical.clone()) {
        return;
    }
    for entry in walkdir::WalkDir::new(canonical.as_std_path())
        .into_iter()
        .filter_map(Result::ok)
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let is_source = entry.path().extension().is_some_and(|extension| extension == "fe")
            || entry.file_name() == "fe.toml";
        if !is_source {
            continue;
        }
        let (Ok(path), Ok(bytes)) = (
            Utf8PathBuf::from_path_buf(entry.path().to_path_buf()),
            std::fs::read(entry.path()),
        ) else {
            continue;
        };
        if let Ok(url) = Url::from_file_path(path.as_std_path()) {
            sources.insert(url.to_string(), sha256_hex(&bytes));
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
            collect_ingot_sources(&dependency_path, visited, sources);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn canonical_entry_policy_combinations_fail_before_io() {
        let missing = build(
            &request(
                "missing.fe",
                "shade",
                WebMode::Render,
                [None, None, None],
                WebCanonicalPolicy::Required,
                &[],
            ),
            &"out".into(),
        )
        .unwrap_err();
        assert!(missing.contains("is required"), "{missing}");

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
        let dependency = temp.path().join("broken_dependency");
        std::fs::create_dir_all(app.join("src")).unwrap();
        std::fs::create_dir_all(dependency.join("src")).unwrap();
        std::fs::write(
            app.join("fe.toml"),
            "[ingot]\nname = \"web_app\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\nbroken = { path = \"../broken_dependency\" }\n",
        )
        .unwrap();
        std::fs::write(
            app.join("src/lib.fe"),
            "pub fn shade(x: u32, y: u32) -> u32 { x + y }\n",
        )
        .unwrap();
        std::fs::write(
            dependency.join("fe.toml"),
            "[ingot]\nname = \"broken_dependency\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(
            dependency.join("src/lib.fe"),
            "pub fn unused_but_invalid() -> i32 { missing_value }\n",
        )
        .unwrap();

        let app = Utf8PathBuf::from_path_buf(app).unwrap();
        let error = compile(&CompileRequest {
            path: app,
            entry: Some("shade".to_owned()),
            mode: Some(WebMode::Render),
            workgroup: [None, None, None],
            source_id: None,
            canonical: WebCanonicalPolicy::Disabled,
            canonical_entries: Vec::new(),
        })
        .unwrap_err();
        assert!(
            error.contains("dependency diagnostics prevent web build")
                && error.contains("broken_dependency")
                && error.contains("missing_value"),
            "{error}"
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
}
