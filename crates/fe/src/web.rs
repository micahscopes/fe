use camino::Utf8PathBuf;
use codegen::{WebBuildOptions, WebBundle};
use common::InputDb;
use driver::{
    DriverDataBase,
    cli_target::{CliTarget, resolve_cli_target},
};
use hir::hir_def::HirIngot;
use url::Url;

use crate::{WebCanonicalPolicy, WebMode};

pub fn build(
    path: &Utf8PathBuf,
    entry: &str,
    mode: WebMode,
    out: &Utf8PathBuf,
    workgroup: [Option<u32>; 3],
    source_id: Option<String>,
    canonical: WebCanonicalPolicy,
    canonical_entries: &[String],
) -> Result<(), String> {
    if entry.is_empty() {
        return Err("`--entry` must not be empty".to_string());
    }
    match (canonical, canonical_entries.is_empty()) {
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
    let workgroup = match (mode, workgroup) {
        (WebMode::Render, [None, None, None]) => None,
        (WebMode::Render, _) => {
            return Err("workgroup flags are only valid with `--mode grid`".to_string());
        }
        (WebMode::Grid, [Some(x), Some(y), Some(z)]) if x > 0 && y > 0 && z > 0 => Some([x, y, z]),
        (WebMode::Grid, _) => {
            return Err(
                "grid mode requires non-zero `--workgroup-x`, `--workgroup-y`, and `--workgroup-z`"
                    .to_string(),
            );
        }
    };

    let mut db = DriverDataBase::default();
    let target = resolve_cli_target(&mut db, path, false)?;
    let top_mod = match target {
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
            db.top_mod(file)
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
                .containing_ingot(&db, url)
                .ok_or_else(|| {
                    format!(
                        "`{dir_path}` did not resolve to one ingot; target an ingot directory explicitly"
                    )
                })?;
            ingot.root_mod(&db)
        }
    };

    let diagnostics = db.run_on_top_mod(top_mod);
    if !diagnostics.is_empty() {
        return Err(format!(
            "source diagnostics prevent web build:\n{}",
            diagnostics.format_diags(&db)
        ));
    }
    let mut options = match mode {
        WebMode::Render => WebBuildOptions::render(entry, source_id),
        WebMode::Grid => WebBuildOptions::grid(entry, workgroup.unwrap(), source_id),
    }
    .with_canonical_policy(match canonical {
        WebCanonicalPolicy::Disabled => codegen::WebCanonicalPolicy::Disabled,
        WebCanonicalPolicy::Optional => codegen::WebCanonicalPolicy::Optional,
        WebCanonicalPolicy::Required => codegen::WebCanonicalPolicy::Required,
    });
    options = options.with_canonical_entries(canonical_entries.iter().cloned());
    let bundle = WebBundle::compile(&db, top_mod, options).map_err(|error| error.to_string())?;
    bundle
        .write_atomic(out.as_std_path())
        .map_err(|error| error.to_string())?;
    println!("wrote web bundle: {out}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_requires_explicit_consistent_workgroup() {
        let missing = build(
            &"missing.fe".into(),
            "shade",
            WebMode::Grid,
            &"out".into(),
            [Some(8), None, Some(1)],
            None,
            WebCanonicalPolicy::Disabled,
            &[],
        )
        .unwrap_err();
        assert!(missing.contains("requires non-zero"), "{missing}");

        let render = build(
            &"missing.fe".into(),
            "shade",
            WebMode::Render,
            &"out".into(),
            [Some(8), Some(4), Some(1)],
            None,
            WebCanonicalPolicy::Disabled,
            &[],
        )
        .unwrap_err();
        assert!(render.contains("only valid"), "{render}");
    }

    #[test]
    fn canonical_entry_policy_combinations_fail_before_io() {
        let missing = build(
            &"missing.fe".into(),
            "shade",
            WebMode::Render,
            &"out".into(),
            [None, None, None],
            None,
            WebCanonicalPolicy::Required,
            &[],
        )
        .unwrap_err();
        assert!(missing.contains("is required"), "{missing}");

        let disabled = build(
            &"missing.fe".into(),
            "shade",
            WebMode::Render,
            &"out".into(),
            [None, None, None],
            None,
            WebCanonicalPolicy::Disabled,
            &["update".to_owned()],
        )
        .unwrap_err();
        assert!(disabled.contains("only valid"), "{disabled}");
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
        build(
            &source,
            "shade",
            WebMode::Render,
            &out,
            [None, None, None],
            None,
            WebCanonicalPolicy::Required,
            &[
                "verify".to_owned(),
                "update".to_owned(),
                "verify".to_owned(),
            ],
        )
        .unwrap();
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
