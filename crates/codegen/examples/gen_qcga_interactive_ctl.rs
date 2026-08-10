//! I-qcga control generator: compile the QUADRIC-PENCIL CONTROLLER
//! (`update_quadric`, the drag-driven `lambda/theta/zoom` update fn) to wasm
//! through the real Fe drivers, gate it, and emit `gen/ctl.fe` + `gen/ctl.wasm`
//! + `gen/ctl.json` under `demos/webgpu-qcga-interactive/`.
//!
//! This generator owns ONLY the controls half of the interactive qcga demo.
//! The render half (the actor's `shade` fragment -> `frag.wasm` / `frag.wgsl`
//! / `manifest.json`) is produced separately by the real, already-shipped
//! `fe web build` CLI against `demos/sketches/qcga` (see `generate.sh`):
//! that path is proven (the qcga ingot, with its CTFE `SparsePlan` + derive
//! provider, compiles through it today), so this generator does not
//! reimplement it.
//!
//! HARD-FAIL discipline (every gate passes before a byte is written):
//!   * the `update_quadric` wasm export signature is EXACTLY
//!     `(f32,f32,f32,i32,i32,i32) -> (f32,f32,f32)` (parsed with wasmparser);
//!   * the wasm module has zero imports (a pure, stateless control fn);
//!   * `update_quadric`, executed under wasmtime, EXACTLY matches an
//!     independently written Rust oracle (directed cases covering the
//!     lambda/theta/zoom clamps + a deterministic 2,000-step forward-fed
//!     random walk). f32 addition/multiplication/comparison are IEEE754-
//!     deterministic, so the match is bit-exact, not epsilon-tolerant.
//! Any deviation panics before `gen/` is touched.
//!
//! Run: `cargo run -p fe-codegen --example gen_qcga_interactive_ctl`

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

/// The SSOT control source, byte-identical to what ships in `gen/ctl.fe`.
const CTL_SOURCE: &str = include_str!("../tests/fixtures/spirv/qcga_ctl.fe");
const CTL_NAME: &str = "update_quadric";

/// The initial (lambda, theta, zoom) the pump seeds: a blended pencil member
/// (an ellipse, not a degenerate line pair), a mild axis tilt so the xz cross
/// term is live, and a zoom that frames the unit-scale conic inside the slice.
const VIEW_INIT: (f32, f32, f32) = (0.25, 0.35, 1.3);

const LAMBDA_SENSITIVITY: f32 = 0.0025;
const THETA_SENSITIVITY: f32 = 0.01;
const ZOOM_MIN: f32 = 0.3;
const ZOOM_MAX: f32 = 4.0;
const ZOOM_STEP_IN: f32 = 0.875;
const ZOOM_STEP_OUT: f32 = 1.125;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/codegen should have a two-level ancestor (the repo root)");
    let demo_dir = repo_root.join("demos/webgpu-qcga-interactive");
    let gen_dir = demo_dir.join("gen");
    std::fs::create_dir_all(&gen_dir)
        .unwrap_or_else(|e| panic!("could not create {}: {e}", gen_dir.display()));

    eprintln!(
        "gen_qcga_interactive_ctl: compiling `{CTL_NAME}` (wasm) through the real Fe drivers"
    );

    // --- 1. Fe -> wasm (the controls). --------------------------------------
    let ctl_wasm = compile_to_wasm(CTL_SOURCE, "gen_qcga_ctl_wasm");
    assert_zero_imports(&ctl_wasm);

    // HARD GATE 1: the control export is EXACTLY
    // (f32,f32,f32,i32,i32,i32) -> (f32,f32,f32).
    let ctl_sig = export_signature(&ctl_wasm, CTL_NAME);
    let expected_sig = (
        vec![
            WasmTy::F32,
            WasmTy::F32,
            WasmTy::F32,
            WasmTy::I32,
            WasmTy::I32,
            WasmTy::I32,
        ],
        vec![WasmTy::F32, WasmTy::F32, WasmTy::F32],
    );
    assert_eq!(
        ctl_sig, expected_sig,
        "control export `{CTL_NAME}` must be (f32,f32,f32,i32,i32,i32) -> (f32,f32,f32) (lambda, \
         theta, zoom, dx, dy, dzoom -> the same triple); got {ctl_sig:?}"
    );
    eprintln!(
        "  wasm export `{CTL_NAME}` signature is exactly (f32,f32,f32,i32,i32,i32) -> (f32,f32,f32)"
    );

    // --- 2. Derive the arg names from the ACTUAL source (not hardcoded). ----
    let ctl_args = parse_fn_params(CTL_SOURCE, CTL_NAME);
    assert_eq!(
        ctl_args,
        vec!["lambda", "theta", "zoom", "dx", "dy", "dzoom"],
        "update_quadric's parsed param names changed shape; the event map below assumes this order"
    );

    // --- 3. HARD GATE 2: wasmtime-executed update_quadric == the independent
    // Rust oracle, directed cases + a deterministic random walk. -------------
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, &ctl_wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(f32, f32, f32, i32, i32, i32), (f32, f32, f32)>(&mut store, CTL_NAME)
        .expect("update_quadric should be (f32,f32,f32,i32,i32,i32) -> (f32,f32,f32)");

    let directed: [(f32, f32, f32, i32, i32, i32); 9] = [
        (0.5, 0.0, 1.0, 0, 0, 0),    // no-op identity
        (0.0, 0.0, 1.0, 200, 0, 0),  // lambda sweeps up
        (0.9, 0.0, 1.0, 200, 0, 0),  // lambda clamps at 1.0
        (0.5, 0.0, 1.0, -300, 0, 0), // lambda clamps at 0.0
        (0.5, 0.5, 1.0, 0, 100, 0),  // theta turns
        (0.5, 0.5, 1.0, 0, 0, -1),   // zoom in one notch
        (0.5, 0.5, 1.0, 0, 0, 1),    // zoom out one notch
        (0.5, 0.5, 0.32, 0, 0, -1),  // zoom-in clamps at ZOOM_MIN
        (0.5, 0.5, 3.9, 0, 0, 1),    // zoom-out clamps at ZOOM_MAX
    ];
    let mut steps = 0usize;
    for (lambda, theta, zoom, dx, dy, dzoom) in directed {
        let got = f
            .call(&mut store, (lambda, theta, zoom, dx, dy, dzoom))
            .expect("update_quadric should run");
        let want = update_quadric_oracle(lambda, theta, zoom, dx, dy, dzoom);
        assert_eq!(
            got, want,
            "HARD FAIL: wasmtime update_quadric({lambda},{theta},{zoom},{dx},{dy},{dzoom}) = {got:?}, \
             oracle = {want:?}. Refusing to emit a stale/faked control fn."
        );
        steps += 1;
    }

    // A deterministic forward-fed random walk (a small LCG), exactly the
    // discipline the clifford/mandelbrot JS oracles use client-side, run here
    // in the generator too since f32 arithmetic is bit-exactly reproducible.
    let mut state = VIEW_INIT;
    let mut s: u32 = 0x9e37_79b9;
    let mut rnd = || {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        s
    };
    for _ in 0..2000 {
        let q = rnd();
        let dx = (((q >> 3) & 63) as i32) - 32;
        let dy = (((q >> 12) & 63) as i32) - 32;
        let dz = (((q >> 20) & 3) as i32) - 1; // -1, 0, 1, 2 -> all handled
        let (lambda, theta, zoom) = state;
        let got = f
            .call(&mut store, (lambda, theta, zoom, dx, dy, dz))
            .expect("update_quadric should run");
        let want = update_quadric_oracle(lambda, theta, zoom, dx, dy, dz);
        assert_eq!(
            got, want,
            "HARD FAIL at walk step {steps}: wasmtime update_quadric({lambda},{theta},{zoom},{dx},{dy},{dz}) \
             = {got:?}, oracle = {want:?}"
        );
        state = got;
        steps += 1;
    }
    eprintln!(
        "  oracle: wasmtime update_quadric == the independent Rust oracle across {steps} directed + \
         random-walk gestures (bit-exact f32)"
    );

    // --- 4. The event map: which normalized DOM input feeds which arg. ------
    // The first three args are the fed-back stored (lambda, theta, zoom); the
    // last three are event-derived (drag dx/dy, wheel dzoom).
    let event_map = serde_json::json!({
        &ctl_args[0]: { "source": "view", "index": 0 },
        &ctl_args[1]: { "source": "view", "index": 1 },
        &ctl_args[2]: { "source": "view", "index": 2 },
        &ctl_args[3]: { "source": "pointer", "field": "movementX", "when": "drag" },
        &ctl_args[4]: { "source": "pointer", "field": "movementY", "when": "drag" },
        &ctl_args[5]: { "source": "wheel", "field": "deltaYSign" },
    });

    let ctl_json = serde_json::to_string_pretty(&serde_json::json!({
        "module": "ctl.wasm",
        "control_export": CTL_NAME,
        "args": ctl_args,
        "arg_types": ["f32", "f32", "f32", "i32", "i32", "i32"],
        "result_types": ["f32", "f32", "f32"],
        // The 3-value reply order IS the qcga actor's (lambda, theta, zoom)
        // uniform order (demos/sketches/qcga/src/lib.fe).
        "result_order": [&ctl_args[0], &ctl_args[1], &ctl_args[2]],
        "view_arg_count": 3,
        "view_init": [VIEW_INIT.0, VIEW_INIT.1, VIEW_INIT.2],
        "clamps": {
            "lambda": [0.0, 1.0],
            "zoom": [ZOOM_MIN, ZOOM_MAX],
        },
        "event_map": event_map,
        "wasm_bytes": ctl_wasm.len(),
        "provenance": provenance(),
    }))
    .expect("ctl.json should serialize");

    // --- 5. Write (only after every gate passed). ---------------------------
    write_file(&gen_dir.join("ctl.fe"), CTL_SOURCE.as_bytes());
    write_file(&gen_dir.join("ctl.wasm"), &ctl_wasm);
    write_file(&gen_dir.join("ctl.json"), ctl_json.as_bytes());

    eprintln!(
        "gen_qcga_interactive_ctl: wrote 3 files to {}\n  ctl.fe  ctl.wasm  ctl.json",
        gen_dir.display()
    );
}

/// The Rust-side independent oracle: an ordinary reimplementation of the Fe
/// control fn, written directly (never copy-pasted from the Fe body), used to
/// hard-gate the wasm export before any file is written.
#[allow(clippy::too_many_arguments)]
fn update_quadric_oracle(
    lambda: f32,
    theta: f32,
    zoom: f32,
    dx: i32,
    dy: i32,
    dzoom: i32,
) -> (f32, f32, f32) {
    let dxf = dx as f32;
    let dyf = dy as f32;
    let lambda1 = (lambda + dxf * LAMBDA_SENSITIVITY).clamp(0.0, 1.0);
    let theta1 = theta + dyf * THETA_SENSITIVITY;
    let mut zoom1 = zoom;
    if dzoom < 0 {
        zoom1 *= ZOOM_STEP_IN;
    }
    if dzoom > 0 {
        zoom1 *= ZOOM_STEP_OUT;
    }
    zoom1 = zoom1.clamp(ZOOM_MIN, ZOOM_MAX);
    (lambda1, theta1, zoom1)
}

fn compile_to_wasm(source: &str, tag: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{tag}.fe")).expect("wasm gen URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db
        .workspace()
        .get(&db, &url)
        .expect("wasm gen file should load");
    let top_mod = db.top_mod(file);
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .unwrap_or_else(|e| panic!("{tag}: Fe -> wasm compile failed: {e:?}"))
        .into_bytecode()
        .expect("wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("Fe-emitted wasm should be valid");
    bytes
}

fn assert_zero_imports(bytes: &[u8]) {
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::ImportSection(reader) = payload.expect("valid wasm payload") {
            assert_eq!(reader.count(), 0, "the control wasm must have zero imports");
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WasmTy {
    I32,
    F32,
    Other,
}

/// Parse the emitted wasm with wasmparser and return the named export's
/// (params, results) as a coarse i32/f32/other signature.
fn export_signature(bytes: &[u8], export_name: &str) -> (Vec<WasmTy>, Vec<WasmTy>) {
    use wasmparser::{ExternalKind, Payload, TypeRef, ValType};

    let map = |v: &ValType| match v {
        ValType::I32 => WasmTy::I32,
        ValType::F32 => WasmTy::F32,
        _ => WasmTy::Other,
    };

    let mut func_sigs: Vec<(Vec<WasmTy>, Vec<WasmTy>)> = Vec::new();
    let mut func_type_indices: Vec<u32> = Vec::new();
    let mut imported_func_count: u32 = 0;
    let mut export_func_index: Option<u32> = None;

    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        match payload.expect("valid wasm payload") {
            Payload::TypeSection(reader) => {
                for rec in reader {
                    let rec = rec.expect("valid rec group");
                    for sub in rec.into_types() {
                        let ft = sub.unwrap_func();
                        func_sigs.push((
                            ft.params().iter().map(map).collect(),
                            ft.results().iter().map(map).collect(),
                        ));
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    if let TypeRef::Func(_) = import.expect("valid import entry").ty {
                        imported_func_count += 1;
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for tyidx in reader {
                    func_type_indices.push(tyidx.expect("valid func type index"));
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.expect("valid export entry");
                    if export.name == export_name {
                        assert!(
                            matches!(export.kind, ExternalKind::Func),
                            "export `{export_name}` must be a function export"
                        );
                        export_func_index = Some(export.index);
                    }
                }
            }
            _ => {}
        }
    }

    let fidx = export_func_index
        .unwrap_or_else(|| panic!("wasm export `{export_name}` not found in the emitted module"));
    assert!(
        fidx >= imported_func_count,
        "export `{export_name}` (index {fidx}) resolves into the imported range"
    );
    let defined = (fidx - imported_func_count) as usize;
    let tyidx = *func_type_indices
        .get(defined)
        .unwrap_or_else(|| panic!("no function-section entry for defined func {defined}"))
        as usize;
    func_sigs
        .get(tyidx)
        .unwrap_or_else(|| panic!("no type-section entry for type index {tyidx}"))
        .clone()
}

/// Extract the parameter NAMES of `pub fn <fn_name>(...)` from Fe source, in order.
fn parse_fn_params(source: &str, fn_name: &str) -> Vec<String> {
    let needle = format!("fn {fn_name}(");
    let start = source
        .find(&needle)
        .unwrap_or_else(|| panic!("`fn {fn_name}(` not found in the source"))
        + needle.len();
    let rest = &source[start..];
    let end = rest
        .find(')')
        .unwrap_or_else(|| panic!("unterminated param list for `{fn_name}`"));
    let params = &rest[..end];
    params
        .split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .map(|p| {
            p.split(':')
                .next()
                .expect("a param has a name before `:`")
                .trim()
                .to_string()
        })
        .collect()
}

fn provenance() -> serde_json::Value {
    serde_json::json!({
        "source": "Fe compiler (branch mb2)",
        "fe_rev": fe_head_rev(),
        "generator": "cargo run -p fe-codegen --example gen_qcga_interactive_ctl",
        "generated_unix_secs": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

fn fe_head_rev() -> String {
    std::process::Command::new("git")
        .args(["-C", env!("CARGO_MANIFEST_DIR"), "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn write_file(path: &std::path::Path, bytes: &[u8]) {
    std::fs::write(path, bytes)
        .unwrap_or_else(|e| panic!("could not write {}: {e}", path.display()));
}
