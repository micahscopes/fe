use std::{
    env,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

const REQUIRED_FE: &str = "ba7724c9b";
const REQUIRED_SONATINA: &str = "dcd96e5fef29096ca7d7715e58ced88e1c328e49";
const DEMOS: [(&str, &str); 3] = [
    ("CGA", "demos/webgpu-cga-inversion/verify-assets.py"),
    ("Mandelbrot", ""),
    ("QCGA", "demos/webgpu-qcga3d-quadric/verify-assets.py"),
];

#[derive(Debug)]
struct Facts {
    fe_head: String,
    fe_required: bool,
    fe_dirty: bool,
    sonatina_head: String,
    sonatina_dirty: bool,
    chrome: Option<PathBuf>,
    vulkan: bool,
    bundles: Vec<(String, bool, String)>,
}

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.as_slice() != ["mb2", "doctor"] {
        eprintln!("usage: cargo run -p xtask -- mb2 doctor");
        return ExitCode::from(2);
    }
    let fe = env::var_os("MB2_FE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .to_owned()
        });
    let sonatina = env::var_os("SONATINA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/workspace/sonatina"));
    match inspect(&fe, &sonatina) {
        Ok(facts) => {
            let (text, ok) = report(&facts, &fe, &sonatina);
            print!("{text}");
            if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(error) => {
            eprintln!("mb2 doctor: ERROR: {error}");
            ExitCode::from(1)
        }
    }
}

fn inspect(fe: &Path, sonatina: &Path) -> Result<Facts, String> {
    let fe_head = git(fe, &["rev-parse", "HEAD"])?;
    let fe_required = Command::new("git")
        .arg("-C")
        .arg(fe)
        .args(["merge-base", "--is-ancestor", REQUIRED_FE, "HEAD"])
        .status()
        .map_err(|e| e.to_string())?
        .success();
    let sonatina_head = git(sonatina, &["rev-parse", "HEAD"])?;
    let fe_dirty = !git(fe, &["status", "--porcelain", "--untracked-files=no"])?.is_empty();
    let sonatina_dirty = !git(sonatina, &["status", "--porcelain"])?.is_empty();
    let chrome = chrome_path();
    let vulkan = vulkan_available();
    let bundles = DEMOS
        .iter()
        .map(|(name, verifier)| {
            if verifier.is_empty() {
                let generated = fe.join("demos/webgpu-mandelbrot-interactive/gen");
                let required = [
                    "kernel.fe",
                    "frag.wgsl",
                    "frag.wasm",
                    "layout.json",
                    "reference.json",
                    "ctl.fe",
                    "ctl.wasm",
                    "ctl.json",
                ];
                let missing = required
                    .iter()
                    .filter(|file| !generated.join(file).is_file())
                    .copied()
                    .collect::<Vec<_>>();
                let provenance = std::fs::read_to_string(generated.join("layout.json"))
                    .is_ok_and(|text| text.contains("\"provenance\""));
                let ok = missing.is_empty() && provenance;
                return (
                    (*name).to_owned(),
                    ok,
                    if ok {
                        "8 generated assets complete; reference provenance present".to_owned()
                    } else {
                        format!("missing={missing:?}; provenance={provenance}")
                    },
                );
            }
            let output = Command::new("python3").arg(fe.join(verifier)).output();
            match output {
                Ok(out) => (
                    (*name).to_owned(),
                    out.status.success(),
                    String::from_utf8_lossy(if out.status.success() {
                        &out.stdout
                    } else {
                        &out.stderr
                    })
                    .trim()
                    .to_owned(),
                ),
                Err(e) => ((*name).to_owned(), false, e.to_string()),
            }
        })
        .collect();
    Ok(Facts {
        fe_head,
        fe_required,
        fe_dirty,
        sonatina_head,
        sonatina_dirty,
        chrome,
        vulkan,
        bundles,
    })
}

fn report(f: &Facts, fe: &Path, sonatina: &Path) -> (String, bool) {
    let sonatina_ok = f.sonatina_head == REQUIRED_SONATINA;
    let chrome_ok = f.chrome.is_some();
    let bundles_ok = f.bundles.iter().all(|(_, ok, _)| *ok);
    let ok =
        f.fe_required && !f.fe_dirty && sonatina_ok && !f.sonatina_dirty && chrome_ok && bundles_ok;
    let mut out = String::from("MB2 doctor\n");
    line(
        &mut out,
        f.fe_required && !f.fe_dirty,
        "Fe",
        &format!(
            "{} at {} (required ancestor {}){}",
            fe.display(),
            f.fe_head,
            REQUIRED_FE,
            if f.fe_dirty { "; tracked dirty" } else { "" }
        ),
    );
    line(
        &mut out,
        sonatina_ok && !f.sonatina_dirty,
        "Sonatina",
        &format!(
            "{} at {} (required exact {}){}",
            sonatina.display(),
            f.sonatina_head,
            REQUIRED_SONATINA,
            if f.sonatina_dirty { "; dirty" } else { "" }
        ),
    );
    line(
        &mut out,
        chrome_ok,
        "Chrome SwiftShader",
        f.chrome
            .as_ref()
            .map(|p| p.display().to_string())
            .as_deref()
            .unwrap_or("not found"),
    );
    if f.vulkan {
        line(
            &mut out,
            true,
            "Vulkan ICD",
            "available (native/lavapipe candidate)",
        );
    } else {
        out.push_str("  [WARN] Vulkan ICD: not found; browser SwiftShader remains usable\n");
    }
    for (name, passed, detail) in &f.bundles {
        line(&mut out, *passed, &format!("{name} bundle"), detail);
    }
    out.push_str("\nSafe next commands (read-only/checking):\n");
    out.push_str(&format!(
        "  SONATINA_DIR={} python3 demos/webgpu-cga-inversion/verify-assets.py\n",
        sonatina.display()
    ));
    out.push_str("  test -s demos/webgpu-mandelbrot-interactive/gen/reference.json && test -s demos/webgpu-mandelbrot-interactive/gen/ctl.json\n");
    out.push_str("  python3 demos/webgpu-qcga3d-quadric/verify-assets.py\n");
    if let Some(chrome) = &f.chrome {
        out.push_str(&format!(
            "  CHROME_BIN={} bash demos/webgpu-cga-inversion/smoke-chrome.sh\n",
            chrome.display()
        ));
        out.push_str(&format!(
            "  CHROME_BIN={} bash demos/webgpu-mandelbrot-interactive/smoke-chrome.sh\n",
            chrome.display()
        ));
        out.push_str(&format!(
            "  CHROME_BIN={} bash demos/webgpu-qcga3d-quadric/smoke-chrome.sh\n",
            chrome.display()
        ));
    }
    out.push_str("  CARGO_BUILD_JOBS=4 cargo nextest run -p fe-codegen --no-fail-fast\n");
    out.push_str(&format!(
        "\nResult: {}\n",
        if ok {
            "READY"
        } else {
            "NOT READY (fix FAIL checks; WARN is non-blocking)"
        }
    ));
    (out, ok)
}

fn line(out: &mut String, ok: bool, name: &str, detail: &str) {
    out.push_str(&format!(
        "  [{}] {name}: {detail}\n",
        if ok { "PASS" } else { "FAIL" }
    ));
}

fn git(path: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn chrome_path() -> Option<PathBuf> {
    if let Some(path) = env::var_os("CHROME_BIN")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
    {
        return Some(path);
    }
    let cached = PathBuf::from("/workspace/.cache/fe-cga-chrome/chrome-nix-wrapper");
    if cached.is_file() {
        return Some(cached);
    }
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .flat_map(|d| ["google-chrome", "chromium"].map(|n| d.join(n)))
            .find(|p| p.is_file())
    })
}

fn vulkan_available() -> bool {
    env::var_os("VK_ICD_FILENAMES").is_some()
        || [
            "/run/opengl-driver/share/vulkan/icd.d",
            "/usr/share/vulkan/icd.d",
        ]
        .iter()
        .any(|p| {
            Path::new(p)
                .read_dir()
                .is_ok_and(|mut d| d.next().is_some())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn facts() -> Facts {
        Facts {
            fe_head: "feed".into(),
            fe_required: true,
            fe_dirty: false,
            sonatina_head: REQUIRED_SONATINA.into(),
            sonatina_dirty: false,
            chrome: Some("/chrome".into()),
            vulkan: false,
            bundles: vec![
                ("CGA".into(), true, "ok".into()),
                ("Mandelbrot".into(), true, "ok".into()),
                ("QCGA".into(), true, "ok".into()),
            ],
        }
    }
    #[test]
    fn ready_is_deterministic_and_vulkan_is_advisory() {
        let (text, ok) = report(&facts(), Path::new("/fe"), Path::new("/sonatina"));
        assert!(ok);
        assert!(text.contains("[WARN] Vulkan ICD"));
        assert!(text.ends_with("Result: READY\n"));
    }
    #[test]
    fn dirty_or_bad_bundle_blocks_readiness() {
        let mut f = facts();
        f.fe_dirty = true;
        f.bundles[2].1 = false;
        let (text, ok) = report(&f, Path::new("/fe"), Path::new("/sonatina"));
        assert!(!ok);
        assert!(text.contains("[FAIL] Fe"));
        assert!(text.contains("[FAIL] QCGA bundle"));
    }
    #[test]
    fn missing_chrome_omits_unsafe_smoke_commands() {
        let mut f = facts();
        f.chrome = None;
        let (text, ok) = report(&f, Path::new("/fe"), Path::new("/sonatina"));
        assert!(!ok);
        assert!(!text.contains("smoke-chrome.sh"));
    }
}
