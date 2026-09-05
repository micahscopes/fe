//! Compile the small scalar-helper Render fixture with and without one named
//! force-inline intervention, preserving exact WGSL and structured captures.

use std::{
    error::Error,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use common::InputDb;
use driver::DriverDataBase;
use sha2::{Digest, Sha256};
use url::Url;

const SOURCE: &str = include_str!("../tests/fixtures/spirv/scalar_helper_call_render.fe");
const SOURCE_URL: &str = "file:///scalar_helper_call_render.fe";

fn main() -> Result<(), Box<dyn Error>> {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|argument| argument == "--compile-one")
    {
        return compile_one_child(&arguments);
    }
    let output = arguments
        .first()
        .map(PathBuf::from)
        .ok_or("usage: bloat_capture_kernel OUTPUT_DIRECTORY")?;
    fs::create_dir(&output)?;
    let observer_disabled = run_child(&output, "observer-disabled", None, false)?;
    let baseline = run_child(&output, "baseline", None, true)?;
    if observer_disabled["wgsl_sha256"] != baseline["wgsl_sha256"] {
        return Err("enabling capture changed the baseline WGSL".into());
    }
    if observer_disabled["spirv_sha256"] != baseline["spirv_sha256"] {
        return Err("enabling capture changed the baseline SPIR-V".into());
    }
    let variant = run_child(&output, "force-inline-mix-words", Some("mix_words"), true)?;
    let summary = serde_json::json!({
        "schema": "fe-bloat-scalar-helper-pilot/1",
        "source": "crates/codegen/tests/fixtures/spirv/scalar_helper_call_render.fe",
        "virtual_source_url": SOURCE_URL,
        "source_sha256": hex::encode(Sha256::digest(SOURCE.as_bytes())),
        "compiler_version": env!("CARGO_PKG_VERSION"),
        "observer_disabled": observer_disabled,
        "baseline": baseline,
        "variant": variant,
        "timing_note": "Frontend package construction and shader lowering/backend/observer wall times are separate single observations, not precision benchmarks.",
        "claim_limit": "Successful compilation proves backend validation. GPU behavior is checked separately by bloat_gpu_oracle."
    });
    write_new(
        &output.join("compile-summary.json"),
        serde_json::to_string_pretty(&summary)?.as_bytes(),
    )?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn run_child(
    output: &Path,
    label: &str,
    force_inline: Option<&str>,
    capture: bool,
) -> Result<serde_json::Value, Box<dyn Error>> {
    let mut command = Command::new(std::env::current_exe()?);
    command.arg("--compile-one").arg(output).arg(label);
    if capture {
        command.env(
            "FE_BLOAT_CAPTURE_DIR",
            output.join(format!("{label}-capture")),
        );
    } else {
        command.env_remove("FE_BLOAT_CAPTURE_DIR");
    }
    if let Some(helper) = force_inline {
        command.env("FE_BLOAT_FORCE_INLINE_HELPERS", helper);
    } else {
        command.env_remove("FE_BLOAT_FORCE_INLINE_HELPERS");
    }
    let result = command.output()?;
    if !result.status.success() {
        return Err(format!(
            "{label} child failed with {}: {}",
            result.status,
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(serde_json::from_slice(&result.stdout)?)
}

fn compile_one_child(arguments: &[std::ffi::OsString]) -> Result<(), Box<dyn Error>> {
    if arguments.len() != 3 {
        return Err("internal usage: --compile-one OUTPUT_DIRECTORY LABEL".into());
    }
    let output = PathBuf::from(&arguments[1]);
    let label = arguments[2].to_str().ok_or("label is not UTF-8")?;
    let mut db = DriverDataBase::default();
    let url = Url::parse(SOURCE_URL)?;
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_owned()));
    let file = db.workspace().get(&db, &url).ok_or("fixture should load")?;
    let frontend_started = Instant::now();
    let package = mir::build_wasm_runtime_package(&db, db.top_mod(file))?;
    let frontend_elapsed = frontend_started.elapsed();
    let shader_started = Instant::now();
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)?;
    let shader_elapsed = shader_started.elapsed();
    let spirv = artifact
        .words
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    write_new(&output.join(format!("{label}.spv")), &spirv)?;
    let wgsl = artifact
        .wgsl
        .ok_or("Render compilation did not emit WGSL")?;
    let path = output.join(format!("{label}.wgsl"));
    write_new(&path, wgsl.as_bytes())?;
    let wgsl_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("WGSL output filename is not UTF-8")?;
    let capture_directory = std::env::var_os("FE_BLOAT_CAPTURE_DIR")
        .map(PathBuf::from)
        .and_then(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        });
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "label": label,
            "intervention": std::env::var("FE_BLOAT_FORCE_INLINE_HELPERS").ok().map_or_else(
                || serde_json::json!({"kind": "none"}),
                |helper| serde_json::json!({"kind": "force_inline_named_retained_helper", "helper": helper})),
            "wgsl": wgsl_name,
            "wgsl_bytes": wgsl.len(),
            "wgsl_sha256": hex::encode(Sha256::digest(wgsl.as_bytes())),
            "spirv_bytes": spirv.len(),
            "spirv_sha256": hex::encode(Sha256::digest(&spirv)),
            "frontend_package_wall_time_ns": u64::try_from(frontend_elapsed.as_nanos()).unwrap_or(u64::MAX),
            "shader_lowering_backend_observer_wall_time_ns": u64::try_from(shader_elapsed.as_nanos()).unwrap_or(u64::MAX),
            "capture_directory": capture_directory,
        }))?
    );
    Ok(())
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    let mut output = OpenOptions::new().write(true).create_new(true).open(path)?;
    output.write_all(bytes)?;
    output.flush()?;
    Ok(())
}
