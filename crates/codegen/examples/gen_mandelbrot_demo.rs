//! M1c page generator: compile the `grid_gradient_u32` GRID kernel through the
//! REAL Fe drivers and emit the first-Fe-computed-IMAGE page inputs under
//! `demos/webgpu-mandelbrot/gen/`.
//!
//! One grid invocation per pixel, `v = px + 1024 * py`. Two delivery mechanisms,
//! one Fe function: the SPIR-V leg dispatches it (gid.xy arriving as args 0,1,
//! driver-declared Grid envelope, layout-stated); the wasm leg CALLS it per pixel
//! with explicit `(px, py)`. Every artifact here is produced by the compiler,
//! never hand-written:
//!
//! | file             | produced by                                                   |
//! |------------------|----------------------------------------------------------------|
//! | `kernel.fe`      | verbatim copy of the SSOT fixture (byte-identical to the test) |
//! | `kernel.wgsl`    | the naga-emitted WGSL from the Grid SPIR-V artifact            |
//! | `layout.json`    | the compiler-stated `SpirvLayout` (Grid schema, result absent)|
//! | `kernel.wasm`    | the Fe -> wasm build (`BackendKind::Wasm`)                     |
//! | `reference.json` | width/height/FNV-1a-32/samples from EXECUTING kernel.wasm here |
//!
//! HARD-FAIL discipline (EVERY gate passes before a single file is written):
//!   * browser-profile WGSL: no 64-bit scalar token, naga `wgsl-in` reparse, and
//!     validation under `Capabilities::default()` (no SHADER_INT64); plus the
//!     Grid shader must reference `global_invocation_id` and `num_workgroups`;
//!   * layout is Grid / word U32 / workgroup `[8, 8, 1]` / `result` None / the
//!     output binding stride is 4 (bytes per element);
//!   * the `kernel.wasm` export signature is EXACTLY `(i32, i32) -> i32` (parsed
//!     with wasmparser) - two gid args, zero broadcast, proving `params: []`;
//!   * reference execution: instantiate `kernel.wasm` under wasmtime, loop the
//!     export over all 512x512 `(px, py)`, and assert EVERY value equals the
//!     in-generator oracle `|x, y| x + 1024 * y` (re-derived here, never trusted
//!     from any doc), then fold the grid into an FNV-1a-32.
//! Any deviation panics before a single file is written.
//!
//! Run: `cargo run -p fe-codegen --example gen_mandelbrot_demo`

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, compile_runtime_package_spirv_grid, layout_for};
use sonatina_codegen::isa::spirv::{Access, LayoutMode, Role, SpirvLayout, WordKind};
use url::Url;

/// The SSOT grid kernel source: the exact fixture the M1b e2e test `include_str!`s,
/// so the tested source and the shipped source are byte-identical by construction.
const KERNEL_SOURCE: &str = include_str!("../tests/fixtures/spirv/grid_gradient_u32.fe");

/// The kernel/export name (the Fe `pub fn`), used for the wasm export lookup and
/// stated in `layout.json` so `wasm-runner.js` calls the right export.
const KERNEL_NAME: &str = "grid_gradient_u32";

/// The sonatina fork rev the fe workspace is pinned to (`Cargo.toml`), recorded
/// in `layout.json` provenance so the page can show Fe -> these files.
const SONATINA_REV: &str = "41541f6b2646dfdff23b8b4539691376b9137993";

/// Grid dispatch: 8x8 workgroups tile the 2D frame. `row_width =
/// num_workgroups.x * workgroup_size[0]` is the Grid mode CONTRACT shared by the
/// translator and the runner; the page supplies W/H at dispatch time.
const WORKGROUP: [u32; 3] = [8, 8, 1];

/// The page's dispatch frame. Both are multiples of the workgroup dims (exact
/// tiling, the v1 requirement). 511 + 1024*511 = 523_775 < 2^31: no overflow,
/// no signedness edge on any backend.
const WIDTH: u32 = 512;
const HEIGHT: u32 = 512;

/// The oracle, re-derived HERE and NEVER trusted from any doc: the grid gradient
/// packs the coordinates base-1024, `v = x + 1024 * y`. Injective for any width
/// <= 1024 and asymmetric, so a transpose/flip/stride bug diverges.
fn grid_gradient_oracle(x: u32, y: u32) -> u32 {
    x + 1024 * y
}

fn main() {
    // gen/ lives at <repo-root>/demos/webgpu-mandelbrot/gen. Resolve it from the
    // crate manifest dir (crates/codegen), robust to the invocation cwd.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/codegen should have a two-level ancestor (the repo root)");
    let demo_dir = repo_root.join("demos/webgpu-mandelbrot");
    let gen_dir = demo_dir.join("gen");
    std::fs::create_dir_all(&gen_dir)
        .unwrap_or_else(|e| panic!("could not create {}: {e}", gen_dir.display()));

    eprintln!("gen_mandelbrot_demo: compiling `{KERNEL_NAME}` (GRID) through the real Fe drivers");

    // --- 1. Fe -> SPIR-V (naga), GRID mode at workgroup [8, 8, 1]. -----------
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///gen_grid_gradient_u32.fe").expect("gen URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(KERNEL_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("gen file should load");
    let top_mod = db.top_mod(file);

    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("grid kernel should build a wasm runtime package");
    let artifact = compile_runtime_package_spirv_grid(&db, &package, WORKGROUP)
        .expect("grid kernel should compile Fe -> naga-validated SPIR-V (Grid mode)");

    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the u32 grid kernel")
        .clone();

    // HARD GATE 1: browser-profile WGSL (no 64-bit tokens, wgsl-in reparse,
    // validate with Capabilities::default() - no SHADER_INT64), plus the Grid
    // shader must reference the gid + num_workgroups builtins.
    assert_browser_profile_wgsl(&wgsl);
    for tok in ["global_invocation_id", "num_workgroups"] {
        assert!(
            wgsl.contains(tok),
            "Grid WGSL must reference the `{tok}` builtin; off-shape, refusing to emit:\n{wgsl}"
        );
    }
    eprintln!(
        "  WGSL passed the browser profile: no 64-bit tokens, wgsl-in reparse OK, \
         validated with Capabilities::default(); references global_invocation_id + num_workgroups"
    );

    // HARD GATE 2: Grid layout as the runner assumes (word, mode, workgroup,
    // result None, per-element output stride).
    assert_eq!(
        artifact.layout.word,
        WordKind::U32,
        "the grid kernel must lower to a Uint word (WordKind::U32); off-pin, refusing to emit"
    );
    assert_eq!(
        artifact.layout.mode,
        LayoutMode::Grid,
        "the mandelbrot page must be Grid-mode (whole-array output); off-pin, refusing to emit"
    );
    assert_eq!(
        artifact.layout.workgroup_size, WORKGROUP,
        "layout workgroup must be {WORKGROUP:?}; off-pin, refusing to emit"
    );
    assert!(
        artifact.layout.result.is_none(),
        "Grid mode has no single-slot result (the whole output array is the result); \
         off-shape, refusing to emit"
    );
    let out_binding = artifact
        .layout
        .bindings
        .iter()
        .find(|b| matches!(b.role, Role::Output))
        .expect("the grid layout must carry an Output binding");
    assert_eq!(
        out_binding.stride, 4,
        "the u32 grid output stride must be 4 bytes per element; off-pin, refusing to emit"
    );
    eprintln!(
        "  layout: mode Grid, word U32, workgroup {WORKGROUP:?}, result None, output stride 4"
    );

    // --- 2. Fe -> wasm (the in-browser wasm lane's module). -----------------
    let wasm_bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("grid kernel should compile Fe -> wasm")
        .into_bytecode()
        .expect("wasm output should be bytecode");

    // HARD GATE 3: the export signature is EXACTLY (i32, i32) -> i32. Two args ==
    // two gid args == zero broadcast params, which is what makes `params: []`
    // honest. Parsed straight from the emitted bytes with wasmparser.
    assert_export_signature_i32_i32_to_i32(&wasm_bytes, KERNEL_NAME);
    eprintln!("  wasm export `{KERNEL_NAME}` signature is exactly (i32, i32) -> i32 (params: [])");

    // --- 3. Reference execution: run kernel.wasm over the whole frame. -------
    // Compute-don't-bake: the page's reference comes from a REAL execution here,
    // asserted equal to the in-generator oracle at every pixel. No hand-typed
    // grid value ever reaches the page.
    let grid = run_wasm_grid(&wasm_bytes, WIDTH, HEIGHT);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let got = grid[(y * WIDTH + x) as usize];
            let want = grid_gradient_oracle(x, y);
            if got != want {
                panic!(
                    "HARD FAIL: wasmtime grid_gradient_u32({x}, {y}) = {got}, oracle = {want}. \
                     Refusing to emit a stale/faked reference; the kernel or a backend is off-pin."
                );
            }
        }
    }
    let hash = fnv1a32(&grid);
    eprintln!(
        "  wasmtime-executed {WIDTH}x{HEIGHT} grid: all {} pixels == oracle x + 1024*y; \
         FNV-1a-32 = {hash} (0x{hash:08x})",
        WIDTH * HEIGHT
    );

    // --- 4. Write gen/ (only after every gate passed). ----------------------
    // Samples recomputed FROM the executed grid, never copied from a doc.
    let sample_coords: [(u32, u32); 5] =
        [(0, 0), (511, 0), (0, 511), (511, 511), (256, 256)];
    let samples: Vec<serde_json::Value> = sample_coords
        .iter()
        .map(|&(x, y)| {
            serde_json::json!({ "x": x, "y": y, "value": grid[(y * WIDTH + x) as usize] })
        })
        .collect();

    let layout_json = serialize_layout(&artifact.layout, &wasm_bytes.len());
    let reference_json = serde_json::to_string_pretty(&serde_json::json!({
        "kernel": KERNEL_NAME,
        "width": WIDTH,
        "height": HEIGHT,
        "fnv1a32": hash,
        "samples": samples,
        "runtime": "wasmtime (Fe -> wasm), executed at generation time",
    }))
    .expect("reference.json should serialize");

    write_file(&gen_dir.join("kernel.fe"), KERNEL_SOURCE.as_bytes());
    write_file(&gen_dir.join("kernel.wgsl"), wgsl.as_bytes());
    write_file(&gen_dir.join("layout.json"), layout_json.as_bytes());
    write_file(&gen_dir.join("kernel.wasm"), &wasm_bytes);
    write_file(&gen_dir.join("reference.json"), reference_json.as_bytes());

    eprintln!(
        "gen_mandelbrot_demo: wrote 5 files to {}\n  \
         kernel.fe  kernel.wgsl  layout.json  kernel.wasm  reference.json",
        gen_dir.display()
    );
    eprintln!(
        "  serve: `cd {} && ./serve.sh` then open http://localhost:8788/webgpu-mandelbrot/",
        repo_root.join("demos").display()
    );
}

/// The browser-profile WGSL gate (static, GPU-free), verbatim in spirit with the
/// keystone generator and the B2 e2e test: (1) no 64-bit scalar token, (2) naga
/// `wgsl-in` round-trips it, (3) it validates under `Capabilities::default()`
/// (the browser set, no SHADER_INT64). Any failure hard-panics.
fn assert_browser_profile_wgsl(wgsl: &str) {
    for tok in ["i64", "u64"] {
        assert!(
            !wgsl.contains(tok),
            "browser-profile WGSL must contain no `{tok}` scalar token; found one in:\n{wgsl}"
        );
    }
    assert!(
        wgsl.contains("u32"),
        "u32 grid WGSL should use the `u32` scalar; got:\n{wgsl}"
    );

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
            panic!("browser-profile validation (no SHADER_INT64) should accept the u32 WGSL: {e:?}")
        });
}

/// Parse the emitted wasm with wasmparser and assert the named export's function
/// signature is EXACTLY `(i32, i32) -> i32`. This proves the kernel takes only
/// the two gid args and zero broadcast params, which is what makes the
/// `layout.json` `params: []` honest (not assumed).
fn assert_export_signature_i32_i32_to_i32(bytes: &[u8], export_name: &str) {
    use wasmparser::{ExternalKind, Payload, TypeRef, ValType};

    wasmparser::validate(bytes).expect("Fe-emitted wasm should be valid");

    // Function signatures indexed by type index (params, results), owned so no
    // borrow of the transient reader outlives the loop.
    let mut func_sigs: Vec<(Vec<ValType>, Vec<ValType>)> = Vec::new();
    // Defined-func index -> type index.
    let mut func_type_indices: Vec<u32> = Vec::new();
    // Func exports share an index space with func imports; count imports so the
    // export index resolves to the right defined function.
    let mut imported_func_count: u32 = 0;
    let mut export_func_index: Option<u32> = None;

    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        match payload.expect("valid wasm payload") {
            Payload::TypeSection(reader) => {
                for rec in reader {
                    let rec = rec.expect("valid rec group");
                    for sub in rec.into_types() {
                        let ft = sub.unwrap_func();
                        func_sigs.push((ft.params().to_vec(), ft.results().to_vec()));
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    let import = import.expect("valid import entry");
                    if let TypeRef::Func(_) = import.ty {
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
        "export `{export_name}` (index {fidx}) resolves into the imported range; \
         a grid kernel is a defined function"
    );
    let defined = (fidx - imported_func_count) as usize;
    let tyidx = *func_type_indices
        .get(defined)
        .unwrap_or_else(|| panic!("no function-section entry for defined func {defined}"))
        as usize;
    let (params, results) = func_sigs
        .get(tyidx)
        .unwrap_or_else(|| panic!("no type-section entry for type index {tyidx}"));

    assert_eq!(
        params.as_slice(),
        [ValType::I32, ValType::I32].as_slice(),
        "grid export `{export_name}` params must be exactly (i32, i32) (two gid args, \
         zero broadcast); got {params:?}"
    );
    assert_eq!(
        results.as_slice(),
        [ValType::I32].as_slice(),
        "grid export `{export_name}` result must be exactly (i32); got {results:?}"
    );
}

/// Execute the grid export over the whole `width x height` frame, row-major,
/// under wasmtime. Fe `u32` lowers to wasm `i32`, so the export returns `i32`;
/// reinterpret as `u32` (`as u32`, mirroring the M1b e2e leg).
fn run_wasm_grid(bytes: &[u8], width: u32, height: u32) -> Vec<u32> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, KERNEL_NAME)
        .unwrap_or_else(|e| panic!("`{KERNEL_NAME}` export should exist as (i32, i32) -> i32: {e}"));
    let mut out = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let v = f
                .call(&mut store, (x as i32, y as i32))
                .unwrap_or_else(|e| panic!("{KERNEL_NAME}({x}, {y}) should run: {e}"))
                as u32;
            out.push(v);
        }
    }
    out
}

/// FNV-1a 32-bit over the grid's little-endian byte stream, folded over each
/// u32's 4 LE bytes in pixel order. Pinned exactly (offset 0x811c9dc5, prime
/// 0x01000193) and mirrored bit-for-bit in the page's JS (Math.imul + `>>> 0`).
fn fnv1a32(grid: &[u32]) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for &v in grid {
        for byte in v.to_le_bytes() {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    hash
}

/// Serialize the compiler-stated Grid `SpirvLayout` to the `layout.json` the
/// kernel-blind runner consumes. Grid schema deltas vs the scalar layout:
/// `mode: "Grid"`, the `result` key is ABSENT (`layout.result` is None), and a
/// `params` table (empty for M1's zero-broadcast gradient) names args 2.. of the
/// Fe signature.
fn serialize_layout(layout: &SpirvLayout, wasm_len: &usize) -> String {
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
        "serialize_layout(Grid) expects layout.result == None; the `result` key is absent"
    );

    let value = serde_json::json!({
        "kernel": KERNEL_NAME,
        "entry_point": layout.entry_point,
        "mode": mode_str(layout.mode),
        "workgroup_size": layout.workgroup_size,
        "word": word_str(layout.word),
        "word_bytes": layout.word.width_bytes(),
        "bindings": bindings,
        // Grid mode: `result` key absent (the whole output array is the result).
        // `params` = broadcast args 2..; the gradient has none, proven by the
        // (i32, i32) -> i32 export-signature gate above.
        "params": [],
        "wasm_export": KERNEL_NAME,
        "wasm_bytes": wasm_len,
        "provenance": {
            "source": "Fe compiler (branch mb2)",
            "fe_rev": fe_head_rev(),
            "sonatina_rev": SONATINA_REV,
            "generator": "cargo run -p fe-codegen --example gen_mandelbrot_demo",
            "generated_unix_secs": SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        },
    });

    serde_json::to_string_pretty(&value).expect("layout.json should serialize")
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

fn mode_str(m: LayoutMode) -> &'static str {
    match m {
        LayoutMode::Scalar => "Scalar",
        LayoutMode::Batch => "Batch",
        LayoutMode::Grid => "Grid",
    }
}

fn word_str(w: WordKind) -> &'static str {
    match w {
        WordKind::U32 => "U32",
        WordKind::I64 => "I64",
    }
}

/// Best-effort fe HEAD rev for provenance; "unknown" if git is unavailable.
/// Never load-bearing for correctness.
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
