//! I-desargues control generator: compile the DESARGUES CONTROLLER
//! (`update_desargues`, the drag-driven `sweep/spin/zoom` update fn) to wasm
//! through the real Fe drivers, gate it, and emit `gen/ctl.fe` + `gen/ctl.wasm`
//! + `gen/ctl.json` under `demos/webgpu-desargues-interactive/`.
//!
//! This generator owns ONLY the controls half of the interactive Desargues
//! demo. The render half (the actor's `shade` fragment -> `frag.wasm` /
//! `frag.wgsl` / `manifest.json`) is produced separately by the real, shipped
//! `fe web build` CLI against `demos/sketches/desargues` (see `generate.sh`).
//!
//! HARD-FAIL discipline (every gate passes before a byte is written):
//!   * the `update_desargues` wasm export signature is EXACTLY
//!     `(f32,f32,f32,i32,i32,i32) -> (f32,f32,f32)` (parsed with wasmparser);
//!   * the wasm module has zero imports (a pure, stateless control fn);
//!   * `update_desargues`, executed under wasmtime, EXACTLY matches an
//!     independently written Rust oracle (directed cases covering the
//!     sweep/spin/zoom clamps + a deterministic 2,000-step forward-fed random
//!     walk). f32 addition/multiplication/comparison are IEEE754-deterministic,
//!     so the match is bit-exact, not epsilon-tolerant.
//! Any deviation panics before `gen/` is touched.
//!
//! Run: `cargo run -p fe-codegen --example gen_desargues_interactive_ctl`

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

/// The SSOT control source, byte-identical to what ships in `gen/ctl.fe`.
const CTL_SOURCE: &str = include_str!("../tests/fixtures/spirv/desargues_ctl.fe");
const CTL_NAME: &str = "update_desargues";

/// The initial (sweep, spin, zoom) the pump seeds: a mid sweep (the second
/// triangle clearly distinct from O and from triangle 1), no view rotation, and
/// a zoom framing the axis and cross-points.
const VIEW_INIT: (f32, f32, f32) = (0.62, 0.0, 2.4);

const SWEEP_SENSITIVITY: f32 = 0.0025;
const SPIN_SENSITIVITY: f32 = 0.01;
const SWEEP_MIN: f32 = 0.3;
const SWEEP_MAX: f32 = 1.6;
const ZOOM_MIN: f32 = 0.8;
const ZOOM_MAX: f32 = 5.0;
const ZOOM_STEP_IN: f32 = 0.875;
const ZOOM_STEP_OUT: f32 = 1.125;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/codegen should have a two-level ancestor (the repo root)");
    let demo_dir = repo_root.join("demos/webgpu-desargues-interactive");
    let gen_dir = demo_dir.join("gen");
    std::fs::create_dir_all(&gen_dir)
        .unwrap_or_else(|e| panic!("could not create {}: {e}", gen_dir.display()));

    eprintln!(
        "gen_desargues_interactive_ctl: compiling `{CTL_NAME}` (wasm) through the real Fe drivers"
    );

    // --- 1. Fe -> wasm (the controls). --------------------------------------
    let ctl_wasm = compile_to_wasm(CTL_SOURCE, "gen_desargues_ctl_wasm");
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
        "control export `{CTL_NAME}` must be (f32,f32,f32,i32,i32,i32) -> (f32,f32,f32) (sweep, \
         spin, zoom, dx, dy, dzoom -> the same triple); got {ctl_sig:?}"
    );
    eprintln!(
        "  wasm export `{CTL_NAME}` signature is exactly (f32,f32,f32,i32,i32,i32) -> (f32,f32,f32)"
    );

    // --- 2. Derive the arg names from the ACTUAL source (not hardcoded). ----
    let ctl_args = parse_fn_params(CTL_SOURCE, CTL_NAME);
    assert_eq!(
        ctl_args,
        vec!["sweep", "spin", "zoom", "dx", "dy", "dzoom"],
        "update_desargues's parsed param names changed shape; the event map below assumes this order"
    );

    // --- 3. HARD GATE 2: wasmtime-executed update_desargues == the independent
    // Rust oracle, directed cases + a deterministic random walk. -------------
    let engine = wasmtime::Engine::default();
    let module =
        wasmtime::Module::new(&engine, &ctl_wasm).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(f32, f32, f32, i32, i32, i32), (f32, f32, f32)>(&mut store, CTL_NAME)
        .expect("update_desargues should be (f32,f32,f32,i32,i32,i32) -> (f32,f32,f32)");

    let directed: [(f32, f32, f32, i32, i32, i32); 9] = [
        (0.62, 0.0, 2.4, 0, 0, 0),    // no-op identity
        (0.4, 0.0, 2.4, 200, 0, 0),   // sweep advances
        (1.5, 0.0, 2.4, 200, 0, 0),   // sweep clamps at 1.6
        (0.35, 0.0, 2.4, -300, 0, 0), // sweep clamps at 0.3
        (0.62, 0.5, 2.4, 0, 100, 0),  // spin turns
        (0.62, 0.5, 2.4, 0, 0, -1),   // zoom in one notch
        (0.62, 0.5, 2.4, 0, 0, 1),    // zoom out one notch
        (0.62, 0.5, 0.85, 0, 0, -1),  // zoom-in clamps at ZOOM_MIN
        (0.62, 0.5, 4.9, 0, 0, 1),    // zoom-out clamps at ZOOM_MAX
    ];
    let mut steps = 0usize;
    for (sweep, spin, zoom, dx, dy, dzoom) in directed {
        let got = f
            .call(&mut store, (sweep, spin, zoom, dx, dy, dzoom))
            .expect("update_desargues should run");
        let want = update_desargues_oracle(sweep, spin, zoom, dx, dy, dzoom);
        assert_eq!(
            got, want,
            "HARD FAIL: wasmtime update_desargues({sweep},{spin},{zoom},{dx},{dy},{dzoom}) = {got:?}, \
             oracle = {want:?}. Refusing to emit a stale/faked control fn."
        );
        steps += 1;
    }

    // A deterministic forward-fed random walk (a small LCG), the same discipline
    // the clifford/mandelbrot JS oracles use client-side, run here in the
    // generator too since f32 arithmetic is bit-exactly reproducible.
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
        let dz = (((q >> 20) & 3) as i32) - 1;
        let (sweep, spin, zoom) = state;
        let got = f
            .call(&mut store, (sweep, spin, zoom, dx, dy, dz))
            .expect("update_desargues should run");
        let want = update_desargues_oracle(sweep, spin, zoom, dx, dy, dz);
        assert_eq!(
            got, want,
            "HARD FAIL at walk step {steps}: wasmtime update_desargues({sweep},{spin},{zoom},{dx},{dy},{dz}) \
             = {got:?}, oracle = {want:?}"
        );
        state = got;
        steps += 1;
    }
    eprintln!(
        "  oracle: wasmtime update_desargues == the independent Rust oracle across {steps} directed + \
         random-walk gestures (bit-exact f32)"
    );

    // --- 4. The event map: which normalized DOM input feeds which arg. ------
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
        // The 3-value reply order IS the desargues actor's (sweep, spin, zoom)
        // uniform order (demos/sketches/desargues/src/lib.fe).
        "result_order": [&ctl_args[0], &ctl_args[1], &ctl_args[2]],
        "view_arg_count": 3,
        "view_init": [VIEW_INIT.0, VIEW_INIT.1, VIEW_INIT.2],
        "clamps": {
            "sweep": [SWEEP_MIN, SWEEP_MAX],
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
        "gen_desargues_interactive_ctl: wrote 3 files to {}\n  ctl.fe  ctl.wasm  ctl.json",
        gen_dir.display()
    );
}

/// The Rust-side independent oracle: an ordinary reimplementation of the Fe
/// control fn, written directly (never copy-pasted from the Fe body), used to
/// hard-gate the wasm export before any file is written.
#[allow(clippy::too_many_arguments)]
fn update_desargues_oracle(
    sweep: f32,
    spin: f32,
    zoom: f32,
    dx: i32,
    dy: i32,
    dzoom: i32,
) -> (f32, f32, f32) {
    let dxf = dx as f32;
    let dyf = dy as f32;
    let sweep1 = (sweep + dxf * SWEEP_SENSITIVITY).clamp(SWEEP_MIN, SWEEP_MAX);
    let spin1 = spin + dyf * SPIN_SENSITIVITY;
    let mut zoom1 = zoom;
    if dzoom < 0 {
        zoom1 *= ZOOM_STEP_IN;
    }
    if dzoom > 0 {
        zoom1 *= ZOOM_STEP_OUT;
    }
    zoom1 = zoom1.clamp(ZOOM_MIN, ZOOM_MAX);
    (sweep1, spin1, zoom1)
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
        "generator": "cargo run -p fe-codegen --example gen_desargues_interactive_ctl",
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
