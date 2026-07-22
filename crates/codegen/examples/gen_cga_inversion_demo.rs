//! Package the typed recursive runtime-center cyclide Render fixture for a browser page.
//!
//! Every compiler, ABI, wasm, and full-frame oracle gate runs before `gen/` is
//! created or any artifact is written.

use std::{
    collections::HashSet,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, compile_runtime_package_spirv_render, layout_for};
use sonatina_codegen::isa::spirv::{
    Access, LayoutMode, Role, SpirvBuiltinSource, SpirvScalarKind, WordKind,
};
use url::Url;

const SOURCE: &str =
    include_str!("../tests/fixtures/spirv/cga_inversion_cyclide_recursive_support.fe");
const FRAG_NAME: &str = "cga_inversion_cyclide_recursive_support";
const WIDTH: u32 = 128;
const HEIGHT: u32 = 128;
const CAM_X: f32 = 0.0;
const CAM_Y: f32 = 0.0;
const ZOOM: f32 = 0.0125;
const INV_CX: f32 = 0.5;
const INV_CY: f32 = 0.0;

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo_root = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root");
    let gen_dir = repo_root.join("demos/webgpu-cga-inversion/gen");

    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///gen_cga_inversion_cyclide_recursive_support.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(SOURCE.to_string()));
    let file = db
        .workspace()
        .get(&db, &url)
        .expect("D1 source should load");
    let package = mir::build_wasm_runtime_package(&db, db.top_mod(file))
        .expect("D1 should build a wasm runtime package");
    assert!(
        package
            .functions(&db)
            .iter()
            .all(|f| f.linkage(&db) != mir::RuntimeLinkage::External),
        "D1 f32 helpers must lower intrinsically, never remain imports"
    );
    let artifact = compile_runtime_package_spirv_render(&db, &package)
        .expect("D1 should compile to Render SPIR-V/WGSL");
    let wgsl = artifact
        .wgsl
        .as_ref()
        .expect("Render compilation must emit WGSL");
    assert_browser_profile_wgsl(wgsl);
    for token in ["@vertex", "@fragment", "unpack4x8unorm", "loop", "sqrt("] {
        assert!(wgsl.contains(token), "D1 WGSL lacks `{token}`");
    }

    let layout = &artifact.layout;
    assert_eq!(layout.mode, LayoutMode::Render);
    assert_eq!(layout.entry_point, "fs_main");
    assert_eq!(layout.word, WordKind::U32);
    assert!(layout.result.is_none());
    assert_eq!(layout.vertex_entry.as_deref(), Some("vs_fullscreen"));
    assert_eq!(layout.fragment_entry.as_deref(), Some("fs_main"));
    assert_eq!(layout.color_target_format.as_deref(), Some("rgba8unorm"));
    let input = layout
        .bindings
        .iter()
        .find(|b| b.role == Role::Input)
        .expect("D1 must have a broadcast Input binding");
    assert_eq!((input.group, input.binding), (0, 1));
    assert_eq!(input.access, Access::Read);
    assert_eq!(
        input.span, 20,
        "member span and allocation stride are separate ABI facts"
    );
    assert_eq!(
        input.stride, 20,
        "D1 tightly packs five f32 broadcast values"
    );
    assert_eq!(input.members.len(), 5);
    assert_eq!(layout.builtin_inputs.len(), 2);
    assert_eq!(layout.builtin_inputs[0].arg_index, 0);
    assert_eq!(layout.builtin_inputs[0].scalar, SpirvScalarKind::I32);
    assert_eq!(
        layout.builtin_inputs[0].source,
        SpirvBuiltinSource::FragmentPositionX
    );
    assert_eq!(layout.builtin_inputs[1].arg_index, 1);
    assert_eq!(layout.builtin_inputs[1].scalar, SpirvScalarKind::I32);
    assert_eq!(
        layout.builtin_inputs[1].source,
        SpirvBuiltinSource::FragmentPositionY
    );
    let param_names = ["cam_x", "cam_y", "zoom", "inv_cx", "inv_cy"];
    let mut params = Vec::new();
    for (index, (member, name)) in input.members.iter().zip(param_names).enumerate() {
        assert_eq!(member.arg_index, index as u32 + 2);
        assert_eq!(member.offset, index as u32 * 4);
        assert_eq!(member.width, 4);
        assert_eq!(member.scalar, SpirvScalarKind::F32);
        params.push(serde_json::json!({
            "name": name,
            "arg_index": member.arg_index,
            "offset": member.offset,
            "width": member.width,
            "scalar": "F32",
        }));
    }

    let wasm = compile_to_wasm(SOURCE);
    assert_zero_imports(&wasm);
    assert_eq!(
        export_signature(&wasm, FRAG_NAME),
        (
            vec![
                WasmTy::I32,
                WasmTy::I32,
                WasmTy::F32,
                WasmTy::F32,
                WasmTy::F32,
                WasmTy::F32,
                WasmTy::F32
            ],
            vec![WasmTy::I32]
        ),
        "D1 wasm export must be exactly (i32,i32,f32,f32,f32,f32,f32)->i32"
    );

    let frame = run_wasm_frame(&wasm);
    let mut sky_count = 0usize;
    let mut upper_count = 0usize;
    let mut lower_count = 0usize;
    let mut distinct = HashSet::new();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let got = frame[(y * WIDTH + x) as usize];
            let (expected, material) =
                oracle(x as i32, y as i32, CAM_X, CAM_Y, ZOOM, INV_CX, INV_CY);
            assert_eq!(got, expected, "wasmtime/oracle mismatch at ({x},{y})");
            distinct.insert(got);
            match material {
                0 => sky_count += 1,
                1 => upper_count += 1,
                2 => lower_count += 1,
                other => panic!("unexpected oracle material {other}"),
            }
        }
    }
    assert!(
        sky_count > 0 && upper_count > 0 && lower_count > 0,
        "D1 cyclide frame must contain sky and both palette halves"
    );
    assert!(
        distinct.len() >= 8,
        "D1 frame must expose at least eight colors"
    );
    let hit_count = upper_count + lower_count;
    assert_eq!(
        sky_count + hit_count,
        (WIDTH * HEIGHT) as usize,
        "D1 classification must cover the complete 128x128 frame"
    );
    let hash = fnv1a32(&frame);

    let provenance = provenance();
    let bindings: Vec<_> = layout
        .bindings
        .iter()
        .map(|b| {
            serde_json::json!({
                "group": b.group, "binding": b.binding, "name": b.name,
                "access": match b.access { Access::Read => "Read", Access::ReadWrite => "ReadWrite" },
                "role": match b.role { Role::Input => "Input", Role::Output => "Output" },
                "span": b.span, "stride": b.stride,
            })
        })
        .collect();
    let builtin_inputs: Vec<_> = layout
        .builtin_inputs
        .iter()
        .map(|builtin| {
            serde_json::json!({
                "arg_index": builtin.arg_index,
                "scalar": scalar_str(builtin.scalar),
                "source": builtin_source_str(builtin.source),
            })
        })
        .collect();
    let layout_json = serde_json::to_string_pretty(&serde_json::json!({
        "kernel": FRAG_NAME, "entry_point": layout.entry_point,
        "mode": "Render", "word": "U32", "word_bytes": 4,
        "vertex_entry": layout.vertex_entry, "fragment_entry": layout.fragment_entry,
        "color_target_format": layout.color_target_format, "bindings": bindings,
        "builtin_inputs": builtin_inputs,
        "params": params, "frag_wasm_export": FRAG_NAME, "frag_wasm_bytes": wasm.len(),
        "width": WIDTH, "height": HEIGHT, "provenance": provenance.clone(),
    }))
    .unwrap();
    let reference_json = serde_json::to_string_pretty(&serde_json::json!({
        "fragment": FRAG_NAME, "width": WIDTH, "height": HEIGHT,
        "view": [CAM_X, CAM_Y, ZOOM], "inversion_center": [INV_CX, INV_CY],
        "parameter_types": ["F32", "F32", "F32", "F32", "F32"],
        "shape": "inverted_offset_torus_cyclide",
        "algebra": "typed support-specialized recursive Cl(4,1) S*P*S",
        "inversion_center_runtime": true,
        "fnv1a32": hash, "sky_pixels": sky_count, "hit_pixels": hit_count,
        "upper_pixels": upper_count, "lower_pixels": lower_count,
        "distinct_colors": distinct.len(),
        "runtime": "wasmtime executing Fe-compiled wasm; every pixel checked against independent Rust f32 oracle",
        "provenance": provenance,
    })).unwrap();
    validate_serialized_schema(&layout_json, &reference_json);

    // No filesystem output before every assertion and the full-frame oracle pass.
    std::fs::create_dir_all(&gen_dir).unwrap_or_else(|e| panic!("{}: {e}", gen_dir.display()));
    write(&gen_dir.join("kernel.fe"), SOURCE.as_bytes());
    write(&gen_dir.join("frag.wgsl"), wgsl.as_bytes());
    write(&gen_dir.join("frag.wasm"), &wasm);
    write(&gen_dir.join("layout.json"), layout_json.as_bytes());
    write(&gen_dir.join("reference.json"), reference_json.as_bytes());
    eprintln!(
        "gen_cga_inversion_demo: wrote 5 gated artifacts to {}; frame FNV-1a-32={hash} (0x{hash:08x}), sky={sky_count}, upper={upper_count}, lower={lower_count}, colors={}",
        gen_dir.display(),
        distinct.len()
    );
}

fn compile_to_wasm(source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///gen_cga_inversion_wasm.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db
        .workspace()
        .get(&db, &url)
        .expect("wasm source should load");
    let bytes = BackendKind::Wasm
        .create()
        .compile(
            &db,
            db.top_mod(file),
            layout_for(BackendKind::Wasm),
            OptLevel::O0,
        )
        .expect("D1 Fe -> wasm compile")
        .into_bytecode()
        .expect("wasm bytecode");
    wasmparser::validate(&bytes).expect("valid wasm");
    bytes
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WasmTy {
    I32,
    F32,
    Other,
}

fn export_signature(bytes: &[u8], export_name: &str) -> (Vec<WasmTy>, Vec<WasmTy>) {
    use wasmparser::{ExternalKind, Payload, TypeRef, ValType};
    let map = |v: &ValType| match v {
        ValType::I32 => WasmTy::I32,
        ValType::F32 => WasmTy::F32,
        _ => WasmTy::Other,
    };
    let mut sigs = Vec::new();
    let mut defined_types = Vec::new();
    let mut imported = 0u32;
    let mut exported = None;
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        match payload.expect("valid wasm payload") {
            Payload::TypeSection(reader) => {
                for group in reader {
                    for subtype in group.expect("valid type group").into_types() {
                        let f = subtype.unwrap_func();
                        sigs.push((
                            f.params().iter().map(map).collect(),
                            f.results().iter().map(map).collect(),
                        ));
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    if matches!(import.expect("valid import").ty, TypeRef::Func(_)) {
                        imported += 1;
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for ty in reader {
                    defined_types.push(ty.expect("valid function type"));
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.expect("valid export");
                    if export.name == export_name {
                        assert_eq!(export.kind, ExternalKind::Func);
                        exported = Some(export.index);
                    }
                }
            }
            _ => {}
        }
    }
    let index = exported.expect("D1 wasm export missing");
    assert!(index >= imported);
    let ty = defined_types[(index - imported) as usize] as usize;
    sigs[ty].clone()
}

fn assert_zero_imports(bytes: &[u8]) {
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::ImportSection(reader) = payload.expect("valid wasm payload") {
            assert_eq!(reader.count(), 0, "browser D1 wasm must have zero imports");
        }
    }
}

fn run_wasm_frame(bytes: &[u8]) -> Vec<u32> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("wasmtime module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("zero-import instance");
    let f = instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32, f32), i32>(&mut store, FRAG_NAME)
        .expect("exact typed D1 export");
    let mut frame = Vec::with_capacity((WIDTH * HEIGHT) as usize);
    for y in 0..HEIGHT as i32 {
        for x in 0..WIDTH as i32 {
            frame.push(
                f.call(&mut store, (x, y, CAM_X, CAM_Y, ZOOM, INV_CX, INV_CY))
                    .expect("D1 wasm pixel") as u32,
            );
        }
    }
    frame
}

fn oracle(
    px: i32,
    py: i32,
    cam_x: f32,
    cam_y: f32,
    zoom: f32,
    inv_cx: f32,
    inv_cy: f32,
) -> (u32, u8) {
    let sx = (px as f32 - 64.0) * zoom;
    let sy = (py as f32 - 64.0) * zoom;
    let rz = 1.8_f32;
    let inv_len = 1.0 / (sx * sx + sy * sy + rz * rz).sqrt();
    let (rdx, rdy, rdz) = (sx * inv_len, sy * inv_len, rz * inv_len);
    let mut t = 0.0_f32;
    let mut i = 0_i32;
    while i < 72 {
        let (x, y, z) = (cam_x + rdx * t, cam_y + rdy * t, -4.0 + rdz * t);
        let vx = x - inv_cx;
        let vy = y - inv_cy;
        let rho2 = vx * vx + vy * vy + z * z;
        let safe_rho2 = if rho2 < 0.0004 { 0.0004 } else { rho2 };
        let (qx, qy, qz) = (
            inv_cx + vx / safe_rho2,
            inv_cy + vy / safe_rho2,
            z / safe_rho2,
        );
        let tx = qx + 0.62;
        let ty = qy - 0.08;
        let ring_radius = (tx * tx + ty * ty).sqrt() - 0.58;
        let base = (ring_radius * ring_radius + qz * qz).sqrt() - 0.17;
        let distance = base * safe_rho2;
        t = t + distance * 0.18;
        if distance < 0.0022 {
            let shade = 38 + 24 * (i >> 3);
            if qy > 0.0 {
                return (
                    (shade + 88 * 256 + (255 - shade) * 65_536 - 16_777_216_i32) as u32,
                    1,
                );
            }
            return ((56 + shade * 256 + 224 * 65_536 - 16_777_216_i32) as u32, 2);
        }
        i += 1;
    }
    ((7 + 11 * 256 + 25 * 65_536 - 16_777_216_i32) as u32, 0)
}

fn fnv1a32(frame: &[u32]) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    for value in frame {
        for byte in value.to_le_bytes() {
            hash ^= byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    hash
}

fn assert_browser_profile_wgsl(wgsl: &str) {
    for token in ["i64", "u64"] {
        assert!(!wgsl.contains(token), "browser WGSL contains {token}");
    }
    let module = naga::front::wgsl::parse_str(wgsl).expect("naga must reparse emitted WGSL");
    let caps = naga::valid::Capabilities::default();
    assert!(!caps.contains(naga::valid::Capabilities::SHADER_INT64));
    naga::valid::Validator::new(naga::valid::ValidationFlags::all(), caps)
        .validate(&module)
        .expect("browser-profile WGSL validation");
}

fn scalar_str(scalar: SpirvScalarKind) -> &'static str {
    match scalar {
        SpirvScalarKind::I32 => "I32",
        SpirvScalarKind::U32 => "U32",
        SpirvScalarKind::F32 => "F32",
        SpirvScalarKind::I64 => "I64",
        SpirvScalarKind::I1 => "I1",
    }
}

fn builtin_source_str(source: SpirvBuiltinSource) -> &'static str {
    match source {
        SpirvBuiltinSource::GlobalInvocationIdX => "GlobalInvocationIdX",
        SpirvBuiltinSource::GlobalInvocationIdY => "GlobalInvocationIdY",
        SpirvBuiltinSource::FragmentPositionX => "FragmentPositionX",
        SpirvBuiltinSource::FragmentPositionY => "FragmentPositionY",
    }
}

/// Reparse the exact browser artifacts and assert the canonical JSON contract.
/// This catches drift in the hand-assembled serialization before `gen/` exists.
fn validate_serialized_schema(layout_json: &str, reference_json: &str) {
    let layout: serde_json::Value = serde_json::from_str(layout_json).expect("layout JSON");
    assert_eq!(layout["mode"], "Render");
    assert_eq!(layout["entry_point"], "fs_main");
    assert_eq!(layout["word"], "U32");
    assert_eq!(layout["vertex_entry"], "vs_fullscreen");
    assert_eq!(layout["fragment_entry"], "fs_main");
    assert_eq!(layout["color_target_format"], "rgba8unorm");
    assert_eq!(layout["width"], WIDTH);
    assert_eq!(layout["height"], HEIGHT);

    let input = layout["bindings"]
        .as_array()
        .expect("bindings array")
        .iter()
        .find(|binding| binding["role"] == "Input")
        .expect("serialized Input binding");
    assert_eq!(input["group"], 0);
    assert_eq!(input["binding"], 1);
    assert_eq!(input["access"], "Read");
    assert_eq!(input["span"], 20);
    assert_eq!(input["stride"], 20);

    let params = layout["params"].as_array().expect("typed params array");
    assert_eq!(params.len(), 5);
    for (param, (name, arg_index, offset)) in params.iter().zip([
        ("cam_x", 2, 0),
        ("cam_y", 3, 4),
        ("zoom", 4, 8),
        ("inv_cx", 5, 12),
        ("inv_cy", 6, 16),
    ]) {
        assert_eq!(param["name"], name);
        assert_eq!(param["arg_index"], arg_index);
        assert_eq!(param["offset"], offset);
        assert_eq!(param["width"], 4);
        assert_eq!(param["scalar"], "F32");
    }

    let builtins = layout["builtin_inputs"]
        .as_array()
        .expect("builtin_inputs array");
    assert_eq!(builtins.len(), 2);
    for (builtin, (arg_index, source)) in builtins
        .iter()
        .zip([(0, "FragmentPositionX"), (1, "FragmentPositionY")])
    {
        assert_eq!(builtin["arg_index"], arg_index);
        assert_eq!(builtin["scalar"], "I32");
        assert_eq!(builtin["source"], source);
    }

    let reference: serde_json::Value =
        serde_json::from_str(reference_json).expect("reference JSON");
    assert_eq!(reference["width"], WIDTH);
    assert_eq!(reference["height"], HEIGHT);
    assert_eq!(
        reference["parameter_types"],
        serde_json::json!(["F32", "F32", "F32", "F32", "F32"])
    );
    let view = reference["view"].as_array().expect("reference view array");
    assert_eq!(view.len(), 3);
    for (actual, expected) in view.iter().zip([CAM_X, CAM_Y, ZOOM]) {
        let actual = actual.as_f64().expect("reference view scalar");
        assert!(
            (actual - f64::from(expected)).abs() <= f64::EPSILON,
            "serialized view scalar {actual} differs from f32 source {expected}"
        );
    }
    assert_eq!(
        reference["inversion_center"],
        serde_json::json!([INV_CX, INV_CY])
    );
    assert_eq!(reference["shape"], "inverted_offset_torus_cyclide");
    assert_eq!(
        reference["algebra"],
        "typed support-specialized recursive Cl(4,1) S*P*S"
    );
    assert_eq!(reference["inversion_center_runtime"], true);
    assert!(
        reference["sky_pixels"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    assert!(
        reference["hit_pixels"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
    let upper = reference["upper_pixels"]
        .as_u64()
        .expect("upper palette pixel count");
    let lower = reference["lower_pixels"]
        .as_u64()
        .expect("lower palette pixel count");
    assert!(upper > 0 && lower > 0);
    assert_eq!(reference["hit_pixels"].as_u64(), Some(upper + lower));
    assert_eq!(
        reference["sky_pixels"].as_u64(),
        Some((WIDTH * HEIGHT) as u64 - upper - lower)
    );
    assert!(
        reference["distinct_colors"]
            .as_u64()
            .is_some_and(|count| count >= 8)
    );
}

fn provenance() -> serde_json::Value {
    const EXPECTED_SONATINA: &str = "ed43625bb5680aeab993371e28a8c8e5c7c16f96";
    let sonatina_path = std::env::var("SONATINA_DIR")
        .expect("SONATINA_DIR must identify the reviewed local Sonatina checkout");
    let fe_rev = std::env::var("FE_CGA_SOURCE_REV")
        .expect("use demos/webgpu-cga-inversion/generate.sh to capture clean Fe provenance");
    let current_fe_rev = git_rev(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(
        fe_rev, current_fe_rev,
        "Fe HEAD changed after generator preflight"
    );
    assert_only_cargo_lock_changed(env!("CARGO_MANIFEST_DIR"));
    let fe_untracked_present = match std::env::var("FE_CGA_SOURCE_UNTRACKED_PRESENT").as_deref() {
        Ok("0") => false,
        Ok("1") => true,
        _ => panic!("invalid FE_CGA_SOURCE_UNTRACKED_PRESENT generator preflight value"),
    };
    let sonatina_rev = git_rev(&sonatina_path);
    assert_eq!(
        sonatina_rev, EXPECTED_SONATINA,
        "Sonatina HEAD changed or is not the reviewed D1 revision"
    );
    assert!(
        git_status(&sonatina_path, "--untracked-files=normal").is_empty(),
        "Sonatina checkout changed after generator preflight"
    );
    serde_json::json!({
        "source": "Fe compiler branch mb2",
        "fe_rev": fe_rev,
        "fe_tracked_dirty": false,
        "fe_untracked_present": fe_untracked_present,
        "sonatina_source": "local-path unpublished checkout",
        "sonatina_path": sonatina_path,
        "sonatina_rev": sonatina_rev,
        "sonatina_tracked_dirty": git_tracked_dirty(&sonatina_path),
        "sonatina_untracked_present": git_untracked_present(&sonatina_path),
        "generator": "cargo with four local-path Sonatina patches run -p fe-codegen --example gen_cga_inversion_demo",
        "generated_unix_secs": SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
    })
}

fn git_rev(path: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["-C", path, "rev-parse", "HEAD"])
        .output()
        .unwrap_or_else(|error| panic!("failed to run git for provenance at {path}: {error}"));
    assert!(
        output.status.success(),
        "git rev-parse failed for provenance at {path}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    let revision = String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("non-UTF-8 git revision at {path}: {error}"));
    let revision = revision.trim().to_string();
    assert!(!revision.is_empty(), "empty git revision at {path}");
    revision
}

fn git_status(path: &str, untracked: &str) -> String {
    let output = std::process::Command::new("git")
        .args(["-C", path, "status", "--porcelain", untracked])
        .output()
        .unwrap_or_else(|error| panic!("failed to run git status at {path}: {error}"));
    assert!(
        output.status.success(),
        "git status failed for provenance at {path}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("non-UTF-8 git status at {path}: {error}"))
}

fn git_tracked_dirty(path: &str) -> bool {
    !git_status(path, "--untracked-files=no").is_empty()
}

fn git_untracked_present(path: &str) -> bool {
    git_status(path, "--untracked-files=normal")
        .lines()
        .any(|line| line.starts_with("?? "))
}

fn assert_only_cargo_lock_changed(path: &str) {
    for line in git_status(path, "--untracked-files=no").lines() {
        let changed_path = line.get(3..).unwrap_or("");
        assert_eq!(
            changed_path, "Cargo.lock",
            "tracked Fe source changed after generator preflight: {line}"
        );
    }
}

fn write(path: &std::path::Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
