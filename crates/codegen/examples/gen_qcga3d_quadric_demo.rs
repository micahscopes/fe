//! Generate the call-free sparse-typed QCGA quadric browser artifacts.

use std::{collections::HashSet, path::PathBuf, process::Command};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    BackendKind, OptLevel, WebBuildOptions, WebBundle, WebCanonicalPolicy,
    compile_runtime_package_spirv_render, layout_for,
};
use sonatina_codegen::isa::spirv::{Access, LayoutMode, Role};
use url::Url;

const SOURCE: &str = include_str!("../tests/fixtures/spirv/qcga3d_rotated_quadric_render.fe");
const EXPORT: &str = "qcga3d_rotated_quadric_render";
const RENDER_LANE: &str = "render";
const VERIFY_LANE: &str = "verify";
const ORACLE_LANE: &str = "oracle";
const ORACLE_PIXEL_LANE: &str = "oracle_pixel";
const WIDTH: u32 = 128;
const HEIGHT: u32 = 128;
const SONATINA_REV: &str = "b2601adc";
const ACTOR_SOURCE: &str = r#"
use core::BrowserBytes

struct RenderRequest {
    generation: u32,
}

struct RenderResponse {
    submitted: bool,
}

struct VerifyRequest {
    generation: u32,
}

struct OracleRequest {
    generation: u32,
}

struct OraclePixelRequest {
    x: i32,
    y: i32,
}

struct OraclePixelResponse {
    rgba: u32,
}

// Nominal host-effect lane. The browser runtime deliberately dispatches this
// schema to its WebGPU actor; this Fe body is never presented as GPU execution.
pub fn render(_request: own RenderRequest) -> RenderResponse {
    RenderResponse { submitted: false }
}

// Nominal host-effect lane for explicit GPU readback. The generated schema is
// authoritative, while the browser runtime supplies the actual WebGPU handler.
pub fn verify(_request: own VerifyRequest) -> BrowserBytes {
    BrowserBytes { ptr: 0, len: 0 }
}

// Nominal host-orchestration lane. Its explicit browser handler assembles an
// owned frame by invoking the genuine Fe/Wasm oracle_pixel lane for every pixel.
pub fn oracle(_request: own OracleRequest) -> BrowserBytes {
    BrowserBytes { ptr: 0, len: 0 }
}

// Genuine Fe/Wasm computation lane. Full-frame orchestration stays in the
// generated host adapter until wasm32 pointer-sized byte storage is available
// without introducing the unsupported u256 scalar envelope.
pub fn oracle_pixel(request: own OraclePixelRequest) -> OraclePixelResponse {
    OraclePixelResponse {
        rgba: qcga3d_rotated_quadric_render(px: request.x, py: request.y),
    }
}
"#;

fn main() {
    let sonatina = std::env::var("SONATINA_DIR").expect("SONATINA_DIR is required");
    let actual_sonatina = git(&sonatina, &["rev-parse", "HEAD"]);
    assert!(
        actual_sonatina.starts_with(SONATINA_REV),
        "expected Sonatina {SONATINA_REV}, found {actual_sonatina}"
    );
    assert!(git(&sonatina, &["status", "--porcelain"]).is_empty());

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest.parent().unwrap().parent().unwrap();
    let out = repo.join("demos/webgpu-qcga3d-quadric/gen");

    let mut raw_db = DriverDataBase::default();
    let raw_url = Url::parse("file:///qcga3d_rotated_quadric_render.fe").unwrap();
    raw_db
        .workspace()
        .touch(&mut raw_db, raw_url.clone(), Some(SOURCE.to_owned()));
    let raw_file = raw_db
        .workspace()
        .get(&raw_db, &raw_url)
        .expect("fixture loads");
    let raw_top = raw_db.top_mod(raw_file);
    let package = mir::build_wasm_runtime_package_for_entry(&raw_db, raw_top, EXPORT)
        .expect("runtime package");
    assert!(
        package
            .functions(&raw_db)
            .iter()
            .all(|f| f.linkage(&raw_db) != mir::RuntimeLinkage::External)
    );
    let artifact = compile_runtime_package_spirv_render(&raw_db, &package)
        .expect("QCGA Render SPIR-V/WGSL compilation");
    let wgsl = artifact.wgsl.as_deref().expect("WGSL side artifact");
    assert_browser_wgsl(wgsl);
    assert_eq!(artifact.layout.mode, LayoutMode::Render);
    assert_eq!(artifact.layout.builtin_inputs.len(), 2);

    let wasm = BackendKind::Wasm
        .create()
        .compile(
            &raw_db,
            raw_top,
            layout_for(BackendKind::Wasm),
            OptLevel::O0,
        )
        .expect("QCGA Wasm compilation")
        .into_bytecode()
        .expect("Wasm bytecode");
    wasmparser::validate(&wasm).expect("valid Wasm");
    assert_zero_imports(&wasm);
    let frame = run_frame(&wasm);
    let hash = fnv1a32(&frame);
    let distinct = frame.iter().copied().collect::<HashSet<_>>().len();
    assert!(distinct > 8, "QCGA reference must not be a flat frame");

    let combined_source = format!("{SOURCE}\n{ACTOR_SOURCE}");
    let mut canonical_db = DriverDataBase::default();
    let canonical_url = Url::parse("file:///qcga3d_actor.fe").unwrap();
    canonical_db.workspace().touch(
        &mut canonical_db,
        canonical_url.clone(),
        Some(combined_source.clone()),
    );
    let canonical_file = canonical_db
        .workspace()
        .get(&canonical_db, &canonical_url)
        .expect("canonical QCGA source loads");
    let canonical_top = canonical_db.top_mod(canonical_file);
    let diagnostics = canonical_db
        .run_on_top_mod(canonical_top)
        .format_diags(&canonical_db);
    assert!(
        diagnostics.is_empty(),
        "QCGA canonical actor source has diagnostics:\n{diagnostics}"
    );
    let canonical_bundle = WebBundle::compile(
        &canonical_db,
        canonical_top,
        WebBuildOptions::render(EXPORT, Some("qcga3d_actor.fe".to_owned()))
            .with_canonical_entries([RENDER_LANE, VERIFY_LANE, ORACLE_LANE, ORACLE_PIXEL_LANE])
            .with_canonical_policy(WebCanonicalPolicy::Required),
    )
    .expect("QCGA canonical multi-lane WebBundle");
    assert_eq!(canonical_bundle.wgsl, wgsl);
    let actor_interface_js = canonical_bundle
        .interface_js
        .as_ref()
        .expect("required QCGA canonical interface.js");
    let actor_interface_d_ts = canonical_bundle
        .interface_d_ts
        .as_ref()
        .expect("required QCGA canonical interface.d.ts");

    let bindings = artifact.layout.bindings.iter().map(|b| serde_json::json!({
        "group": b.group, "binding": b.binding, "name": b.name,
        "access": match b.access { Access::Read => "Read", Access::ReadWrite => "ReadWrite" },
        "role": match b.role { Role::Input => "Input", Role::Output => "Output" },
        "span": b.span, "stride": b.stride,
    })).collect::<Vec<_>>();
    let fe_rev = git(repo.to_str().unwrap(), &["rev-parse", "HEAD"]);
    let provenance = serde_json::json!({
        "fixture": "crates/codegen/tests/fixtures/spirv/qcga3d_rotated_quadric_render.fe",
        "fe_rev": fe_rev,
        "sonatina_rev": actual_sonatina,
        "generator": "gen_qcga3d_quadric_demo",
        "source_fnv1a32": fnv1a32_bytes(SOURCE.as_bytes()),
        "actor_source_fnv1a32": fnv1a32_bytes(ACTOR_SOURCE.as_bytes()),
    });
    let layout = serde_json::to_string_pretty(&serde_json::json!({
        "kernel": EXPORT, "mode": "Render", "entry_point": artifact.layout.entry_point,
        "vertex_entry": artifact.layout.vertex_entry,
        "fragment_entry": artifact.layout.fragment_entry,
        "color_target_format": artifact.layout.color_target_format,
        "width": WIDTH, "height": HEIGHT, "bindings": bindings,
        "builtin_inputs": artifact.layout.builtin_inputs.len(),
        "frag_wasm_export": EXPORT,
        "actor_wasm": "actor-canonical.wasm",
        "actor_interface": "actor-interface.js",
        "actor_lanes": [RENDER_LANE, VERIFY_LANE, ORACLE_LANE, ORACLE_PIXEL_LANE],
        "provenance": provenance,
    }))
    .unwrap();
    let reference = serde_json::to_string_pretty(&serde_json::json!({
        "fragment": EXPORT, "width": WIDTH, "height": HEIGHT,
        "fnv1a32": hash, "distinct_colors": distinct,
        "runtime": "wasmtime executing the zero-import Fe-compiled Wasm over all 16384 pixels",
        "provenance": provenance,
    }))
    .unwrap();

    std::fs::create_dir_all(&out).unwrap();
    write(&out.join("kernel.fe"), SOURCE.as_bytes());
    write(&out.join("frag.wgsl"), wgsl.as_bytes());
    write(&out.join("frag.wasm"), &wasm);
    write(&out.join("actor-canonical.wasm"), &canonical_bundle.wasm);
    write(
        &out.join("actor-interface.js"),
        actor_interface_js.as_bytes(),
    );
    write(
        &out.join("actor-interface.d.ts"),
        actor_interface_d_ts.as_bytes(),
    );
    write(
        &out.join("actor-manifest.json"),
        &canonical_bundle
            .manifest_json()
            .expect("QCGA canonical manifest serializes"),
    );
    write(&out.join("actor-source.fe"), combined_source.as_bytes());
    write(&out.join("layout.json"), layout.as_bytes());
    write(&out.join("reference.json"), reference.as_bytes());
    eprintln!(
        "QCGA bundle: FNV-1a-32={hash} (0x{hash:08x}), colors={distinct}, \
         canonical lanes={RENDER_LANE},{VERIFY_LANE},{ORACLE_LANE},{ORACLE_PIXEL_LANE}"
    );
}

fn run_frame(bytes: &[u8]) -> Vec<u32> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let render = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, EXPORT)
        .unwrap();
    let mut frame = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT as i32 {
        for x in 0..WIDTH as i32 {
            frame.push(render.call(&mut store, (x, y)).unwrap() as u32);
        }
    }
    frame
}

fn assert_zero_imports(bytes: &[u8]) {
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::ImportSection(imports) = payload.unwrap() {
            assert_eq!(imports.count(), 0, "browser Wasm must be zero-import");
        }
    }
}

fn assert_browser_wgsl(wgsl: &str) {
    assert!(!wgsl.contains("i64") && !wgsl.contains("u64"));
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL reparses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("browser-profile WGSL validates");
}

fn fnv1a32(frame: &[u32]) -> u32 {
    frame
        .iter()
        .flat_map(|v| v.to_le_bytes())
        .fold(0x811c9dc5, |h, b| (h ^ b as u32).wrapping_mul(0x01000193))
}

fn fnv1a32_bytes(bytes: &[u8]) -> u32 {
    bytes
        .iter()
        .fold(0x811c9dc5, |h, b| (h ^ *b as u32).wrapping_mul(0x01000193))
}

fn git(path: &str, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write(path: &std::path::Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
