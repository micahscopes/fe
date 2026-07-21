//! B3 page generator: compile the `poseidon_sigma_u32` keystone through the REAL
//! Fe drivers and emit the static-page inputs under `demos/webgpu-keystone/gen/`.
//!
//! Every artifact on the page is produced here from the compiler, never
//! hand-written:
//!
//! | file            | produced by                                                   |
//! |-----------------|----------------------------------------------------------------|
//! | `kernel.fe`     | verbatim copy of the SSOT fixture (byte-identical to the test) |
//! | `kernel.wgsl`   | the naga-emitted WGSL from the SPIR-V artifact                 |
//! | `layout.json`   | the compiler-stated `SpirvLayout` (binding table + workgroup) |
//! | `kernel.wasm`   | the Fe -> wasm build (`BackendKind::Wasm`)                     |
//! | `reference.json`| the value from EXECUTING `kernel.wasm` under wasmtime here     |
//!
//! HARD-FAIL discipline (never emit a stale or faked artifact):
//!   * the emitted WGSL must be browser-shaped: no 64-bit scalar tokens, naga
//!     `wgsl-in` must reparse it, and it must validate under
//!     `Capabilities::default()` (the browser set, NO SHADER_INT64);
//!   * the word must be `U32` and the workgroup `[1, 1, 1]` (scalar mode: a
//!     single invocation writes the single output slot, no 64-way write race);
//!   * the wasmtime-executed value must equal the pinned `4261282562`.
//! Any deviation panics before a single file is written.
//!
//! Run: `cargo run -p fe-codegen --example gen_webgpu_demo`

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    BackendKind, OptLevel, compile_runtime_package_spirv_with_workgroup, layout_for,
};
use sonatina_codegen::isa::spirv::{Access, LayoutMode, Role, SpirvLayout, WordKind};
use url::Url;

/// The SSOT kernel source: the exact fixture the B2 e2e test `include_str!`s, so
/// the tested source and the shipped source are byte-identical by construction.
const KERNEL_SOURCE: &str = include_str!("../tests/fixtures/spirv/poseidon_sigma_u32.fe");

/// The kernel/export name (the Fe `pub fn`), used for the wasm export lookup and
/// stated in `layout.json` so `wasm-runner.js` calls the right export.
const KERNEL_NAME: &str = "poseidon_sigma_u32";

/// The independently oracle-verified pin (`1 -> 14 -> 210 -> 251 -> 63252 ->
/// 65278 -> 4261282562`). The generator hard-fails if the executed wasm differs.
const PINNED: u32 = 4_261_282_562;

/// The sonatina fork rev the fe workspace is pinned to (`Cargo.toml`), recorded
/// in `layout.json` provenance so the page can show Fe -> these files.
const SONATINA_REV: &str = "76a9a7b92d628333d66ce4569e526e7362152dd8";

/// Scalar-mode kernels dispatch (1,1,1): one invocation writes the single output
/// slot. Shipping the default 64-wide workgroup to Chrome would be a 64-way
/// same-value write race; the browser page must not carry it.
const WORKGROUP: [u32; 3] = [1, 1, 1];

fn main() {
    // gen/ lives at <repo-root>/demos/webgpu-keystone/gen. Resolve it from the
    // crate manifest dir (crates/codegen), robust to the invocation cwd.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("crates/codegen should have a two-level ancestor (the repo root)");
    let demo_dir = repo_root.join("demos/webgpu-keystone");
    let gen_dir = demo_dir.join("gen");
    std::fs::create_dir_all(&gen_dir)
        .unwrap_or_else(|e| panic!("could not create {}: {e}", gen_dir.display()));

    eprintln!("gen_webgpu_demo: compiling `{KERNEL_NAME}` through the real Fe drivers");

    // --- 1. Fe -> SPIR-V (naga) at workgroup (1,1,1). -----------------------
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///gen_poseidon_sigma_u32.fe").expect("gen URL should parse");
    db.workspace()
        .touch(&mut db, url.clone(), Some(KERNEL_SOURCE.to_string()));
    let file = db.workspace().get(&db, &url).expect("gen file should load");
    let top_mod = db.top_mod(file);

    let package = mir::build_wasm_runtime_package(&db, top_mod)
        .expect("keystone should build a wasm runtime package");
    let artifact = compile_runtime_package_spirv_with_workgroup(&db, &package, WORKGROUP)
        .expect("keystone should compile Fe -> naga-validated SPIR-V");

    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("the naga backend should emit WGSL for the u32 kernel")
        .clone();

    // HARD GATE 1: browser-profile WGSL (no 64-bit tokens, wgsl-in reparse,
    // validate with Capabilities::default() - no SHADER_INT64).
    assert_browser_profile_wgsl(&wgsl);

    // HARD GATE 2: word + workgroup + scalar mode as the runner assumes.
    assert_eq!(
        artifact.layout.word,
        WordKind::U32,
        "the u32 kernel must lower to a Uint word (WordKind::U32); off-pin, refusing to emit"
    );
    assert_eq!(
        artifact.layout.workgroup_size, WORKGROUP,
        "layout workgroup must be {WORKGROUP:?}; off-pin, refusing to emit"
    );
    assert_eq!(
        artifact.layout.mode,
        LayoutMode::Scalar,
        "the keystone must be scalar-mode (single output slot); off-pin, refusing to emit"
    );
    assert_eq!(
        artifact
            .layout
            .result
            .expect("scalar keystone must state a single-slot result (Grid mode has none)")
            .width,
        4,
        "the u32 result must read back as 4 bytes; off-pin, refusing to emit"
    );
    eprintln!(
        "  WGSL passed the browser profile: no 64-bit tokens, wgsl-in reparse OK, \
         validated with Capabilities::default() (no SHADER_INT64)"
    );

    // --- 2. Fe -> wasm (the in-browser wasm lane's module). -----------------
    let wasm_bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("keystone should compile Fe -> wasm")
        .into_bytecode()
        .expect("wasm output should be bytecode");

    // --- 3. Execute the wasm under wasmtime for the reference value. --------
    // Compute-don't-bake: the page's reference comes from a REAL execution here,
    // asserted equal to the pin. No hand-typed value reaches the page except the
    // pin itself, which is the anti-fudge cross-check, not the display source.
    let executed = run_wasm_reference(&wasm_bytes);
    if executed != PINNED {
        panic!(
            "HARD FAIL: wasmtime-executed keystone = {executed}, but the pin is {PINNED}. \
             Refusing to emit a stale/faked reference. The kernel or a backend is off-pin."
        );
    }
    eprintln!("  wasmtime-executed reference = {executed} == pin {PINNED}");

    // --- 4. Write gen/ (only after every gate passed). ----------------------
    let layout_json = serialize_layout(&artifact.layout, &wasm_bytes.len());
    let reference_json = serde_json::to_string_pretty(&serde_json::json!({
        "kernel": KERNEL_NAME,
        "value": executed,
        "pinned": PINNED,
        "runtime": "wasmtime (Fe -> wasm), executed at generation time",
    }))
    .expect("reference.json should serialize");

    write_file(&gen_dir.join("kernel.fe"), KERNEL_SOURCE.as_bytes());
    write_file(&gen_dir.join("kernel.wgsl"), wgsl.as_bytes());
    write_file(&gen_dir.join("layout.json"), layout_json.as_bytes());
    write_file(&gen_dir.join("kernel.wasm"), &wasm_bytes);
    write_file(&gen_dir.join("reference.json"), reference_json.as_bytes());

    eprintln!(
        "gen_webgpu_demo: wrote 5 files to {}\n  \
         kernel.fe  kernel.wgsl  layout.json  kernel.wasm  reference.json",
        gen_dir.display()
    );
    eprintln!(
        "  serve: `cd {} && ./serve.sh` then open http://localhost:8787",
        demo_dir.display()
    );
}

/// The browser-profile WGSL gate (static, GPU-free), verbatim in spirit with the
/// B2 e2e test's `assert_browser_profile_wgsl`: (1) no 64-bit scalar token, (2)
/// naga `wgsl-in` round-trips it, (3) it validates under `Capabilities::default()`
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
        "u32 keystone WGSL should use the `u32` scalar; got:\n{wgsl}"
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

/// Execute the keystone wasm under wasmtime and read back the export. Fe `u32`
/// lowers to wasm `i32`, so the export returns `i32`; `as u32` restores the
/// > 2^31 pin exactly.
fn run_wasm_reference(bytes: &[u8]) -> u32 {
    wasmparser::validate(bytes).expect("Fe-emitted wasm should be valid");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime should load the module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("wasmtime should instantiate");
    let f = instance
        .get_typed_func::<(), i32>(&mut store, KERNEL_NAME)
        .unwrap_or_else(|e| panic!("`{KERNEL_NAME}` export should exist: {e}"));
    f.call(&mut store, ())
        .unwrap_or_else(|e| panic!("`{KERNEL_NAME}()` should run: {e}")) as u32
}

/// Serialize the compiler-stated `SpirvLayout` to the `layout.json` the
/// kernel-blind runner consumes. The fork carries no serde (plan ruling 1 item
/// 5); the fe side maps each field here, so the compiler stays the single source
/// of the ABI and nothing downstream re-derives it.
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

    // Scalar keystone: the layout states a single-slot result (Grid mode is the
    // only mode where `result` is None, and this generator asserts Scalar above).
    let result = layout
        .result
        .expect("scalar keystone must state a single-slot result (Grid mode has none)");

    let value = serde_json::json!({
        "kernel": KERNEL_NAME,
        "entry_point": layout.entry_point,
        "mode": mode_str(layout.mode),
        "workgroup_size": layout.workgroup_size,
        "word": word_str(layout.word),
        "word_bytes": layout.word.width_bytes(),
        "bindings": bindings,
        "result": {
            "group": result.group,
            "binding": result.binding,
            "offset": result.offset,
            "width": result.width,
        },
        "wasm_export": KERNEL_NAME,
        "wasm_bytes": wasm_len,
        "provenance": {
            "source": "Fe compiler (branch mb2)",
            "fe_rev": fe_head_rev(),
            "sonatina_rev": SONATINA_REV,
            "generator": "cargo run -p fe-codegen --example gen_webgpu_demo",
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
        LayoutMode::Render => "Render",
    }
}

fn word_str(w: WordKind) -> &'static str {
    match w {
        WordKind::U32 => "U32",
        WordKind::I64 => "I64",
    }
}

/// Best-effort fe HEAD rev for provenance (`git rev-parse HEAD` in the repo);
/// "unknown" if git is unavailable. Never load-bearing for correctness.
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
