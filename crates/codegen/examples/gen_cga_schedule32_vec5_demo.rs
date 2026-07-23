//! Stage the canonical CTFE Schedule<32> + provider-emitted five-lane render.
//!
//! This intentionally writes to `gen-schedule32`; promotion to the live demo is
//! a separate reviewed operation.

use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    CanonicalCapability, CanonicalExecution, CanonicalInterfaceManifest, CanonicalPlacement,
    WasmCompileOptions, WebBuildOptions, WebBundle, WebCanonicalPolicy,
    canonical_lane_decl_from_entry, compile_runtime_package_spirv_render,
    compile_runtime_package_wasm_with_options,
};
use sonatina_codegen::isa::spirv::{
    Access, LayoutMode, Role, SpirvBuiltinSource, SpirvScalarKind, WordKind,
};
use hir::hir_def::HirIngot;
use url::Url;

const CANONICAL: &str = include_str!("../tests/fixtures/fco_cga80_direct_lanes.fe");
const SPARSE_CLIFFORD_API: &str = include_str!("../../../ingots/sparse_clifford/src/lib.fe");
const BODY: &str = include_str!("../tests/fixtures/spirv/fco_cga80_direct_de_body.fe");
const NAME: &str = "cga_schedule32_vec5_de_render";
const WIDTH: u32 = 128;
const HEIGHT: u32 = 128;
const CAM_X: f32 = 0.0;
const CAM_Y: f32 = 0.0;
const ZOOM: f32 = 0.0125;
const INV_CX: f32 = 0.5;
const INV_CY: f32 = 0.0;
const RENDER_LANE: &str = "render";
const VERIFY_LANE: &str = "verify";
const ORACLE_LANE: &str = "oracle";
const APP_IMPORTS: &str = r#"use sparse_clifford::{
    BladeSet, Nat, PlanLength, SparsePlan, blade_set, plan_mask_candidate,
    plan_mask_cardinality, support_gp, support_grade,
}
"#;
const ACTOR_SOURCE: &str = r#"
use core::{AllocatedBrowserBytes, BrowserBytes, HostEffect, MainThread}
use core::effect_ref::alloc_bytes
use core::num::IntDowncast
use std::webgpu::{Dispatch, WebGpuBackend}

struct FrameRequest {
    generation: u32,
    cam_x: f32,
    cam_y: f32,
    zoom: f32,
    inv_cx: f32,
    inv_cy: f32,
}

struct Submitted {
    submitted: bool,
}

pub fn render(_request: own FrameRequest) -> Submitted
    uses (HostEffect, MainThread, mut Dispatch<WebGpuBackend>)
{
    Submitted { submitted: false }
}

pub fn verify(_request: own FrameRequest) -> BrowserBytes
    uses (HostEffect, MainThread, mut Dispatch<WebGpuBackend>)
{
    BrowserBytes { ptr: 0, len: 0 }
}

fn append_byte(value: u32) {
    let byte = alloc_bytes(1)
    byte.write(value.downcast_truncate())
}

fn append_frame_tail(
    first_rgba: u32,
    cam_x: f32,
    cam_y: f32,
    zoom: f32,
    inv_cx: f32,
    inv_cy: f32,
) {
    append_byte(value: first_rgba >> 8)
    append_byte(value: first_rgba >> 16)
    append_byte(value: first_rgba >> 24)

    let mut y: i32 = 0
    let mut x: i32 = 1
    while y < 128 {
        while x < 128 {
            let rgba = cga_schedule32_vec5_de_render(
                px: x,
                py: y,
                cam_x: cam_x,
                cam_y: cam_y,
                zoom: zoom,
                inv_cx: inv_cx,
                inv_cy: inv_cy,
            )
            append_byte(value: rgba)
            append_byte(value: rgba >> 8)
            append_byte(value: rgba >> 16)
            append_byte(value: rgba >> 24)
            x = x + 1
        }
        y = y + 1
        x = 0
    }
}

pub fn oracle(request: own FrameRequest) -> AllocatedBrowserBytes {
    let first_rgba = cga_schedule32_vec5_de_render(
        px: 0,
        py: 0,
        cam_x: request.cam_x,
        cam_y: request.cam_y,
        zoom: request.zoom,
        inv_cx: request.inv_cx,
        inv_cy: request.inv_cy,
    )
    let first = alloc_bytes(1)
    first.write(first_rgba.downcast_truncate())
    append_frame_tail(
        first_rgba: first_rgba,
        cam_x: request.cam_x,
        cam_y: request.cam_y,
        zoom: request.zoom,
        inv_cx: request.inv_cx,
        inv_cy: request.inv_cy,
    )
    AllocatedBrowserBytes { ptr: first, len: 65536 }
}
"#;

fn app_source() -> String {
    let (prefix, rest) = CANONICAL
        .split_once("// BEGIN_PUBLIC_ORACLES")
        .expect("canonical public-oracle begin marker");
    let (_, suffix) = rest
        .split_once("// END_PUBLIC_ORACLES")
        .expect("canonical public-oracle end marker");
    let source = format!("{APP_IMPORTS}\n{prefix}{suffix}\n{BODY}");
    assert!(source.contains(
        "SparsePlan<2707775, 4498990, 8948932, 136, 0, 0, 0, 0, 80, 32>",
    ));
    assert!(source.contains("const fn survivor_triple"));
    assert!(source.contains("struct CanonicalCgaProvider"));
    assert!(
        CANONICAL.contains("for triple in 0..80"),
        "the provider must scan the complete semantic candidate universe",
    );
    assert!(
        CANONICAL.matches("builder.emit_method(").count() == 1
            && CANONICAL.contains("builder.emit_method(\"sandwich\", image)"),
        "the provider must emit exactly one aggregate sandwich method",
    );
    assert!(
        ["e1", "e2", "e4", "e8", "e16"]
            .into_iter()
            .all(|lane| !CANONICAL.contains(&format!("builder.emit_method(\"{lane}\""))),
        "the provider must not retain the legacy five scalar methods",
    );
    assert!(!source.contains("trait Eval5"));
    assert!(!source.contains("ScheduleChunk"));
    source
}

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root");
    let out = repo.join("demos/webgpu-cga-inversion/gen-schedule32");
    let source = app_source();

    // Analyze one module containing both the canonical kernel and actor lanes.
    // The kernel package remains rooted at `NAME`, so actor-only functions are
    // unreachable from its Wasm/SPIR-V artifacts.
    let actor_source = format!("{source}\n{ACTOR_SOURCE}");
    let temp_root = repo.join("output/demo-tmp");
    std::fs::create_dir_all(&temp_root).expect("Schedule32 temporary ingot root");
    let app_dir = tempfile::Builder::new()
        .prefix("fe-schedule32-app-")
        .tempdir_in(&temp_root)
        .expect("temporary Schedule32 application ingot");
    let app_src = app_dir.path().join("src");
    std::fs::create_dir_all(&app_src).expect("temporary Schedule32 src directory");
    std::fs::write(
        app_dir.path().join("fe.toml"),
        format!(
            "[ingot]\nname = \"cga_schedule32_app\"\nversion = \"0.1.0\"\n\n\
             [dependencies]\nsparse_clifford = {{ path = \"../../../ingots/sparse_clifford\" }}\n",
        ),
    )
    .expect("temporary Schedule32 fe.toml");
    std::fs::write(app_src.join("lib.fe"), &actor_source)
        .expect("temporary Schedule32 application source");
    let mut db = DriverDataBase::default();
    let app_url = Url::from_directory_path(app_dir.path()).expect("temporary app URL");
    assert!(
        !driver::init_ingot(&mut db, &app_url),
        "Schedule32 application ingot initialization diagnostics"
    );
    let app_ingot = db
        .workspace()
        .containing_ingot(&db, app_url)
        .expect("Schedule32 application ingot");
    let top_mod = app_ingot.root_mod(&db);
    let actor_diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        actor_diagnostics.is_empty(),
        "Schedule32 canonical actor source has diagnostics:\n{actor_diagnostics}"
    );
    let actor_declarations = [RENDER_LANE, VERIFY_LANE, ORACLE_LANE].map(|lane| {
        canonical_lane_decl_from_entry(&db, top_mod, lane, lane)
            .unwrap_or_else(|error| panic!("derive Schedule32 `{lane}` lane: {error}"))
    });
    for declaration in &actor_declarations[..2] {
        assert_eq!(declaration.intent.execution, CanonicalExecution::HostEffect);
        assert_eq!(declaration.intent.placement, CanonicalPlacement::MainThread);
        assert_eq!(declaration.intent.capabilities.len(), 1);
        assert_eq!(
            declaration.intent.capabilities[0].capability,
            CanonicalCapability::WebgpuDispatch
        );
        assert!(declaration.intent.capabilities[0].mutable);
    }
    assert_eq!(
        actor_declarations[2].intent.execution,
        CanonicalExecution::Wasm
    );
    assert_eq!(
        actor_declarations[2].intent.placement,
        CanonicalPlacement::Any
    );
    assert!(actor_declarations[2].intent.capabilities.is_empty());
    assert!(actor_declarations[2].export.is_some());
    CanonicalInterfaceManifest::build(actor_declarations.to_vec())
        .expect("Schedule32 compiler-derived canonical manifest");
    let all_app_roots =
        mir::build_wasm_runtime_package(&db, top_mod).expect("Schedule32 app runtime package");
    assert!(
        all_app_roots.functions(&db).iter().all(|function| {
            let symbol = function.symbol(&db);
            !symbol.contains("support_")
                && !symbol.contains("blade_set")
                && !symbol.contains("plan_mask_")
                && !symbol.contains("sparse_rank")
                && !symbol.contains("sparse_present")
        }),
        "public CTFE helpers from the sparse_clifford dependency must not become app runtime roots",
    );
    if std::env::var_os("FE_CGA_SCHEDULE32_HIR_ONLY").is_some() {
        eprintln!(
            "Schedule32 Vec5 render and canonical actor source: HIR clean \
             (backend intentionally skipped)"
        );
        return;
    }
    let package_started = std::time::Instant::now();
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, NAME)
        .expect("Schedule32 Vec5 runtime package");
    eprintln!(
        "Schedule32 Vec5 runtime package built in {:.3}s",
        package_started.elapsed().as_secs_f64()
    );
    assert!(
        package
            .functions(&db)
            .iter()
            .all(|f| f.linkage(&db) != mir::RuntimeLinkage::External)
    );
    if std::env::var_os("FE_CGA_SCHEDULE32_PACKAGE_ONLY").is_some() {
        eprintln!(
            "Schedule32 Vec5 composed source: runtime package clean (backends intentionally skipped)"
        );
        return;
    }

    let artifact = compile_runtime_package_spirv_render(&db, &package)
        .expect("Schedule32 Vec5 Render SPIR-V/WGSL");
    let wgsl = normalize_text(artifact.wgsl.as_ref().expect("Render WGSL"));
    assert_browser_wgsl(&wgsl);
    let layout = &artifact.layout;
    assert_eq!(layout.mode, LayoutMode::Render);
    assert_eq!(layout.entry_point, "fs_main");
    assert_eq!(layout.word, WordKind::U32);
    assert!(
        layout.result.is_none(),
        "Render output must stay on the color attachment"
    );
    assert_eq!(layout.vertex_entry.as_deref(), Some("vs_fullscreen"));
    assert_eq!(layout.fragment_entry.as_deref(), Some("fs_main"));
    assert_eq!(layout.color_target_format.as_deref(), Some("rgba8unorm"));

    let input = layout
        .bindings
        .iter()
        .find(|b| b.role == Role::Input)
        .expect("broadcast input");
    assert_eq!(
        (input.group, input.binding, input.access),
        (0, 1, Access::Read)
    );
    assert_eq!((input.span, input.stride, input.members.len()), (20, 20, 5));
    let names = ["cam_x", "cam_y", "zoom", "inv_cx", "inv_cy"];
    let params: Vec<_> = input
        .members
        .iter()
        .zip(names)
        .enumerate()
        .map(|(index, (m, name))| {
            assert_eq!(m.arg_index, index as u32 + 2);
            assert_eq!(m.offset, index as u32 * 4);
            assert_eq!(m.width, 4);
            assert_eq!(m.scalar, SpirvScalarKind::F32);
            serde_json::json!({
                "name": name, "arg_index": m.arg_index, "offset": m.offset,
                "width": m.width, "scalar": "F32",
            })
        })
        .collect();
    assert_eq!(layout.builtin_inputs.len(), 2);
    assert_eq!(layout.builtin_inputs[0].arg_index, 0);
    assert_eq!(layout.builtin_inputs[0].scalar, SpirvScalarKind::I32);
    assert_eq!(
        layout.builtin_inputs[0].source,
        SpirvBuiltinSource::FragmentPositionX
    );
    assert_eq!(
        layout.builtin_inputs[1].source,
        SpirvBuiltinSource::FragmentPositionY
    );
    assert_eq!(layout.builtin_inputs[1].arg_index, 1);
    assert_eq!(layout.builtin_inputs[1].scalar, SpirvScalarKind::I32);

    let wasm =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .expect("Schedule32 Vec5 rooted Wasm")
            .bytes;
    wasmparser::validate(&wasm).expect("valid Wasm");
    assert_zero_imports(&wasm);
    assert_eq!(
        function_exports(&wasm),
        vec![NAME.to_owned()],
        "entry-rooted kernel Wasm must not expose generated helpers or actor exports",
    );
    assert_eq!(
        export_signature(&wasm, NAME),
        (
            vec![
                WasmTy::I32,
                WasmTy::I32,
                WasmTy::F32,
                WasmTy::F32,
                WasmTy::F32,
                WasmTy::F32,
                WasmTy::F32,
            ],
            vec![WasmTy::I32],
        ),
        "browser fragment export ABI drift",
    );

    let frame = run_wasm_frame(&wasm);
    if std::env::var_os("FE_CGA_SCHEDULE32_ORACLE_AUDIT").is_some() {
        audit_frame_against_oracle(&frame);
        eprintln!(
            "oracle audit complete (assert acceptance and artifact writes intentionally skipped)"
        );
        return;
    }
    let mut sky = 0usize;
    let mut upper = 0usize;
    let mut lower = 0usize;
    let mut distinct = HashSet::new();
    let mut exact_mismatches = 0usize;
    let mut shade_deltas = BTreeMap::<i32, usize>::new();
    let mut max_abs_shade_delta = 0i32;
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let got = frame[(y * WIDTH + x) as usize];
            let (want, material) = oracle(x as i32, y as i32, CAM_X, CAM_Y, ZOOM, INV_CX, INV_CY);
            if got != want {
                exact_mismatches += 1;
            }
            assert_eq!(
                got, want,
                "Wasm/oracle pixel mismatch at ({x},{y}): got=0x{got:08x} want=0x{want:08x}",
            );
            assert_eq!(
                material_class(got),
                Some(material),
                "Wasm/oracle material mismatch at ({x},{y}): got=0x{got:08x} want=0x{want:08x}",
            );
            let family = palette_family(got);
            assert_eq!(
                family,
                palette_family(want),
                "Wasm/oracle palette mismatch at ({x},{y}): got=0x{got:08x} want=0x{want:08x}",
            );
            match family {
                PaletteFamily::Sky => {
                    assert_eq!(
                        got, want,
                        "the fixed sky color must be bit-exact at ({x},{y})"
                    );
                }
                PaletteFamily::Upper | PaletteFamily::Lower => {
                    let delta = shade_bucket(got).expect("classified Wasm shade")
                        - shade_bucket(want).expect("classified oracle shade");
                    *shade_deltas.entry(delta).or_default() += 1;
                    max_abs_shade_delta = max_abs_shade_delta.max(delta.abs());
                }
                PaletteFamily::Other => unreachable!("material assertion rejects other colors"),
            }
            distinct.insert(got);
            match material {
                0 => sky += 1,
                1 => upper += 1,
                2 => lower += 1,
                _ => unreachable!(),
            }
        }
    }
    assert!(sky > 0 && upper > 0 && lower > 0 && distinct.len() >= 8);
    assert_eq!(sky + upper + lower, (WIDTH * HEIGHT) as usize);
    let frame_hash = fnv1a32_words(&frame);

    let actor_bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(NAME, Some("cga_schedule32_actor.fe".to_owned()))
            .with_canonical_entries([RENDER_LANE, VERIFY_LANE, ORACLE_LANE])
            .with_canonical_policy(WebCanonicalPolicy::Required),
    )
    .expect("Schedule32 canonical actor WebBundle");
    assert_eq!(
        normalize_text(&actor_bundle.wgsl),
        wgsl,
        "adding canonical actor lanes must not change the browser render WGSL"
    );
    let bindings: Vec<_> = layout
        .bindings
        .iter()
        .map(|b| {
            serde_json::json!({
                "group": b.group, "binding": b.binding, "name": b.name,
                "access": if b.access == Access::Read { "Read" } else { "ReadWrite" },
                "role": if b.role == Role::Input { "Input" } else { "Output" },
                "span": b.span, "stride": b.stride,
            })
        })
        .collect();
    let builtins: Vec<_> = layout
        .builtin_inputs
        .iter()
        .map(|b| {
            serde_json::json!({
                "arg_index": b.arg_index,
                "scalar": match b.scalar { SpirvScalarKind::I32 => "I32", _ => "unexpected" },
                "source": match b.source {
                    SpirvBuiltinSource::FragmentPositionX => "FragmentPositionX",
                    SpirvBuiltinSource::FragmentPositionY => "FragmentPositionY",
                    _ => "unexpected",
                },
            })
        })
        .collect();
    let provenance = provenance(repo, &source);
    let schedule = independently_derived_schedule();
    assert_eq!(schedule.len(), 32);
    let schedule_json = schedule
        .iter()
        .map(|tuple| {
            serde_json::json!([
                tuple.left,
                tuple.point,
                tuple.right,
                tuple.output,
                tuple.magnitude,
                tuple.negative
            ])
        })
        .collect::<Vec<_>>();
    let layout_json = serde_json::to_string_pretty(&serde_json::json!({
        "kernel": NAME, "entry_point": layout.entry_point, "mode": "Render",
        "word": "U32", "word_bytes": 4, "vertex_entry": layout.vertex_entry,
        "fragment_entry": layout.fragment_entry, "color_target_format": layout.color_target_format,
        "bindings": bindings, "builtin_inputs": builtins, "params": params,
        "frag_wasm_export": NAME, "frag_wasm_bytes": wasm.len(),
        "actor_bundle": "actor/manifest.json",
        "actor_wasm": "actor/module.wasm",
        "actor_interface": "actor/interface.js",
        "actor_lanes": [RENDER_LANE, VERIFY_LANE, ORACLE_LANE],
        "source_model": "application ingot with sparse_clifford path dependency",
        "application_manifest": "app/fe.toml",
        "application_source": "app/src/lib.fe",
        "kernel_source": "kernel.fe (dependency-backed; not standalone)",
        "kernel_wasm_scope": "entry-rooted fragment only; zero imports; no generated helper or actor exports",
        "actor_wasm_scope": "separate canonical WebBundle owning actor exports and arena",
        "width": WIDTH, "height": HEIGHT, "provenance": provenance.clone(),
    }))
    .unwrap();
    let reference_json = serde_json::to_string_pretty(&serde_json::json!({
        "fragment": NAME, "width": WIDTH, "height": HEIGHT,
        "view": [CAM_X, CAM_Y, ZOOM], "inversion_center": [INV_CX, INV_CY],
        "parameter_types": ["F32", "F32", "F32", "F32", "F32"],
        "shape": "inverted_offset_torus_cyclide",
        "algebra": "canonical Fe helpers shared by a forced typed Schedule<32> witness and a bounded 80-candidate FCO provider that emits five direct lanes with the canonical four-bucket balanced reduction",
        "inversion_center_runtime": true, "fnv1a32": frame_hash,
        "sky_pixels": sky, "hit_pixels": upper + lower,
        "upper_pixels": upper, "lower_pixels": lower, "distinct_colors": distinct.len(),
        "oracle_agreement": {
            "policy": "bit-exact 16384/16384 pixels against the independent Rust f32 oracle",
            "pixels": WIDTH * HEIGHT,
            "exact_pixels": WIDTH as usize * HEIGHT as usize - exact_mismatches,
            "exact_mismatches": exact_mismatches,
            "shade_bucket_delta_histogram": shade_deltas,
            "max_abs_shade_bucket_delta": max_abs_shade_delta,
        },
        "runtime": "wasmtime executing dependency-backed Fe application-ingot Wasm; every pixel semantically checked against independent Rust f32 oracle",
        "artifact_separation": {
            "frag_wasm": "entry-rooted kernel package; render export only; zero imports; no generated helper or actor exports",
            "actor_wasm": "canonical WebBundle package; owns actor exports and arena",
        },
        "schedule_tuple_fields": ["left_blade", "point_blade", "right_blade", "output_blade", "magnitude", "negative"],
        "canonical_survivor_tuples": schedule_json,
        "runtime_tuple_order": "canonical candidate scan order; four ordinal buckets of eight, term-prepend within each bucket, then balanced (3+2)+(1+0) reduction",
        "provenance": provenance,
    })).unwrap();

    // Nothing is written before compilation and all structural/ABI gates pass.
    std::fs::create_dir_all(&out).unwrap_or_else(|e| panic!("{}: {e}", out.display()));
    write(&out.join("kernel.fe"), source.as_bytes());
    write(
        &out.join("app/fe.toml"),
        b"[ingot]\nname = \"cga_schedule32_app\"\nversion = \"0.1.0\"\n\n\
[dependencies]\nsparse_clifford = { path = \"../../../../ingots/sparse_clifford\" }\n",
    );
    write(&out.join("app/src/lib.fe"), actor_source.as_bytes());
    write(&out.join("frag.wgsl"), wgsl.as_bytes());
    write(&out.join("frag.wasm"), &wasm);
    for file in actor_bundle
        .materialized_files()
        .expect("Schedule32 actor WebBundle materializes")
    {
        write(&out.join("actor").join(file.path()), file.bytes());
    }
    write(&out.join("actor-source.fe"), actor_source.as_bytes());
    write(&out.join("layout.json"), layout_json.as_bytes());
    write(&out.join("reference.json"), reference_json.as_bytes());
    eprintln!(
        "staged Schedule32 Vec5 browser artifacts in {}",
        out.display()
    );
}

fn assert_browser_wgsl(wgsl: &str) {
    for token in ["@vertex", "@fragment", "loop", "sqrt(", "unpack4x8unorm"] {
        assert!(wgsl.contains(token), "WGSL lacks {token}");
    }
    assert!(!wgsl.contains("i64") && !wgsl.contains("u64"));
    let module = naga::front::wgsl::parse_str(wgsl).expect("reparse WGSL");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("browser-profile WGSL");
}

fn normalize_text(text: &str) -> String {
    let mut normalized = text
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    normalized.push('\n');
    normalized
}

fn assert_zero_imports(bytes: &[u8]) {
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let wasmparser::Payload::ImportSection(reader) = payload.expect("Wasm payload") {
            assert_eq!(reader.count(), 0, "browser Wasm must have zero imports");
        }
    }
}

fn function_exports(bytes: &[u8]) -> Vec<String> {
    use wasmparser::{ExternalKind, Payload};
    let mut names = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        if let Payload::ExportSection(reader) = payload.expect("Wasm payload") {
            for export in reader {
                let export = export.expect("Wasm export");
                if export.kind == ExternalKind::Func {
                    names.push(export.name.to_owned());
                }
            }
        }
    }
    names
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WasmTy {
    I32,
    F32,
    Other,
}

fn export_signature(bytes: &[u8], name: &str) -> (Vec<WasmTy>, Vec<WasmTy>) {
    use wasmparser::{ExternalKind, Payload, TypeRef, ValType};
    let map = |v: &ValType| match v {
        ValType::I32 => WasmTy::I32,
        ValType::F32 => WasmTy::F32,
        _ => WasmTy::Other,
    };
    let mut signatures = Vec::new();
    let mut defined_types = Vec::new();
    let mut imported_functions = 0u32;
    let mut exported = None;
    for payload in wasmparser::Parser::new(0).parse_all(bytes) {
        match payload.expect("valid Wasm payload") {
            Payload::TypeSection(reader) => {
                for group in reader {
                    for subtype in group.expect("type group").into_types() {
                        let f = subtype.unwrap_func();
                        signatures.push((
                            f.params().iter().map(map).collect(),
                            f.results().iter().map(map).collect(),
                        ));
                    }
                }
            }
            Payload::ImportSection(reader) => {
                for import in reader.into_imports() {
                    if matches!(import.expect("import").ty, TypeRef::Func(_)) {
                        imported_functions += 1;
                    }
                }
            }
            Payload::FunctionSection(reader) => {
                for ty in reader {
                    defined_types.push(ty.expect("function type"));
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export = export.expect("export");
                    if export.name == name {
                        assert_eq!(export.kind, ExternalKind::Func);
                        exported = Some(export.index);
                    }
                }
            }
            _ => {}
        }
    }
    let index = exported.expect("fragment Wasm export missing");
    assert!(index >= imported_functions);
    signatures[defined_types[(index - imported_functions) as usize] as usize].clone()
}

fn run_wasm_frame(bytes: &[u8]) -> Vec<u32> {
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).expect("Wasm module");
    let workers = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .min(8)
        .min(HEIGHT as usize);
    let rows_per_worker = (HEIGHT as usize).div_ceil(workers);
    let mut chunks = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for worker in 0..workers {
            let y_start = worker * rows_per_worker;
            let y_end = ((worker + 1) * rows_per_worker).min(HEIGHT as usize);
            if y_start == y_end {
                continue;
            }
            let engine = engine.clone();
            let module = module.clone();
            handles.push(scope.spawn(move || {
                let mut store = wasmtime::Store::new(&engine, ());
                let instance = wasmtime::Instance::new(&mut store, &module, &[])
                    .expect("zero-import instance");
                let render = instance
                    .get_typed_func::<(i32, i32, f32, f32, f32, f32, f32), i32>(&mut store, NAME)
                    .expect("typed fragment export");
                let mut pixels = Vec::with_capacity((y_end - y_start) * WIDTH as usize);
                for y in y_start as i32..y_end as i32 {
                    for x in 0..WIDTH as i32 {
                        pixels.push(
                            render
                                .call(&mut store, (x, y, CAM_X, CAM_Y, ZOOM, INV_CX, INV_CY))
                                .expect("Wasm pixel") as u32,
                        );
                    }
                }
                (y_start, pixels)
            }));
        }
        handles
            .into_iter()
            .map(|handle| handle.join().expect("Wasm frame worker"))
            .collect::<Vec<_>>()
    });
    chunks.sort_by_key(|(y_start, _)| *y_start);
    let frame: Vec<_> = chunks.into_iter().flat_map(|(_, pixels)| pixels).collect();
    assert_eq!(frame.len(), (WIDTH * HEIGHT) as usize);
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
    let inv_len = 1.0 / (sx * sx + sy * sy + 1.8_f32 * 1.8_f32).sqrt();
    let (rdx, rdy, rdz) = (sx * inv_len, sy * inv_len, 1.8_f32 * inv_len);
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
        let ring = (tx * tx + ty * ty).sqrt() - 0.58;
        let base = (ring * ring + qz * qz).sqrt() - 0.17;
        let distance = base * safe_rho2;
        t += distance * 0.18;
        if distance < 0.0022 {
            let shade = 38 + 24 * (i >> 3);
            if qy > 0.0 {
                return (
                    (shade + 88 * 256 + (255 - shade) * 65_536 - 16_777_216) as u32,
                    1,
                );
            }
            return ((56 + shade * 256 + 224 * 65_536 - 16_777_216) as u32, 2);
        }
        i += 1;
    }
    ((7 + 11 * 256 + 25 * 65_536 - 16_777_216) as u32, 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PaletteFamily {
    Sky,
    Upper,
    Lower,
    Other,
}

fn rgba(color: u32) -> [u8; 4] {
    color.to_le_bytes()
}

fn palette_family(color: u32) -> PaletteFamily {
    let [r, g, b, a] = rgba(color);
    if [r, g, b, a] == [7, 11, 25, 255] {
        PaletteFamily::Sky
    } else if a == 255 && g == 88 && u16::from(r) + u16::from(b) == 255 {
        PaletteFamily::Upper
    } else if a == 255 && r == 56 && b == 224 {
        PaletteFamily::Lower
    } else {
        PaletteFamily::Other
    }
}

fn material_class(color: u32) -> Option<u8> {
    match palette_family(color) {
        PaletteFamily::Sky => Some(0),
        PaletteFamily::Upper => Some(1),
        PaletteFamily::Lower => Some(2),
        PaletteFamily::Other => None,
    }
}

fn shade_bucket(color: u32) -> Option<i32> {
    let [r, g, _, _] = rgba(color);
    let shade = match palette_family(color) {
        PaletteFamily::Upper => i32::from(r),
        PaletteFamily::Lower => i32::from(g),
        PaletteFamily::Sky | PaletteFamily::Other => return None,
    };
    let delta = shade - 38;
    (delta >= 0 && delta % 24 == 0).then_some(delta / 24)
}

fn audit_frame_against_oracle(frame: &[u32]) {
    const FIRST_LIMIT: usize = 24;
    let pixels = (WIDTH * HEIGHT) as usize;
    assert_eq!(frame.len(), pixels);
    let mut exact_mismatches = 0usize;
    let mut material_mismatches = 0usize;
    let mut palette_mismatches = 0usize;
    let mut shade_deltas = BTreeMap::<i32, usize>::new();
    let mut max_abs_shade_delta = 0i32;
    let mut shade_unclassified = 0usize;
    let mut got_sky = 0usize;
    let mut got_hit = 0usize;
    let mut got_other = 0usize;
    let mut want_sky = 0usize;
    let mut want_hit = 0usize;
    let mut first = Vec::new();

    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let got = frame[(y * WIDTH + x) as usize];
            let (want, want_material) =
                oracle(x as i32, y as i32, CAM_X, CAM_Y, ZOOM, INV_CX, INV_CY);
            let got_family = palette_family(got);
            let want_family = palette_family(want);
            match got_family {
                PaletteFamily::Sky => got_sky += 1,
                PaletteFamily::Upper | PaletteFamily::Lower => got_hit += 1,
                PaletteFamily::Other => got_other += 1,
            }
            if want_material == 0 {
                want_sky += 1;
            } else {
                want_hit += 1;
            }
            if got != want {
                exact_mismatches += 1;
                if first.len() < FIRST_LIMIT {
                    first.push((x, y, got, want, got_family, want_family));
                }
            }
            if material_class(got) != Some(want_material) {
                material_mismatches += 1;
            }
            if got_family != want_family {
                palette_mismatches += 1;
            }
            if got_family == want_family
                && matches!(got_family, PaletteFamily::Upper | PaletteFamily::Lower)
            {
                match (shade_bucket(got), shade_bucket(want)) {
                    (Some(got_bucket), Some(want_bucket)) => {
                        let delta = got_bucket - want_bucket;
                        *shade_deltas.entry(delta).or_default() += 1;
                        max_abs_shade_delta = max_abs_shade_delta.max(delta.abs());
                    }
                    _ => shade_unclassified += 1,
                }
            }
        }
    }

    eprintln!("=== Schedule32 Wasm/oracle full-frame audit ===");
    eprintln!(
        "exact mismatches: {exact_mismatches}/{pixels} ({:.6}%)",
        exact_mismatches as f64 * 100.0 / pixels as f64
    );
    eprintln!(
        "material classification mismatches: {material_mismatches}/{pixels} ({:.6}%)",
        material_mismatches as f64 * 100.0 / pixels as f64
    );
    eprintln!(
        "palette-family mismatches: {palette_mismatches}/{pixels} ({:.6}%)",
        palette_mismatches as f64 * 100.0 / pixels as f64
    );
    eprintln!(
        "sky/hit counts: wasm={got_sky}/{got_hit} oracle={want_sky}/{want_hit}; wasm_other={got_other}"
    );
    eprintln!(
        "shade bucket delta histogram (wasm-oracle): {shade_deltas:?}; max_abs={max_abs_shade_delta}; unclassified={shade_unclassified}"
    );
    eprintln!("first {} exact mismatches:", first.len());
    for (x, y, got, want, got_family, want_family) in first {
        eprintln!(
            "  ({x:3},{y:3}) got=0x{got:08x} rgba={:?} {got_family:?}; want=0x{want:08x} rgba={:?} {want_family:?}",
            rgba(got),
            rgba(want),
        );
    }
    assert_eq!(
        exact_mismatches, 0,
        "Schedule32 Wasm frame must be bit-exact against the independent oracle"
    );
}

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    for byte in bytes {
        hash = (hash ^ u32::from(*byte)).wrapping_mul(0x0100_0193);
    }
    hash
}

fn fnv1a32_words(words: &[u32]) -> u32 {
    let mut hash = 0x811c_9dc5_u32;
    for word in words {
        for byte in word.to_le_bytes() {
            hash = (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193);
        }
    }
    hash
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ScheduleTuple {
    left: usize,
    point: usize,
    right: usize,
    output: usize,
    magnitude: usize,
    negative: usize,
}

fn independently_derived_schedule() -> Vec<ScheduleTuple> {
    (0usize..80)
        .filter(|&triple| keep_tag_rust(triple) != 0)
        .map(|triple| {
            let left = sphere_blade_rust(triple / 20);
            let point = 1usize << ((triple / 4) % 5);
            let right = sphere_blade_rust(triple % 4);
            ScheduleTuple {
                left,
                point,
                right,
                output: left ^ point ^ right,
                magnitude: 2 - usize::from(triple / 20 == triple % 4),
                negative: gp_negative_rust(left, point) ^ gp_negative_rust(left ^ point, right),
            }
        })
        .collect()
}

fn sphere_blade_rust(slot: usize) -> usize {
    1usize << (slot + slot / 2)
}

fn gp_negative_rust(a: usize, b: usize) -> usize {
    let mut swaps = 0usize;
    for bit in 1..5 {
        swaps += ((a >> bit) & 1) * (b & ((1usize << bit) - 1)).count_ones() as usize;
    }
    (swaps + ((a >> 4) & 1) * ((b >> 4) & 1)) & 1
}

fn keep_tag_rust(triple: usize) -> usize {
    let left_slot = triple / 20;
    let right_slot = triple % 4;
    if left_slot > right_slot {
        return 0;
    }
    let left = sphere_blade_rust(left_slot);
    let point = 1usize << ((triple / 4) % 5);
    let right = sphere_blade_rust(right_slot);
    let forward = gp_negative_rust(left, point) ^ gp_negative_rust(left ^ point, right);
    let reverse = gp_negative_rust(right, point) ^ gp_negative_rust(right ^ point, left);
    usize::from(forward == reverse)
}

fn git_output(path: &std::path::Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .expect("git command");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("UTF-8 git output")
        .trim()
        .to_owned()
}

fn provenance(repo: &std::path::Path, source: &str) -> serde_json::Value {
    let sonatina = PathBuf::from(
        std::env::var("SONATINA_DIR")
            .expect("SONATINA_DIR must identify the Sonatina checkout used by Cargo patches"),
    );
    assert!(
        sonatina.join("crates/codegen").is_dir(),
        "invalid SONATINA_DIR"
    );
    let fe_rev = std::env::var("FE_CGA_SOURCE_REV")
        .expect("use generate.sh to capture clean Fe provenance before Cargo patches the lockfile");
    assert_eq!(
        fe_rev,
        git_output(repo, &["rev-parse", "HEAD"]),
        "Fe HEAD changed after generator preflight",
    );
    let fe_tracked_status =
        git_output(repo, &["status", "--porcelain", "--untracked-files=no"]);
    assert!(
        fe_tracked_status.is_empty() || fe_tracked_status == "M Cargo.lock",
        "Fe checkout changed after generator preflight: {fe_tracked_status:?}",
    );
    let fe_untracked_present =
        match std::env::var("FE_CGA_SOURCE_UNTRACKED_PRESENT").as_deref() {
            Ok("0") => false,
            Ok("1") => true,
            _ => panic!("invalid FE_CGA_SOURCE_UNTRACKED_PRESENT generator preflight value"),
        };
    let sonatina_rev = git_output(&sonatina, &["rev-parse", "HEAD"]);
    let sonatina_status = git_output(
        &sonatina,
        &["status", "--porcelain", "--untracked-files=normal"],
    );
    serde_json::json!({
        "source": "Schedule32 application ingot depending on the public sparse_clifford ingot",
        "fe_rev": fe_rev,
        "fe_dirty": false,
        "fe_untracked_present": fe_untracked_present,
        "fe_status_fnv1a32": fnv1a32(b""),
        "sonatina_path": sonatina.to_string_lossy(),
        "sonatina_rev": sonatina_rev,
        "sonatina_dirty": !sonatina_status.is_empty(),
        "sonatina_status_fnv1a32": fnv1a32(sonatina_status.as_bytes()),
        "canonical_fixture": "crates/codegen/tests/fixtures/fco_cga80_direct_lanes.fe",
        "sparse_clifford_fixture": "ingots/sparse_clifford/src/lib.fe",
        "published_app_manifest": "demos/webgpu-cga-inversion/gen-schedule32/app/fe.toml",
        "published_app_source": "demos/webgpu-cga-inversion/gen-schedule32/app/src/lib.fe",
        "published_dependency_path": "../../../../ingots/sparse_clifford",
        "kernel_source_self_contained": false,
        "body_fixture": "crates/codegen/tests/fixtures/spirv/fco_cga80_direct_de_body.fe",
        "canonical_fnv1a32": fnv1a32(CANONICAL.as_bytes()),
        "sparse_clifford_api_fnv1a32": fnv1a32(SPARSE_CLIFFORD_API.as_bytes()),
        "body_fnv1a32": fnv1a32(BODY.as_bytes()),
        "application_kernel_source_fnv1a32": fnv1a32(source.as_bytes()),
        "algebra": "CTFE-derived 80-to-32 typed witness; bounded FCO provider emits one shared five-lane sandwich aggregate from the same helpers",
        "generated_unix_secs": SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
    })
}

fn write(path: &std::path::Path, bytes: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|e| panic!("{}: {e}", parent.display()));
    }
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}
