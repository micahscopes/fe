//! C3 page generator: assemble the INTERACTIVE all-Fe Clifford rotor page inputs
//! under `demos/webgpu-clifford-interactive/gen/`. A clone of
//! `gen_mandelbrot_interactive_demo` with the rotor fragment + rotor controller in
//! place of the view fragment + pan/zoom controller; the render runner, the
//! demo-blind pump, and the generator discipline are shared.
//!
//! Two Fe units, both compiled through the REAL Fe drivers (never hand-written):
//!   * `clifford_frag_rgba` (the RENDER fragment): the Cl(3) rotor sandwich body
//!     VERBATIM from the C1/C2 kernel, returning a packed RGBA8 word. Compiled
//!     through the Render seam `compile_runtime_package_spirv_render` -> naga WGSL
//!     + a Render `SpirvLayout`. The FOUR broadcast rotor members (rc, r12, r13,
//!     r23) ride the Input struct at `@group(0) @binding(1)`, span 16 (the C2
//!     four-member broadcast, now on the render arm). Also compiled to wasm for the
//!     AMBER per-pixel path + the pinned-rotor oracle.
//!   * `clifford_ctl` (the CONTROLS): compiled to wasm. `update_rotor` is the
//!     pointer-drag rotor controller (yaw in e12, pitch in e13, by geometric-
//!     product composition); its export interface is emitted as `ctl.json` and
//!     GATED by a wasmparser signature check of the actual wasm export.
//!
//! HARD-FAIL discipline (EVERY gate passes before a single file is written):
//!   * browser-profile WGSL: no 64-bit scalar token, naga `wgsl-in` reparse,
//!     validation under `Capabilities::default()` (no SHADER_INT64); plus the
//!     render epilogue (`@vertex` + `@fragment` + `@location(0)` + `unpack4x8unorm`),
//!     the signed-sandwich evidence (`bitcast<i32>`), and the four-member broadcast
//!     load (`input.p0` .. `input.p3`). The fragment is branchless (no `loop`).
//!   * layout is Render / word U32 / result None / vertex `vs_fullscreen` /
//!     fragment `fs_main` / color target `rgba8unorm` / Input binding stride 16;
//!   * the `frag.wasm` export signature is EXACTLY `(i32 x6) -> i32` (2 coords + 4
//!     broadcast rotor members), proving `params.len() == 4`;
//!   * the `ctl.wasm` `update_rotor` export signature is EXACTLY `(i32 x6) ->
//!     (i32, i32, i32, i32)` (parsed with wasmparser), which GATES the `ctl.json`
//!     arg/result interface;
//!   * the rotor ORACLE at the pinned rotors: instantiate `frag.wasm` under
//!     wasmtime, render every 512x512 pixel at each pinned rotor, and assert EVERY
//!     packed-RGBA word equals the in-generator oracle `clifford_frag_rgba_oracle`
//!     (re-derived here from the Cl(3) algebra, never trusted from any doc), then
//!     fold each frame into an FNV-1a-32.
//! Any deviation panics before a single file is written.
//!
//! Run: `cargo run -p fe-codegen --example gen_clifford_interactive_demo`

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, compile_runtime_package_spirv_render, layout_for};
use sonatina_codegen::isa::spirv::{Access, LayoutMode, Role, SpirvLayout, WordKind};
use url::Url;

/// The SSOT fragment source: the exact fixture the C3a e2e tests `include_str!`, so
/// the tested source and the shipped source are byte-identical by construction.
const FRAG_SOURCE: &str = include_str!("../tests/fixtures/spirv/clifford_frag_rgba.fe");
/// The SSOT control source (the pointer-drag `update_rotor` fn).
const CTL_SOURCE: &str = include_str!("../tests/fixtures/spirv/clifford_ctl.fe");

/// The render fragment / wasm export name (the Fe `pub fn`).
const FRAG_NAME: &str = "clifford_frag_rgba";
/// The control export the page drives per input event.
const CTL_NAME: &str = "update_rotor";

/// The dispatch frame: 512x512, the proven view resolution.
const WIDTH: u32 = 512;
const HEIGHT: u32 = 512;

/// The initial rotor the pump seeds (a gentle 3D tilt, so the opening image reads
/// as a tumbled checker). Emitted into `ctl.json` as the initial state (NOT
/// computed in JS): theta/2 = 25deg about the unit bivector (1,2,2)/3.
const ROTOR_INIT: (i32, i32, i32, i32) = (3712, 577, 1154, 1154);

/// The sonatina fork rev the fe workspace is pinned to (render push #3), recorded
/// in `layout.json` provenance.
const SONATINA_REV: &str = "150d327edfa88374802a6cc8089fd77da5fa818b";

/// The pinned rotors the C3a legs use, each `(name, rc, r12, r13, r23,
/// min_distinct)`. A pure-e12 rotor fixes the slab depth -> a flat two-tone checker
/// -> exactly 2 packed colors; the tilted rotor tumbles the slab in 3D and spreads
/// the shades. Floors are DERIVED (>= 2 flat, >= 8 tumble): a one-color image fails.
const ROTOR_PINS: [(&str, i32, i32, i32, i32, usize); 4] = [
    ("identity", 4096, 0, 0, 0, 2),
    ("e12_90", 2896, 2896, 0, 0, 2),
    ("tilted_default", 3712, 577, 1154, 1154, 8),
    ("e12_180", 0, 4096, 0, 0, 2),
];

/// The rotor-sandwich half of the independent oracle, re-derived from the Cl(3)
/// blade products (NOT copied from any doc), integer-identical to the fixture.
fn clifford_sandwich(px: i32, py: i32, rc: i32, r12: i32, r13: i32, r23: i32) -> (i32, i32, i32) {
    let x: i32 = (px - 256) * 16;
    let y: i32 = (py - 256) * 16;
    let z: i32 = 2048;
    let t1: i32 = (rc * x + r12 * y + r13 * z) >> 12;
    let t2: i32 = (rc * y - r12 * x + r23 * z) >> 12;
    let t3: i32 = (rc * z - r13 * x - r23 * y) >> 12;
    let tw: i32 = (r12 * z - r13 * y + r23 * x) >> 12;
    let sx: i32 = (rc * t1 + r12 * t2 + r13 * t3 + r23 * tw) >> 12;
    let sy: i32 = (rc * t2 - r12 * t1 - r13 * tw + r23 * t3) >> 12;
    let sz: i32 = (rc * t3 + r12 * tw - r13 * t1 - r23 * t2) >> 12;
    (sx, sy, sz)
}

/// The independent packed-RGBA oracle for `clifford_frag_rgba`: the sandwich, the
/// branchless 0..255 shade (checker two-tone + depth cue), then the pure-i32 RGBA
/// packing (R=G=shade, B=255-shade, A=255; alpha folded as `- 16777216`). The
/// returned i32 word's bit pattern IS the LE RGBA8; `as u32` reinterprets it.
fn clifford_frag_rgba_oracle(px: i32, py: i32, rc: i32, r12: i32, r13: i32, r23: i32) -> u32 {
    let (sx, sy, sz) = clifford_sandwich(px, py, rc, r12, r13, r23);
    let cell: i32 = (sx >> 11) + (sy >> 11) + (sz >> 11);
    let par: i32 = cell - ((cell >> 1) * 2);
    let s: i32 = par * 160 + 48 + (sz >> 7);
    let lo: i32 = s * (1 + (s >> 31));
    let d: i32 = lo - 255;
    let shade: i32 = lo - d * (1 + (d >> 31));
    let packed: i32 = shade + shade * 256 + (255 - shade) * 65536 - 16_777_216;
    packed as u32
}

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/codegen should have a two-level ancestor (the repo root)");
    let demo_dir = repo_root.join("demos/webgpu-clifford-interactive");
    let gen_dir = demo_dir.join("gen");
    std::fs::create_dir_all(&gen_dir)
        .unwrap_or_else(|e| panic!("could not create {}: {e}", gen_dir.display()));

    eprintln!(
        "gen_clifford_interactive_demo: compiling `{FRAG_NAME}` (Render) + `{CTL_NAME}` (wasm) \
         through the real Fe drivers"
    );

    // --- 1. Fe -> SPIR-V/WGSL, RENDER mode (the fragment). ------------------
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///gen_clifford_frag_rgba.fe").expect("gen URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(FRAG_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("gen file should load");
    let top_mod = db.top_mod(file);

    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("render fragment should build a wasm runtime package");
    let artifact = compile_runtime_package_spirv_render(&db, &package)
        .expect("render fragment should compile Fe -> naga-validated SPIR-V (Render mode)");

    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the u32 render fragment")
        .clone();

    // HARD GATE 1: browser-profile WGSL + the render/sandwich/broadcast tokens.
    assert_browser_profile_wgsl(&wgsl);
    for tok in [
        "@vertex",
        "@fragment",
        "@location(0)",
        "unpack4x8unorm",
        "bitcast<i32>",
        "input.p0",
        "input.p3",
    ] {
        assert!(
            wgsl.contains(tok),
            "render WGSL must contain `{tok}` (render epilogue / signed sandwich / 4-member \
             broadcast load); off-shape, refusing to emit:\n{wgsl}"
        );
    }
    eprintln!(
        "  WGSL passed the browser profile: no 64-bit tokens, wgsl-in reparse OK, validated with \
         Capabilities::default(); @vertex+@fragment+@location(0)+unpack4x8unorm, bitcast<i32>, \
         input.p0..input.p3 (4 broadcast rotor members)"
    );

    // HARD GATE 2: Render layout as the runner assumes.
    assert_eq!(
        artifact.layout.mode,
        LayoutMode::Render,
        "the render seam must state LayoutMode::Render; off-pin, refusing to emit"
    );
    assert_eq!(
        artifact.layout.word,
        WordKind::U32,
        "the render fragment must lower to the u32 word (browser profile)"
    );
    assert!(
        artifact.layout.result.is_none(),
        "Render mode has no single-slot result (the color target is the result)"
    );
    assert_eq!(
        artifact.layout.vertex_entry.as_deref(),
        Some("vs_fullscreen"),
        "Render mode states the @vertex entry name"
    );
    assert_eq!(
        artifact.layout.fragment_entry.as_deref(),
        Some("fs_main"),
        "Render mode states the @fragment entry name"
    );
    assert_eq!(
        artifact.layout.color_target_format.as_deref(),
        Some("rgba8unorm"),
        "Render mode states its color-target format"
    );
    let input_binding = artifact
        .layout
        .bindings
        .iter()
        .find(|b| matches!(b.role, Role::Input))
        .expect("the render layout must carry an Input binding (the broadcast rotor)");
    assert_eq!(
        input_binding.stride, 16,
        "the FOUR broadcast rotor members (4 bytes each) span 16 bytes; off-pin, refusing to emit"
    );
    eprintln!(
        "  layout: mode Render, word U32, result None, vertex vs_fullscreen, fragment fs_main, \
         color rgba8unorm, Input stride {}",
        input_binding.stride
    );

    // --- 2. Fe -> wasm (the fragment, for the amber per-pixel + oracle leg). --
    let frag_wasm = compile_to_wasm(FRAG_SOURCE, "gen_frag_wasm");
    // HARD GATE 3: the fragment export is EXACTLY (i32 x6) -> i32 (2 coords + 4
    // broadcast rotor members). This is what makes `params.len() == 4` honest.
    let frag_sig = export_signature(&frag_wasm, FRAG_NAME);
    assert_eq!(
        frag_sig,
        (vec![WasmTy::I32; 6], vec![WasmTy::I32]),
        "render fragment export `{FRAG_NAME}` must be (i32 x6) -> i32 (px, py + 4 broadcast rotor \
         members); got {frag_sig:?}"
    );
    eprintln!("  wasm export `{FRAG_NAME}` signature is exactly (i32 x6) -> i32 (2 coords + 4 rotor)");

    // --- 3. Fe -> wasm (the controls). --------------------------------------
    let ctl_wasm = compile_to_wasm(CTL_SOURCE, "gen_ctl_wasm");
    // HARD GATE 4: the control export is EXACTLY (i32 x6) -> (i32, i32, i32, i32).
    let ctl_sig = export_signature(&ctl_wasm, CTL_NAME);
    assert_eq!(
        ctl_sig,
        (vec![WasmTy::I32; 6], vec![WasmTy::I32; 4]),
        "control export `{CTL_NAME}` must be (i32 x6) -> (i32, i32, i32, i32) (the native wasm \
         multi-value rotor reply); got {ctl_sig:?}"
    );
    eprintln!("  wasm export `{CTL_NAME}` signature is exactly (i32 x6) -> (i32, i32, i32, i32)");

    // --- 4. Derive the interfaces from the ACTUAL sources (not hardcoded). ---
    let frag_params = parse_fn_params(FRAG_SOURCE, FRAG_NAME);
    assert_eq!(
        frag_params.len(),
        6,
        "the fragment signature must have 6 params (px, py + 4 rotor); parsed {frag_params:?}"
    );
    let rotor_names: Vec<String> = frag_params[2..6].to_vec();
    let param_bytes = artifact.layout.word.width_bytes();
    let params: Vec<serde_json::Value> = rotor_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            serde_json::json!({
                "name": name,
                "offset": i as u32 * param_bytes,
                "width": param_bytes,
            })
        })
        .collect();
    assert_eq!(
        params.len() as u32 * param_bytes,
        input_binding.stride,
        "the derived params[] must exactly cover the Input span (offsets 0..{})",
        input_binding.stride
    );

    let ctl_args = parse_fn_params(CTL_SOURCE, CTL_NAME);
    assert_eq!(
        ctl_args.len(),
        6,
        "update_rotor must have 6 params (rotor quad + dx, dy); parsed {ctl_args:?}"
    );
    // The positional result-order/param-order contract: the FIRST FOUR control args
    // ARE the rotor quad, and update_rotor's 4-value reply is in that same order,
    // which IS the fragment's broadcast-param order. Asserted, not assumed.
    assert_eq!(
        &ctl_args[0..4],
        rotor_names.as_slice(),
        "the control's first four args {:?} must equal the fragment's broadcast rotor names {:?} \
         (the positional result-order == param-order contract)",
        &ctl_args[0..4],
        rotor_names
    );

    // The event map: which normalized DOM input feeds which update_rotor arg. The
    // first four args feed back the stored rotor; the last two are drag-derived
    // (pointer movement). No wheel: this control rotates, it does not zoom.
    let event_map = serde_json::json!({
        &ctl_args[0]: { "source": "view", "index": 0 },
        &ctl_args[1]: { "source": "view", "index": 1 },
        &ctl_args[2]: { "source": "view", "index": 2 },
        &ctl_args[3]: { "source": "view", "index": 3 },
        &ctl_args[4]: { "source": "pointer", "field": "movementX", "when": "drag" },
        &ctl_args[5]: { "source": "pointer", "field": "movementY", "when": "drag" },
    });

    // --- 5. The pinned-rotor oracle: run frag.wasm over 512x512 at each pinned
    // rotor, assert every packed-RGBA word == the in-generator oracle, fold FNV. ---
    let mut references = Vec::new();
    for (name, rc, r12, r13, r23, min_distinct) in ROTOR_PINS {
        let frame = run_wasm_frag(&frag_wasm, WIDTH, HEIGHT, rc, r12, r13, r23);
        let mut distinct = std::collections::HashSet::new();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let got = frame[(y * WIDTH + x) as usize];
                let want = clifford_frag_rgba_oracle(x as i32, y as i32, rc, r12, r13, r23);
                if got != want {
                    panic!(
                        "HARD FAIL: wasmtime {FRAG_NAME}({x},{y}; {rc},{r12},{r13},{r23}) = \
                         0x{got:08x}, oracle = 0x{want:08x} [rotor {name}]. Refusing to emit a \
                         stale/faked reference; the kernel or a backend is off-pin."
                    );
                }
                distinct.insert(got);
            }
        }
        assert!(
            distinct.len() >= min_distinct,
            "rotor {name}: rendered color histogram must have >= {min_distinct} distinct colors \
             (got {})",
            distinct.len()
        );
        let hash = fnv1a32(&frame);
        eprintln!(
            "  oracle [{name} = ({rc},{r12},{r13},{r23})]: all {} packed-RGBA words == \
             clifford_frag_rgba_oracle; {} distinct colors; FNV-1a-32 = {hash} (0x{hash:08x})",
            WIDTH * HEIGHT,
            distinct.len()
        );
        references.push(serde_json::json!({
            "name": name,
            "rotor": [rc, r12, r13, r23],
            "fnv1a32": hash,
            "distinct_colors": distinct.len(),
        }));
    }

    // --- 6. Serialize + write (only after every gate passed). ---------------
    let layout_json = serialize_render_layout(&artifact.layout, params, frag_wasm.len());
    let ctl_json = serde_json::to_string_pretty(&serde_json::json!({
        "module": "ctl.wasm",
        "control_export": CTL_NAME,
        "args": ctl_args,
        "arg_types": vec!["i32"; ctl_args.len()],
        "result_types": vec!["i32"; 4],
        // The 4-value reply order IS the fragment's broadcast-param order (asserted).
        "result_order": rotor_names,
        // The first `view_arg_count` args are the fed-back stored rotor quad.
        "view_arg_count": 4,
        "view_init": [ROTOR_INIT.0, ROTOR_INIT.1, ROTOR_INIT.2, ROTOR_INIT.3],
        "event_map": event_map,
        "wasm_bytes": ctl_wasm.len(),
        "provenance": provenance("cargo run -p fe-codegen --example gen_clifford_interactive_demo"),
    }))
    .expect("ctl.json should serialize");
    let reference_json = serde_json::to_string_pretty(&serde_json::json!({
        "fragment": FRAG_NAME,
        "width": WIDTH,
        "height": HEIGHT,
        "runtime": "wasmtime (Fe -> wasm), executed at generation time",
        "views": references,
    }))
    .expect("reference.json should serialize");

    write_file(&gen_dir.join("kernel.fe"), FRAG_SOURCE.as_bytes());
    write_file(&gen_dir.join("ctl.fe"), CTL_SOURCE.as_bytes());
    write_file(&gen_dir.join("frag.wgsl"), wgsl.as_bytes());
    write_file(&gen_dir.join("layout.json"), layout_json.as_bytes());
    write_file(&gen_dir.join("frag.wasm"), &frag_wasm);
    write_file(&gen_dir.join("ctl.wasm"), &ctl_wasm);
    write_file(&gen_dir.join("ctl.json"), ctl_json.as_bytes());
    write_file(&gen_dir.join("reference.json"), reference_json.as_bytes());

    eprintln!(
        "gen_clifford_interactive_demo: wrote 8 files to {}\n  \
         kernel.fe  ctl.fe  frag.wgsl  layout.json  frag.wasm  ctl.wasm  ctl.json  reference.json",
        gen_dir.display()
    );
    eprintln!(
        "  serve: `cd {} && ./serve.sh` then open \
         http://localhost:8788/webgpu-clifford-interactive/",
        repo_root.join("demos").display()
    );
}

/// The browser-profile WGSL gate (static, GPU-free): (1) no 64-bit scalar token,
/// (2) naga `wgsl-in` round-trips it, (3) it validates under
/// `Capabilities::default()` (the browser set, no SHADER_INT64). Any failure panics.
fn assert_browser_profile_wgsl(wgsl: &str) {
    for tok in ["i64", "u64"] {
        assert!(
            !wgsl.contains(tok),
            "browser-profile WGSL must contain no `{tok}` scalar token; found one in:\n{wgsl}"
        );
    }
    let reparsed = naga::front::wgsl::parse_str(wgsl)
        .unwrap_or_else(|e| panic!("naga wgsl-in should reparse the emitted WGSL: {e:?}\n{wgsl}"));
    let caps = naga::valid::Capabilities::default();
    assert!(
        !caps.contains(naga::valid::Capabilities::SHADER_INT64),
        "the browser capability set must exclude SHADER_INT64"
    );
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps)
        .validate(&reparsed)
        .unwrap_or_else(|e| {
            panic!("browser-profile validation (no SHADER_INT64) should accept the WGSL: {e:?}")
        });
}

/// Compile a Fe source string to wasm bytecode through `BackendKind::Wasm`.
fn compile_to_wasm(source: &str, tag: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{tag}.fe")).expect("wasm gen URL should parse");
    db.workspace().touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).expect("wasm gen file should load");
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WasmTy {
    I32,
    Other,
}

/// Parse the emitted wasm with wasmparser and return the named export's
/// (params, results) as a coarse i32/other signature.
fn export_signature(bytes: &[u8], export_name: &str) -> (Vec<WasmTy>, Vec<WasmTy>) {
    use wasmparser::{ExternalKind, Payload, TypeRef, ValType};

    let map = |v: &ValType| if matches!(v, ValType::I32) { WasmTy::I32 } else { WasmTy::Other };

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

/// Run the Fe render fragment (the 6-arg typed func) over the FULL grid for one
/// rotor, returning the per-pixel packed RGBA8 grid (row-major, u32).
fn run_wasm_frag(bytes: &[u8], width: u32, height: u32, rc: i32, r12: i32, r13: i32, r23: i32) -> Vec<u32> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(i32, i32, i32, i32, i32, i32), i32>(&mut store, FRAG_NAME)
        .expect("`clifford_frag_rgba` export should exist as (i32 x6) -> i32");
    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let v = f
                .call(&mut store, (x, y, rc, r12, r13, r23))
                .expect("clifford_frag_rgba should run") as u32;
            out.push(v);
        }
    }
    out
}

/// FNV-1a 32-bit over the frame's little-endian byte stream (mirrored bit-for-bit
/// in the page's JS: Math.imul + `>>> 0`).
fn fnv1a32(frame: &[u32]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &v in frame {
        for byte in v.to_le_bytes() {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    hash
}

/// Serialize the compiler-stated Render `SpirvLayout` + the POPULATED params[] to
/// the `layout.json` the kernel-blind render runner consumes.
fn serialize_render_layout(
    layout: &SpirvLayout,
    params: Vec<serde_json::Value>,
    frag_wasm_len: usize,
) -> String {
    let bindings: Vec<serde_json::Value> = layout
        .bindings
        .iter()
        .map(|b| {
            serde_json::json!({
                "group": b.group,
                "binding": b.binding,
                "name": b.name,
                "access": access_str(b.access),
                "role": role_str(b.role),
                "stride": b.stride,
            })
        })
        .collect();

    assert!(
        layout.result.is_none(),
        "serialize_render_layout expects layout.result == None (the color target is the result)"
    );

    let value = serde_json::json!({
        "kernel": FRAG_NAME,
        "entry_point": layout.entry_point,
        "mode": "Render",
        "vertex_entry": layout.vertex_entry,
        "fragment_entry": layout.fragment_entry,
        "color_target_format": layout.color_target_format,
        "word": word_str(layout.word),
        "word_bytes": layout.word.width_bytes(),
        "bindings": bindings,
        // POPULATED (not `[]`): the four broadcast rotor members with byte offsets,
        // derived from the Input span + the fragment signature.
        "params": params,
        "frag_wasm_export": FRAG_NAME,
        "frag_wasm_bytes": frag_wasm_len,
        "width": WIDTH,
        "height": HEIGHT,
        "provenance": provenance("cargo run -p fe-codegen --example gen_clifford_interactive_demo"),
    });

    serde_json::to_string_pretty(&value).expect("layout.json should serialize")
}

fn provenance(generator: &str) -> serde_json::Value {
    serde_json::json!({
        "source": "Fe compiler (branch mb2)",
        "fe_rev": fe_head_rev(),
        "sonatina_rev": SONATINA_REV,
        "generator": generator,
        "generated_unix_secs": SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

fn access_str(a: Access) -> &'static str {
    match a {
        Access::Read => "Read",
        Access::ReadWrite => "ReadWrite",
    }
}

fn role_str(r: Role) -> &'static str {
    match r {
        Role::Output => "Output",
        Role::Input => "Input",
    }
}

fn word_str(w: WordKind) -> &'static str {
    match w {
        WordKind::U32 => "U32",
        WordKind::I64 => "I64",
    }
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
