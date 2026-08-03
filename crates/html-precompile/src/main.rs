use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn usage() -> &'static str {
    "usage: fe-html-precompile <index.html> --out <directory> [--document-url <url>]"
}

fn main() {
    if let Err(error) = run() {
        eprintln!("fe-html-precompile: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| usage().to_owned())?;
    let mut output = None;
    let mut document_url = None;
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| format!("missing value for {}; {}", flag.to_string_lossy(), usage()))?;
        match flag.to_str() {
            Some("--out") => output = Some(PathBuf::from(value)),
            Some("--document-url") => {
                document_url = Some(
                    value
                        .into_string()
                        .map_err(|_| "--document-url must be valid UTF-8".to_owned())?,
                );
            }
            _ => {
                return Err(format!(
                    "unknown option {}; {}",
                    flag.to_string_lossy(),
                    usage()
                ));
            }
        }
    }
    let output = output.ok_or_else(|| format!("--out is required; {}", usage()))?;
    if output.exists() {
        return Err(format!(
            "output directory {} already exists; refusing to overwrite",
            output.display()
        ));
    }
    let input = input
        .canonicalize()
        .map_err(|error| format!("could not resolve {}: {error}", input.display()))?;
    let html = fs::read_to_string(&input)
        .map_err(|error| format!("could not read {}: {error}", input.display()))?;
    let document_url = document_url.unwrap_or_else(|| {
        url::Url::from_file_path(&input)
            .expect("canonical input path must convert to a file URL")
            .to_string()
    });
    let built = fe_html_precompile::precompile_html(&document_url, &html, |url| {
        let path = url
            .to_file_path()
            .map_err(|_| format!("external Fe source is not a local file URL: {url}"))?;
        fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))
    })
    .map_err(|error| error.to_string())?;

    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let leaf = output
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid output directory {}", output.display()))?;
    let staging = parent.join(format!(".{leaf}.fe-staging-{}", std::process::id()));
    if staging.exists() {
        return Err(format!(
            "staging directory {} already exists",
            staging.display()
        ));
    }
    let publish = (|| -> Result<(), String> {
        fs::create_dir_all(&staging)
            .map_err(|error| format!("could not create {}: {error}", staging.display()))?;
        fs::write(staging.join("index.html"), built.html)
            .map_err(|error| format!("could not write staged index: {error}"))?;
        for (relative, bytes) in built.assets {
            let destination = staging.join(&relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
            }
            fs::write(&destination, bytes)
                .map_err(|error| format!("could not write {}: {error}", destination.display()))?;
        }
        fs::rename(&staging, &output).map_err(|error| {
            format!(
                "could not publish {} as {}: {error}",
                staging.display(),
                output.display()
            )
        })
    })();
    if publish.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    publish?;
    println!("built {}", output.display());
    Ok(())
}
