use camino::Utf8PathBuf;
use codegen::{WebBuildOptions, WebBundle};
use common::InputDb;
use driver::{
    DriverDataBase,
    cli_target::{CliTarget, resolve_cli_target},
};
use hir::hir_def::HirIngot;
use url::Url;

use crate::WebMode;

pub fn build(
    path: &Utf8PathBuf,
    entry: &str,
    mode: WebMode,
    out: &Utf8PathBuf,
    workgroup: [Option<u32>; 3],
    source_id: Option<String>,
) -> Result<(), String> {
    if entry.is_empty() {
        return Err("`--entry` must not be empty".to_string());
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
    let options = match mode {
        WebMode::Render => WebBuildOptions::render(entry, source_id),
        WebMode::Grid => WebBuildOptions::grid(entry, workgroup.unwrap(), source_id),
    };
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
        )
        .unwrap_err();
        assert!(render.contains("only valid"), "{render}");
    }
}
