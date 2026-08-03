//! Standards-parsed HTML entrypoint for generic Fe Web applications.
//!
//! Script discovery and rewriting live in `fe-html-precompile`; this module is
//! deliberately just host tooling for filesystem loading and atomic publication.

use std::fs;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};

pub fn precompile(html_path: &Utf8Path, output: &Utf8Path) -> Result<(), String> {
    if output.exists() {
        return Err(format!(
            "output destination already exists: {output} (refusing to merge or overwrite)"
        ));
    }
    let canonical_html = html_path
        .canonicalize_utf8()
        .map_err(|error| format!("failed to resolve HTML entry {html_path}: {error}"))?;
    let source_html = fs::read_to_string(&canonical_html)
        .map_err(|error| format!("failed to read HTML entry {canonical_html}: {error}"))?;
    let document_url = url::Url::from_file_path(&canonical_html)
        .map_err(|_| format!("HTML entry cannot be represented as a file URL: {canonical_html}"))?;
    let result =
        fe_html_precompile::precompile_html(document_url.as_str(), &source_html, |source_url| {
            let path = source_url
                .to_file_path()
                .map_err(|_| format!("unsupported non-file Fe source URL: {source_url}"))?;
            fs::read_to_string(&path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))
        })
        .map_err(|error| error.to_string())?;

    let parent = publication_parent(output);
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create output parent {parent}: {error}"))?;
    let staging = tempfile::Builder::new()
        .prefix(".fe-web-")
        .tempdir_in(parent)
        .map_err(|error| format!("failed to create staging directory in {parent}: {error}"))?;
    let staging_path = Utf8PathBuf::from_path_buf(staging.keep())
        .map_err(|path| format!("staging path is not UTF-8: {}", path.display()))?;

    let publication = (|| -> io::Result<()> {
        fs::write(staging_path.join("index.html"), result.html)?;
        for (relative, bytes) in result.assets {
            let destination = staging_path.join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(destination, bytes)?;
        }
        fs::rename(&staging_path, output)
    })();
    if let Err(error) = publication {
        let _ = fs::remove_dir_all(&staging_path);
        return Err(format!("failed to publish Web site at {output}: {error}"));
    }
    Ok(())
}

fn publication_parent(output: &Utf8Path) -> &Utf8Path {
    output
        .parent()
        .filter(|path| !path.as_str().is_empty())
        .unwrap_or_else(|| Utf8Path::new("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precompile_publishes_a_digest_verified_site_atomically() {
        let root = tempfile::tempdir().unwrap();
        let root = Utf8Path::from_path(root.path()).unwrap();
        fs::write(
            root.join("index.html"),
            r#"<!doctype html><script type="application/fe" data-fe-src="app.fe"></script>"#,
        )
        .unwrap();
        fs::write(root.join("app.fe"), "pub fn main() {}").unwrap();
        let output = root.join("dist");
        precompile(&root.join("index.html"), &output).unwrap();
        let html = fs::read_to_string(output.join("index.html")).unwrap();
        assert!(html.contains(r#"type="application/fe+wasm""#));
        assert!(html.contains("data-fe-integrity=\"sha256-"));
        assert!(output.join("assets").is_dir());
        assert!(
            precompile(&root.join("index.html"), &output)
                .unwrap_err()
                .contains("refusing to merge or overwrite")
        );
    }

    #[test]
    fn relative_output_uses_the_current_directory_as_staging_parent() {
        assert_eq!(
            publication_parent(Utf8Path::new("dist")),
            Utf8Path::new(".")
        );
        assert_eq!(
            publication_parent(Utf8Path::new("build/dist")),
            Utf8Path::new("build")
        );
    }
}
