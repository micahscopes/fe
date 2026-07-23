//! I3 page generator: assemble the INTERACTIVE all-Fe mandelbrot page inputs
//! under `demos/webgpu-mandelbrot-interactive/gen/`.
//!
//! Two Fe units, both compiled through the REAL Fe drivers (never hand-written):
//!   * `mandel_view_frag` (the RENDER fragment): compiled through the Render seam
//!     `compile_runtime_package_spirv_render` -> naga WGSL + a Render `SpirvLayout`.
//!     The three broadcast view members (center_re, center_im, scale_q) ride the
//!     Input struct at `@group(0) @binding(1)`, span 12. Also compiled to wasm for
//!     the AMBER per-pixel path + the pinned-view oracle.
//!   * `mandel_view_ctl` (the CONTROLS): compiled to wasm. `update_view` is the
//!     pan/zoom control fn; its export interface is emitted as `ctl.json` and
//!     GATED by a wasmparser signature check of the actual wasm export.
//!
//! | file             | produced by                                                    |
//! |------------------|----------------------------------------------------------------|
//! | `kernel.fe`      | verbatim copy of the SSOT fragment fixture (page panel)         |
//! | `ctl.fe`         | verbatim copy of the SSOT control fixture (page panel)          |
//! | `frag.wgsl`      | the naga-emitted WGSL from the Render SPIR-V artifact           |
//! | `layout.json`    | the compiler-stated Render `SpirvLayout` with POPULATED params |
//! | `frag.wasm`      | `mandel_view_frag` -> wasm (the amber per-pixel + oracle leg)   |
//! | `ctl.wasm`       | `mandel_view_ctl` -> wasm (the update_view controls)           |
//! | `ctl.json`       | the wasmparser-gated update_view export interface + event map  |
//! | `reference.json` | FNV-1a-32 of the RGBA stream at each pinned view (wasmtime)     |
//!
//! HARD-FAIL discipline (EVERY gate passes before a single file is written):
//!   * browser-profile WGSL: no 64-bit scalar token, naga `wgsl-in` reparse,
//!     validation under `Capabilities::default()` (no SHADER_INT64); plus the
//!     render epilogue (`@vertex` + `@fragment` + `@location(0)` + `unpack4x8unorm`),
//!     the honest escape evidence (`loop` + `bitcast<i32>`), and the three-member
//!     broadcast load (`input.p0` .. `input.p2`);
//!   * layout is Render / word U32 / result None / vertex `vs_fullscreen` /
//!     fragment `fs_main` / color target `rgba8unorm` / Input binding stride 12;
//!   * the `frag.wasm` export signature is EXACTLY `(i32, i32, i32, i32, i32) ->
//!     i32` (2 coords + 3 broadcast view members), proving `params.len() == 3`;
//!   * the `ctl.wasm` `update_view` export signature is EXACTLY
//!     `(i32 x8) -> (i32, i32, i32)` (parsed with wasmparser), which GATES the
//!     `ctl.json` arg/result interface;
//!   * the view-param ORACLE at the pinned views: instantiate `frag.wasm` under
//!     wasmtime, render every 512x512 pixel at each pinned view, and assert EVERY
//!     packed-RGBA word equals the in-generator oracle `mandel_view_frag_oracle`
//!     (re-derived here from the kernel arithmetic, never trusted from any doc),
//!     then fold each frame into an FNV-1a-32.
//! Any deviation panics before a single file is written.
//!
//! Run: `cargo run -p fe-codegen --example gen_mandelbrot_interactive_demo`

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    BackendKind, CanonicalCapability, CanonicalExecution, CanonicalPlacement, OptLevel,
    WebBuildOptions, WebBundle, WebCanonicalPolicy, compile_runtime_package_spirv_render,
    layout_for,
};
use sonatina_codegen::isa::spirv::{Access, LayoutMode, Role, SpirvLayout, WordKind};
use url::Url;

/// The SSOT fragment source: the exact fixture the I1 e2e tests `include_str!`, so
/// the tested source and the shipped source are byte-identical by construction.
const FRAG_SOURCE: &str = include_str!("../tests/fixtures/spirv/mandel_view_frag.fe");
/// The SSOT control source (the pan/zoom `update_view` fn).
const CTL_SOURCE: &str = include_str!("../tests/fixtures/spirv/mandel_view_ctl.fe");

/// The render fragment / wasm export name (the Fe `pub fn`).
const FRAG_NAME: &str = "mandel_view_frag";
/// The control export the page drives per input event.
const CTL_NAME: &str = "update_view";
const CTL_MESSAGE_NAME: &str = "update_view_message";
const RENDER_LANE: &str = "render";
const VERIFY_LANE: &str = "verify";

/// The dispatch frame: 512x512, the proven view resolution (spec section 2).
const WIDTH: u32 = 512;
const HEIGHT: u32 = 512;

/// The default view token (spec section 2): center (-0.5, 0) in Q12, scale 384
/// (= 24 Q12-units/px, the proven R1 fixed view). Emitted into `ctl.json` as the
/// initial view the pump seeds (the R1 regression anchor; NOT computed in JS).
const VIEW_INIT: (i32, i32, i32) = (-2048, 0, 384);

/// The sonatina fork rev the fe workspace is pinned to (`Cargo.toml`), the render
/// push #3 rev; recorded in `layout.json` provenance.
const SONATINA_REV: &str = "547519d46f9b6191881943fefb7cddd1880e77cf";

/// The pinned views the spec (section 7) names, each `(name, center_re, center_im,
/// scale_q, min_distinct)`. `min_distinct` is a DERIVED non-degeneracy floor: the
/// default and the two interior valleys show many colors; the clamp corner is a
/// fast-escape near-uniform patch, asserted `>= 2` so a one-color image still fails.
const VIEW_PINS: [(&str, i32, i32, i32, usize); 4] = [
    ("default", -2048, 0, 384, 10),
    ("seahorse", -3072, 410, 48, 10),
    ("ceiling", 1126, 29, 16, 10),
    ("clamp_corner", 10240, 10240, 384, 2),
];

/// The independent view-parameterized Q12 escape+color oracle, re-derived HERE from
/// the kernel logic (never trusted from the spec), integer-identical to the fixture:
/// the pixel->complex map is `center + (((p - 256) * scale_q) >> 4)` (arithmetic i32
/// `>>`), the escape body is the R1 silhouette, and the color is the same u32 ramp.
/// Returns the packed RGBA8 word (LE bytes = [R, G, B, A]).
fn mandel_view_frag_oracle(px: i32, py: i32, center_re: i32, center_im: i32, scale_q: i32) -> u32 {
    let c_re: i32 = center_re + (((px - 256) * scale_q) >> 4);
    let c_im: i32 = center_im + (((py - 256) * scale_q) >> 4);
    let mut zr: i32 = 0;
    let mut zi: i32 = 0;
    let mut i: u32 = 0;
    let mut color: u32 = 4_278_190_080;
    while i < 100 {
        let rr: i32 = zr * zr;
        let ii: i32 = zi * zi;
        let mag: i32 = rr + ii;
        if mag < 67_108_864 {
            let t: i32 = rr - ii;
            let nzi: i32 = ((zr * 2) * zi) >> 12;
            zr = (t >> 12) + c_re;
            zi = nzi + c_im;
            i += 1;
            let v: u32 = (i * 655) >> 8;
            color = v + v * 256 + (255 - v) * 65536 + 4_278_190_080;
        } else {
            return color;
        }
    }
    4_278_190_080
}

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/codegen should have a two-level ancestor (the repo root)");
    let demo_dir = repo_root.join("demos/webgpu-mandelbrot-interactive");
    let gen_dir = demo_dir.join("gen");
    std::fs::create_dir_all(&gen_dir)
        .unwrap_or_else(|e| panic!("could not create {}: {e}", gen_dir.display()));

    eprintln!(
        "gen_mandelbrot_interactive_demo: compiling `{FRAG_NAME}` (Render) + `{CTL_NAME}` (wasm) \
         through the real Fe drivers"
    );

    // --- 1. Fe -> SPIR-V/WGSL, RENDER mode (the fragment). ------------------
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///gen_mandel_view_frag.fe").expect("gen URL should parse");
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

    // HARD GATE 1: browser-profile WGSL + the render/fractal/broadcast tokens.
    assert_browser_profile_wgsl(&wgsl);
    for tok in [
        "@vertex",
        "@fragment",
        "@location(0)",
        "unpack4x8unorm",
        "loop",
        "bitcast<i32>",
        "input.p0",
        "input.p2",
    ] {
        assert!(
            wgsl.contains(tok),
            "render WGSL must contain `{tok}` (render epilogue / honest escape / 3-member \
             broadcast load); off-shape, refusing to emit:\n{wgsl}"
        );
    }
    eprintln!(
        "  WGSL passed the browser profile: no 64-bit tokens, wgsl-in reparse OK, validated with \
         Capabilities::default(); @vertex+@fragment+@location(0)+unpack4x8unorm, loop+bitcast<i32>, \
         input.p0..input.p2 (3 broadcast view members)"
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
        .expect("the render layout must carry an Input binding (the broadcast view)");
    assert_eq!(
        input_binding.stride, 12,
        "the THREE broadcast view members (4 bytes each) span 12 bytes; off-pin, refusing to emit"
    );
    eprintln!(
        "  layout: mode Render, word U32, result None, vertex vs_fullscreen, fragment fs_main, \
         color rgba8unorm, Input stride {}",
        input_binding.stride
    );

    // --- 2. Fe -> wasm (the fragment, for the amber per-pixel + oracle leg). --
    let frag_wasm = compile_to_wasm(FRAG_SOURCE, "gen_frag_wasm");
    // HARD GATE 3: the fragment export is EXACTLY (i32 x5) -> i32 (2 coords + 3
    // broadcast members). This is what makes `params.len() == 3` honest.
    let frag_sig = export_signature(&frag_wasm, FRAG_NAME);
    assert_eq!(
        frag_sig,
        (vec![WasmTy::I32; 5], vec![WasmTy::I32]),
        "render fragment export `{FRAG_NAME}` must be (i32 x5) -> i32 (px, py + 3 broadcast \
         view members); got {frag_sig:?}"
    );
    eprintln!(
        "  wasm export `{FRAG_NAME}` signature is exactly (i32 x5) -> i32 (2 coords + 3 view)"
    );

    // --- 3. Fe -> wasm (the controls). --------------------------------------
    let ctl_wasm = compile_to_wasm(CTL_SOURCE, "gen_ctl_wasm");
    // HARD GATE 4: the control export is EXACTLY (i32 x8) -> (i32, i32, i32).
    // This GATES the ctl.json interface below (it is not assumed, it is measured).
    let ctl_sig = export_signature(&ctl_wasm, CTL_NAME);
    assert_eq!(
        ctl_sig,
        (vec![WasmTy::I32; 8], vec![WasmTy::I32; 3]),
        "control export `{CTL_NAME}` must be (i32 x8) -> (i32, i32, i32) (the native wasm \
         multi-value view reply); got {ctl_sig:?}"
    );
    eprintln!("  wasm export `{CTL_NAME}` signature is exactly (i32 x8) -> (i32, i32, i32)");

    // The Worker-facing module is emitted by the same WebBundle path users invoke
    // from `fe web build`. Its interface.js is generated from the exact semantic
    // request/response records and the Wasm module contains the verified arena
    // plus `fe_cabi_update_view_message` wrapper.
    let combined_source = format!("{FRAG_SOURCE}\n{CTL_SOURCE}");
    let mut canonical_db = DriverDataBase::default();
    let canonical_url =
        Url::parse("file:///mandel_view_canonical.fe").expect("canonical URL should parse");
    canonical_db.workspace().touch(
        &mut canonical_db,
        canonical_url.clone(),
        Some(combined_source),
    );
    let canonical_file = canonical_db
        .workspace()
        .get(&canonical_db, &canonical_url)
        .expect("canonical source should load");
    let canonical_bundle = WebBundle::compile(
        &canonical_db,
        canonical_db.top_mod(canonical_file),
        WebBuildOptions::render(FRAG_NAME, Some("mandel_view_canonical.fe".to_owned()))
            .with_canonical_entries([CTL_MESSAGE_NAME, RENDER_LANE, VERIFY_LANE])
            .with_canonical_policy(WebCanonicalPolicy::Required),
    )
    .expect("Mandelbrot canonical control WebBundle must compile");
    let canonical_interface = canonical_bundle
        .manifest
        .canonical_interface
        .as_ref()
        .expect("required Mandelbrot canonical interface");
    for lane_name in [RENDER_LANE, VERIFY_LANE] {
        let lane = canonical_interface
            .lanes
            .iter()
            .find(|lane| lane.name == lane_name)
            .unwrap_or_else(|| panic!("missing generated `{lane_name}` lane"));
        assert_eq!(lane.intent.execution, CanonicalExecution::HostEffect);
        assert_eq!(lane.intent.placement, CanonicalPlacement::MainThread);
        assert_eq!(lane.intent.capabilities.len(), 1);
        assert_eq!(
            lane.intent.capabilities[0].capability,
            CanonicalCapability::WebgpuDispatch
        );
        assert!(lane.intent.capabilities[0].mutable);
        assert!(lane.export.is_none());
    }
    // --- 4. Derive the interfaces from the ACTUAL sources (not hardcoded). ---
    // The broadcast param names are the fragment's args 2..4 (parsed from source);
    // the control arg names are update_view's 8 params (parsed from source). The
    // wasm signature gates above prove the counts, so a source/signature drift
    // panics rather than emitting a mislabeled interface.
    let frag_params = parse_fn_params(FRAG_SOURCE, FRAG_NAME);
    assert_eq!(
        frag_params.len(),
        5,
        "the fragment signature must have 5 params (px, py + 3 view); parsed {frag_params:?}"
    );
    let view_names: Vec<String> = frag_params[2..5].to_vec();
    // Offsets from the Input span (0, 4, 8), widths from the u32 word (4).
    let param_bytes = artifact.layout.word.width_bytes();
    let params: Vec<serde_json::Value> = view_names
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
        8,
        "update_view must have 8 params (view triple + dx,dy,dzoom + mx,my); parsed {ctl_args:?}"
    );
    // The positional result-order/param-order contract (spec section 4): the FIRST
    // THREE control args ARE the view triple, and update_view's 3-value reply is in
    // that same order, which IS the fragment's broadcast-param order. Asserted, not
    // assumed: the control's first three arg names must equal the fragment's three
    // broadcast view names.
    assert_eq!(
        &ctl_args[0..3],
        view_names.as_slice(),
        "the control's first three args {:?} must equal the fragment's broadcast view names {:?} \
         (the positional result-order == param-order contract)",
        &ctl_args[0..3],
        view_names
    );

    // The event map: which normalized DOM input feeds which update_view arg. The
    // first three args feed back the stored view triple; the last five are event-
    // derived. This is DATA the demo-blind pump reads, never hand-synced in JS.
    let event_map = serde_json::json!({
        &ctl_args[0]: { "source": "view", "index": 0 },
        &ctl_args[1]: { "source": "view", "index": 1 },
        &ctl_args[2]: { "source": "view", "index": 2 },
        &ctl_args[3]: { "source": "pointer", "field": "movementX", "when": "drag" },
        &ctl_args[4]: { "source": "pointer", "field": "movementY", "when": "drag" },
        &ctl_args[5]: { "source": "wheel", "field": "deltaYSign" },
        &ctl_args[6]: { "source": "pointer", "field": "offsetX" },
        &ctl_args[7]: { "source": "pointer", "field": "offsetY" },
    });

    // --- 5. The pinned-view oracle: run frag.wasm over 512x512 at each pinned
    // view, assert every packed-RGBA word == the in-generator oracle, fold FNV. ---
    let mut references = Vec::new();
    for (name, cr, ci, sq, min_distinct) in VIEW_PINS {
        let frame = run_wasm_frag(&frag_wasm, WIDTH, HEIGHT, cr, ci, sq);
        let mut distinct = std::collections::HashSet::new();
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let got = frame[(y * WIDTH + x) as usize];
                let want = mandel_view_frag_oracle(x as i32, y as i32, cr, ci, sq);
                if got != want {
                    panic!(
                        "HARD FAIL: wasmtime {FRAG_NAME}({x},{y}; {cr},{ci},{sq}) = 0x{got:08x}, \
                         oracle = 0x{want:08x} [view {name}]. Refusing to emit a stale/faked \
                         reference; the kernel or a backend is off-pin."
                    );
                }
                distinct.insert(got);
            }
        }
        assert!(
            distinct.len() >= min_distinct,
            "view {name}: rendered color histogram must have >= {min_distinct} distinct colors \
             (got {})",
            distinct.len()
        );
        let hash = fnv1a32(&frame);
        eprintln!(
            "  oracle [{name} = ({cr},{ci},{sq})]: all {} packed-RGBA words == mandel_view_frag_oracle; \
             {} distinct colors; FNV-1a-32 = {hash} (0x{hash:08x})",
            WIDTH * HEIGHT,
            distinct.len()
        );
        references.push(serde_json::json!({
            "name": name,
            "view": [cr, ci, sq],
            "fnv1a32": hash,
            "distinct_colors": distinct.len(),
        }));
    }

    // --- 6. Serialize + write (only after every gate passed). ---------------
    let layout_json = serialize_render_layout(&artifact.layout, params, frag_wasm.len());
    let ctl_json = serde_json::to_string_pretty(&serde_json::json!({
        "module": "ctl.wasm",
        "canonical_bundle": "actor/manifest.json",
        "canonical_module": "actor/module.wasm",
        "canonical_interface": "actor/interface.js",
        "canonical_lane": CTL_MESSAGE_NAME,
        "control_export": CTL_NAME,
        "args": ctl_args,
        "arg_types": vec!["i32"; ctl_args.len()],
        "result_types": vec!["i32"; 3],
        // The 3-value reply order IS the fragment's broadcast-param order (asserted).
        "result_order": view_names,
        // The first `view_arg_count` args are the fed-back stored view triple.
        "view_arg_count": 3,
        "view_init": [VIEW_INIT.0, VIEW_INIT.1, VIEW_INIT.2],
        "event_map": event_map,
        "wasm_bytes": ctl_wasm.len(),
        "canonical_wasm_bytes": canonical_bundle.wasm.len(),
        "provenance": provenance("cargo run -p fe-codegen --example gen_mandelbrot_interactive_demo"),
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
    for file in canonical_bundle
        .materialized_files()
        .expect("Mandelbrot actor WebBundle materializes")
    {
        write_file(&gen_dir.join("actor").join(file.path()), file.bytes());
    }
    write_file(&gen_dir.join("ctl.json"), ctl_json.as_bytes());
    write_file(&gen_dir.join("reference.json"), reference_json.as_bytes());

    eprintln!(
        "gen_mandelbrot_interactive_demo: wrote application files and canonical actor/ WebBundle to {}\n  \
         kernel.fe  ctl.fe  frag.wgsl  layout.json  frag.wasm  ctl.wasm  \
         actor/  ctl.json  reference.json",
        gen_dir.display()
    );
    eprintln!(
        "  serve: `cd {} && ./serve.sh` then open \
         http://localhost:8788/webgpu-mandelbrot-interactive/",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WasmTy {
    I32,
    Other,
}

/// Parse the emitted wasm with wasmparser and return the named export's
/// (params, results) as a coarse i32/other signature. Used to GATE the emitted
/// interfaces against the ACTUAL compiled artifact.
fn export_signature(bytes: &[u8], export_name: &str) -> (Vec<WasmTy>, Vec<WasmTy>) {
    use wasmparser::{ExternalKind, Payload, TypeRef, ValType};

    let map = |v: &ValType| {
        if matches!(v, ValType::I32) {
            WasmTy::I32
        } else {
            WasmTy::Other
        }
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
/// Honest interface derivation: the param names shown on the page (kernel.fe/ctl.fe)
/// ARE the names emitted into layout.json/ctl.json, so a drift is visible + gated.
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

/// Run the Fe render fragment (the 5-arg typed func) over the FULL grid for one
/// view, returning the per-pixel packed RGBA8 grid (row-major, u32).
fn run_wasm_frag(bytes: &[u8], width: u32, height: u32, cr: i32, ci: i32, sq: i32) -> Vec<u32> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(i32, i32, i32, i32, i32), i32>(&mut store, FRAG_NAME)
        .expect("`mandel_view_frag` export should exist as (i32 x5) -> i32");
    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height as i32 {
        for x in 0..width as i32 {
            let v = f
                .call(&mut store, (x, y, cr, ci, sq))
                .expect("mandel_view_frag should run") as u32;
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
/// the `layout.json` the kernel-blind render runner consumes. Render deltas vs the
/// Grid schema: `mode: "Render"`, `result` absent, `vertex_entry`/`fragment_entry`/
/// `color_target_format` present, and `params` = the three broadcast view members.
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
        // POPULATED (not `[]`): the three broadcast view members with byte offsets,
        // derived from the Input span + the fragment signature.
        "params": params,
        "frag_wasm_export": FRAG_NAME,
        "frag_wasm_bytes": frag_wasm_len,
        "width": WIDTH,
        "height": HEIGHT,
        "provenance": provenance("cargo run -p fe-codegen --example gen_mandelbrot_interactive_demo"),
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
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .unwrap_or_else(|e| panic!("could not create {}: {e}", parent.display()));
    }
    std::fs::write(path, bytes)
        .unwrap_or_else(|e| panic!("could not write {}: {e}", path.display()));
}
