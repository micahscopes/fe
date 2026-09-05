//! Recorder coverage for the public scalar shader adapter. These gates establish
//! capture lifecycle and artifact parity, not shader execution correctness.
#![cfg(feature = "spirv-backend")]

use common::InputDb;
use driver::DriverDataBase;
use std::{fs, path::Path, process::Command};
use url::Url;

#[test]
fn scalar_shader_capture_preserves_artifacts_and_reports_partial_runs() {
    check_capture("scalar");
}

#[test]
fn grid_shader_capture_preserves_artifacts_and_reports_partial_runs() {
    check_capture("grid");
}

fn check_capture(pipeline: &str) {
    let directory = tempfile::tempdir().unwrap();
    for mode in ["off", "complete", "partial", "strict"] {
        let output = directory.path().join(mode);
        fs::create_dir(&output).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap());
        child
            .args(["--exact", "scalar_shader_capture_child", "--nocapture"])
            .env("FE_SCALAR_CAPTURE_TEST_MODE", mode)
            .env("FE_SCALAR_CAPTURE_TEST_PIPELINE", pipeline)
            .env("FE_SCALAR_CAPTURE_TEST_OUTPUT", &output)
            .env_remove("FE_BLOAT_CAPTURE_DIR")
            .env_remove("FE_OBSERVE_STRICT")
            .env_remove("FE_OBSERVE_MAX_EVENTS")
            .env_remove("FE_BLOAT_FORCE_INLINE_HELPERS");
        if mode != "off" {
            child.env("FE_BLOAT_CAPTURE_DIR", output.join("capture"));
        }
        if mode == "partial" || mode == "strict" {
            child.env("FE_OBSERVE_MAX_EVENTS", "1");
        }
        if mode == "strict" {
            child.env("FE_OBSERVE_STRICT", "1");
        }
        let result = child.output().unwrap();
        assert!(
            result.status.success(),
            "mode={mode}\n{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    for mode in ["complete", "partial"] {
        for artifact in ["shader.wgsl", "shader.spv"] {
            assert_eq!(
                fs::read(directory.path().join("off").join(artifact)).unwrap(),
                fs::read(directory.path().join(mode).join(artifact)).unwrap(),
                "observation mode {mode} changed {artifact}"
            );
        }
    }
    let records = events(&directory.path().join("complete/capture"));
    assert_eq!(records[0]["pipeline"], format!("legacy_{pipeline}"));
    assert!(
        records
            .iter()
            .any(|e| e["event"] == "stage" && e["stage_id"] == "pre-merge")
    );
    assert!(records.iter().any(|e| e["event"] == "capture_completed"));
    let request = request_directory(&directory.path().join("complete/capture"));
    for artifact in ["shader.wgsl", "shader.spv"] {
        assert_eq!(
            fs::read(request.join(artifact)).unwrap(),
            fs::read(directory.path().join("off").join(artifact)).unwrap(),
            "recorded {artifact} must be the emitted artifact"
        );
    }
    let partial = events(&directory.path().join("partial/capture"));
    assert_eq!(partial.len(), 1);
    assert_eq!(partial[0]["event"], "capture_started");
    assert!(!directory.path().join("strict/shader.wgsl").exists());
}

fn events(directory: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(request_directory(directory).join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn request_directory(directory: &Path) -> std::path::PathBuf {
    let requests = fs::read_dir(directory)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(requests.len(), 1);
    requests[0].path()
}

#[test]
fn scalar_shader_capture_child() {
    let Ok(mode) = std::env::var("FE_SCALAR_CAPTURE_TEST_MODE") else {
        return;
    };
    let directory = std::env::var_os("FE_SCALAR_CAPTURE_TEST_OUTPUT").unwrap();
    let grid = std::env::var("FE_SCALAR_CAPTURE_TEST_PIPELINE").unwrap() == "grid";
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///scalar_capture.fe").unwrap();
    db.workspace().touch(
        &mut db,
        url.clone(),
        Some(if grid {
            "fn add(_ x: u32) -> u32 { x + 7 }\npub fn kernel(_ x: u32, _ y: u32) -> u32 { add(x + y) }\n".into()
        } else {
            "fn add(_ x: u32) -> u32 { x + 7 }\npub fn kernel(_ x: u32) -> u32 { add(x) }\n".into()
        }),
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let top = db.top_mod(file);
    assert!(db.run_on_top_mod(top).format_diags(&db).is_empty());
    let package = mir::build_wasm_runtime_package_for_entry(&db, top, "kernel").unwrap();
    let result = if grid {
        fe_codegen::compile_runtime_package_spirv_grid(&db, &package, [1, 1, 1])
    } else {
        fe_codegen::compile_runtime_package_spirv_with_workgroup(&db, &package, [1, 1, 1])
    };
    if mode == "strict" {
        let error = match result {
            Ok(_) => panic!("strict capture budget must fail"),
            Err(e) => e,
        };
        assert!(error.to_string().contains("event limit"), "{error}");
        return;
    }
    let artifact = result.unwrap();
    assert!(
        if grid {
            // Grid diagnostics are a per-invocation array, not a scalar slot.
            artifact.layout.bindings.iter().any(|binding| binding.name == "trap")
        } else {
            artifact.layout.trap.is_some()
        },
        "authored checked addition must retain a shader trap channel"
    );
    let directory = Path::new(&directory);
    fs::write(directory.join("shader.spv"), artifact.as_bytes()).unwrap();
    fs::write(directory.join("shader.wgsl"), artifact.wgsl.unwrap()).unwrap();
}
