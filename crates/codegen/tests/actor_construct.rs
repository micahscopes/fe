//! Acceptance for the R-A1 `actor` construct on the DEC render program.
//!
//! `DecSurface` is declared as an `actor` in `demos/sketches/dec/src/lib.fe`.
//! Two claims are gated here:
//!
//!  1. The render entry and mode the CLI used to pass as
//!     `--entry dec_render --mode render` are DERIVED from the actor
//!     declaration, and explicit flags that contradict it are rejected.
//!  2. The `actor` desugar reproduces the flattened free kernel a hand-written
//!     `pub fn` would emit, byte for byte.
//!
//! Note on the DEC render bundle itself: it cannot be lowered to wasm or
//! SPIR-V today because `dec_render` uses `!=` (via `laplacian0`) and the
//! wasm/SPIR-V "R1" path does not yet lower `NotEq` (a pre-existing R2 gap that
//! blocks the flag path and the actor path identically). So claim 2's BYTE
//! comparison runs on a small buildable analog kernel, while claim 1 runs on
//! the real DEC actor; together they establish that the actor reproduces the
//! flag-built inputs with zero backend change.

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    CanonicalType, WasmCompileOptions, WebActorPassCycle, WebActorResourceElement,
    WebActorStageKind, WebBuildOptions, WebBuiltinSource, WebBundle, WebBundleMode,
    WebDepthCompare, WebDepthFormat, WebFaceCull, WebResourceAccess, WebResourceInitialization,
    WebResourceKind, WebResourcePolicy, WebResourceRecovery, WebResourceResidency,
    WebResourceVisibility, actor_gpu_program, actor_web_entry,
    compile_runtime_package_spirv_compute_with_resources,
    compile_runtime_package_spirv_render_with_resources, compile_runtime_package_wasm_with_options,
    resolve_web_entry,
};
use hir::hir_def::{GpuResource, HirIngot, TopLevelMod};
use sonatina_codegen::isa::spirv::{
    Access, SpirvExternalResource, SpirvResourceElement, SpirvResourceField, SpirvScalarKind,
};
use url::Url;

fn ingot_root(relative: &str) -> Url {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn ingot_top_mod<'db>(db: &'db DriverDataBase, url: &Url) -> TopLevelMod<'db> {
    let ingot = db
        .workspace()
        .containing_ingot(db, url.clone())
        .expect("ingot");
    ingot.root_mod(db)
}

fn build_entry_wasm(db: &DriverDataBase, top_mod: TopLevelMod<'_>, entry: &str) -> Vec<u8> {
    let package =
        mir::build_wasm_runtime_package_for_entries(db, top_mod, &[entry.to_string()]).unwrap();
    compile_runtime_package_wasm_with_options(
        db,
        &package,
        WasmCompileOptions::default().with_optimization(),
    )
    .unwrap()
    .bytes
}

/// Opens the DEC ingot and returns its clean top module (init + no diagnostics).
fn dec_top_mod<'db>(db: &'db DriverDataBase, url: &Url) -> TopLevelMod<'db> {
    let top_mod = ingot_top_mod(db, url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(db);
    assert!(
        diagnostics.is_empty(),
        "unexpected dec diagnostics:\n{diagnostics}"
    );
    top_mod
}

#[test]
fn dec_actor_reproduces_the_flag_built_bundle() {
    // The DEC render program is declared as an `actor`; the render entry and
    // mode the flags used to supply are DERIVED from that declaration, so the
    // derivation path reproduces exactly the build inputs the flag path used.
    let mut db = DriverDataBase::default();
    let url = ingot_root("../../demos/sketches/dec");
    assert!(
        !driver::init_ingot(&mut db, &url),
        "dec ingot init diagnostics"
    );
    let top_mod = dec_top_mod(&db, &url);

    // Zero-config: no flags supplied, entry + mode derived from the actor.
    let derived = resolve_web_entry(&db, top_mod, None, None).expect("derivation");
    assert_eq!(derived, ("dec_render".to_string(), WebBundleMode::Render));

    // The same fact, read directly off the declaration.
    assert_eq!(
        actor_web_entry(&db, top_mod).unwrap(),
        Some(("dec_render".to_string(), WebBundleMode::Render)),
    );

    // Explicit flags that MATCH the declaration are accepted and reconcile to
    // the identical build inputs the flag path built from.
    let reconciled = resolve_web_entry(
        &db,
        top_mod,
        Some("dec_render".to_string()),
        Some(WebBundleMode::Render),
    )
    .expect("matching flags");
    assert_eq!(reconciled, derived);
}

#[test]
fn flags_contradicting_the_actor_are_rejected() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("../../demos/sketches/dec");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = dec_top_mod(&db, &url);

    // An explicit entry that is not the actor's fragment behavior.
    let err = resolve_web_entry(
        &db,
        top_mod,
        Some("not_dec_render".to_string()),
        Some(WebBundleMode::Render),
    )
    .unwrap_err();
    let text = format!("{err}");
    assert!(
        text.contains("not_dec_render") && text.contains("dec_render"),
        "{text}"
    );

    // An explicit mode that contradicts the derived render mode.
    let err = resolve_web_entry(
        &db,
        top_mod,
        Some("dec_render".to_string()),
        Some(WebBundleMode::Grid),
    )
    .unwrap_err();
    assert!(format!("{err}").contains("contradicts"), "{err}");
}

#[test]
fn actor_without_a_unique_fragment_behavior_is_rejected() {
    // Two role-marked behaviors in one actor: no unique render entry to pick.
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_two_fragment");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let err = actor_web_entry(&db, top_mod).unwrap_err();
    assert!(format!("{err}").contains("fragment-stage"), "{err}");

    // An actor with the placement row but no fragment behavior at all.
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_no_fragment");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let err = actor_web_entry(&db, top_mod).unwrap_err();
    assert!(format!("{err}").contains("gpu_stage(fragment)"), "{err}");
}

#[test]
fn authored_raster_roles_derive_one_nominal_typed_varying() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_raster_typed");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "typed raster diagnostics:\n{diagnostics}"
    );

    let program = actor_gpu_program(&db, top_mod)
        .expect("typed raster plan")
        .expect("GPU actor");
    assert_eq!(program.actor, "TypedMesh");
    assert_eq!(program.stages.len(), 2);
    assert_eq!(program.stages[0].source_entry, "vertices");
    assert_eq!(program.stages[1].source_entry, "shade");
    let WebActorStageKind::Vertex {
        varying: vertex,
        vertex_count,
    } = &program.stages[0].kind
    else {
        panic!(
            "expected authored vertex stage, got {:?}",
            program.stages[0].kind
        );
    };
    assert_eq!(*vertex_count, 3);
    let WebActorStageKind::RasterFragment { varying: fragment } = &program.stages[1].kind else {
        panic!(
            "expected authored raster fragment, got {:?}",
            program.stages[1].kind
        );
    };
    assert_eq!(
        vertex, fragment,
        "the paired stages must share one derived payload"
    );
    let CanonicalType::Record(fields) = vertex else {
        panic!("varying must remain a named-field record: {vertex:?}");
    };
    assert_eq!(
        fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["normal", "heat"]
    );
    let CanonicalType::Record(normal) = &fields[0].ty else {
        panic!("normal must retain its nested record structure");
    };
    assert_eq!(
        normal
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["x", "y", "z"]
    );
    assert!(normal.iter().all(|field| field.ty == CanonicalType::F32));
    assert_eq!(fields[1].ty, CanonicalType::F32);
    assert_eq!(
        actor_web_entry(&db, top_mod).unwrap(),
        Some(("shade".to_owned(), WebBundleMode::Render)),
    );

    // Compile the pair as one real raster pipeline. These assertions check the
    // typed interface and Fe-authored source logic, not only artifact bytes.
    let bundle = WebBundle::compile(&db, top_mod, WebBuildOptions::render("shade", None))
        .expect("typed authored raster bundle");
    assert_eq!(bundle.manifest.passes.len(), 1);
    let pass = &bundle.manifest.passes[0];
    assert_eq!(pass.draw_vertices, Some(3));
    assert_eq!(pass.layout.vertex_entry.as_deref(), Some("vertices"));
    assert_eq!(pass.layout.fragment_entry.as_deref(), Some("shade"));
    assert_eq!(pass.layout.bindings.len(), 1);
    assert_eq!(pass.layout.bindings[0].members[0].name, "tint");
    assert!(bundle.wgsl.contains("@vertex"), "{}", bundle.wgsl);
    assert!(bundle.wgsl.contains("@fragment"), "{}", bundle.wgsl);
    assert!(bundle.wgsl.contains("@location(3)"), "{}", bundle.wgsl);
    assert!(bundle.wgsl.contains("unpack4x8unorm"), "{}", bundle.wgsl);
}

#[test]
fn authored_raster_shares_one_content_addressed_resource_across_both_stages() {
    const BYTES: &[u8] = b"0123456789abcde\n";
    const SHA256: &str = "dc08b6f2c7aaeca6d88cd9c82797b328160ccb3b1a84243b8eadb296744426c4";

    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_raster_content_addressed");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "raster resource diagnostics:\n{diagnostics}"
    );

    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("shade", Some("raster-resource.fe".to_owned()))
            .with_resource_asset(BYTES.to_vec()),
    )
    .expect("content-addressed authored raster bundle");
    let [pass] = bundle.manifest.passes.as_slice() else {
        panic!("one authored pair must produce one raster pass")
    };
    let [binding] = pass.layout.bindings.as_slice() else {
        panic!("one actor resource must produce one shared binding")
    };
    assert_eq!(binding.role, fe_codegen::WebBindingRole::Resource);
    assert_eq!(binding.name, "fixture");
    assert_eq!(binding.binding, 0);
    assert_eq!(pass.layout.vertex_entry.as_deref(), Some("vertices"));
    assert_eq!(pass.layout.fragment_entry.as_deref(), Some("shade"));
    assert!(
        bundle.wgsl.contains("var<storage> fixture"),
        "{}",
        bundle.wgsl
    );

    let [resource] = bundle.manifest.resources.as_slice() else {
        panic!("one content-addressed actor resource must be materialized once")
    };
    assert_eq!(
        resource.policy.initialization,
        WebResourceInitialization::ContentAddressed {
            sha256: SHA256.to_owned(),
        }
    );
}

#[test]
fn fullscreen_and_authored_raster_form_one_ordered_fe_pass_graph() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_layered_raster");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "layered raster diagnostics:\n{diagnostics}"
    );

    let program = actor_gpu_program(&db, top_mod)
        .expect("derive layered actor")
        .expect("GPU actor");
    assert_eq!(program.actor, "LayeredSurface");
    assert_eq!(program.stages.len(), 3);
    assert_eq!(program.stages[0].source_entry, "background");
    assert_eq!(program.stages[0].kind, WebActorStageKind::Fragment);
    assert!(matches!(
        program.stages[1].kind,
        WebActorStageKind::Vertex {
            vertex_count: 6,
            ..
        }
    ));
    assert!(matches!(
        program.stages[2].kind,
        WebActorStageKind::RasterFragment { .. }
    ));
    let raster = program.raster.as_ref().expect("Fe-authored raster plan");
    assert_eq!(raster.sample_count, 4);
    assert_eq!(raster.cull_mode, WebFaceCull::None);
    let depth = raster.depth.as_ref().expect("Fe-authored depth attachment");
    assert_eq!(depth.format, WebDepthFormat::Depth24Plus);
    assert_eq!(depth.compare, WebDepthCompare::LessEqual);
    assert!(depth.write_enabled);
    assert_eq!(depth.clear, 1.0);
    assert_eq!(
        actor_web_entry(&db, top_mod).unwrap(),
        Some(("background".to_owned(), WebBundleMode::Render)),
        "the fullscreen surface remains the page-facing entry; source order carries the overlay",
    );

    let bundle = WebBundle::compile(&db, top_mod, WebBuildOptions::render("background", None))
        .expect("compile layered render graph");
    assert_eq!(bundle.manifest.source_entry, "background");
    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(bundle.pass_wgsl.len(), 2);
    let base = &bundle.manifest.passes[0];
    let overlay = &bundle.manifest.passes[1];
    assert_eq!(base.source_entry, "background");
    assert_eq!(bundle.manifest.artifacts.wgsl, base.shader);
    assert_eq!(bundle.wgsl, bundle.pass_wgsl[0].source);
    assert_eq!(base.draw_vertices, None);
    assert_eq!(base.layout.fragment_entry.as_deref(), Some("fs_main"));
    assert_eq!(overlay.source_entry, "overlay_fragment");
    assert_eq!(overlay.draw_vertices, Some(6));
    assert_eq!(
        overlay.layout.vertex_entry.as_deref(),
        Some("overlay_vertices")
    );
    assert_eq!(
        overlay.layout.fragment_entry.as_deref(),
        Some("overlay_fragment")
    );
    assert_eq!(
        base.layout.bindings[0].members[0].name, overlay.layout.bindings[0].members[0].name,
        "both passes derive the same Fe actor-state identity",
    );
    assert_eq!(
        base.layout.bindings[0]
            .members
            .iter()
            .map(|member| member.name.as_str())
            .collect::<Vec<_>>(),
        ["tint", "lower.x", "lower.y", "upper.x", "upper.y"],
        "repeated nested vector leaves derive their shortest unique semantic paths",
    );
    assert_eq!(
        base.layout.bindings[0]
            .members
            .iter()
            .map(|member| member.name.as_str())
            .collect::<Vec<_>>(),
        overlay.layout.bindings[0]
            .members
            .iter()
            .map(|member| member.name.as_str())
            .collect::<Vec<_>>(),
        "every pass sees one compiler-derived nested state layout",
    );
    assert!(bundle.pass_wgsl[0].source.contains("@fragment"));
    assert!(bundle.pass_wgsl[1].source.contains("@vertex"));
    assert!(bundle.pass_wgsl[1].source.contains("@fragment"));
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bundle.wasm).expect("quality-only Wasm module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("quality-only Wasm instance");
    let quality = instance
        .get_typed_func::<(f32, f32, f32, f32, f32, f32, i32, i32), (f32, f32)>(
            &mut store,
            "fe_surface_quality_v1",
        )
        .expect("fixed quality export");
    assert_eq!(
        quality
            .call(&mut store, (900.0, 700.0, 3.0, 640.0, 360.0, 4096.0, 1, 1),)
            .expect("execute fixture-selected quality policy"),
        (320.0, 180.0),
        "the exact source-selected policy must execute; neither browser nor compiler may substitute the standard policy",
    );
    let exports = wasmparser::Parser::new(0)
        .parse_all(&bundle.wasm)
        .filter_map(|payload| match payload.expect("quality Wasm payload") {
            wasmparser::Payload::ExportSection(section) => Some(
                section
                    .into_iter()
                    .map(|entry| entry.expect("quality export").name.to_owned())
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert!(exports.iter().any(|name| name == "fe_surface_quality_v1"));
    assert!(
        !exports.iter().any(|name| name == "decide_fixture"),
        "the authored policy method must remain private",
    );
}

#[test]
fn authored_raster_rejects_mismatched_nominal_payloads() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_raster_mismatch");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "mismatch fixture diagnostics:\n{diagnostics}"
    );
    let error = actor_gpu_program(&db, top_mod).unwrap_err();
    assert!(
        format!("{error}").contains("different varying payload types"),
        "unexpected mismatch diagnostic: {error}",
    );
}

#[test]
fn attributed_aliases_derive_compute_resource_and_fragment_plan() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_compute_storage");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "compute/storage fixture diagnostics:\n{diagnostics}"
    );

    let program = actor_gpu_program(&db, top_mod)
        .expect("attributed GPU plan")
        .expect("GPU actor");
    assert_eq!(program.actor, "KnownColor");
    assert_eq!(program.stages.len(), 2);
    assert_eq!(program.stages[0].source_entry, "seed");
    assert_eq!(
        program.stages[0].kind,
        WebActorStageKind::Compute {
            workgroup_size: [1, 1, 1],
            dispatch: [1, 1, 1],
            repeat: 1,
            taper: None,
            cooperation: None,
            cycle: None,
            invocation_context: false,
        }
    );
    assert_eq!(program.stages[1].source_entry, "paint");
    assert_eq!(program.stages[1].kind, WebActorStageKind::Fragment);
    assert_eq!(program.resources.len(), 1);
    let orbit = &program.resources[0];
    assert_eq!(
        (orbit.field_index, orbit.name.as_str(), orbit.length),
        (0, "orbit", 1)
    );
    assert_eq!(
        orbit.element,
        WebActorResourceElement::Record {
            fields: vec![
                fe_codegen::WebActorResourceField {
                    name: "re_bits".to_owned(),
                    offset: 0,
                    scalar: fe_codegen::WebScalarKind::U32,
                },
                fe_codegen::WebActorResourceField {
                    name: "im_bits".to_owned(),
                    offset: 4,
                    scalar: fe_codegen::WebScalarKind::U32,
                },
            ],
            span: 8,
        }
    );

    assert_eq!(
        actor_web_entry(&db, top_mod).expect("legacy fragment projection"),
        Some(("paint".to_owned(), WebBundleMode::Render))
    );
}

#[test]
fn attributed_storage_intrinsics_compile_to_compute_and_fragment_wgsl() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_compute_storage");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "compute/storage fixture diagnostics:\n{diagnostics}"
    );

    let resource = |arg_index, access| SpirvExternalResource {
        arg_index,
        group: 0,
        binding: 0,
        name: "orbit".to_owned(),
        access,
        element: SpirvResourceElement::Record {
            fields: vec![
                SpirvResourceField {
                    name: "re_bits".to_owned(),
                    scalar: SpirvScalarKind::U32,
                    offset: 0,
                },
                SpirvResourceField {
                    name: "im_bits".to_owned(),
                    scalar: SpirvScalarKind::U32,
                    offset: 4,
                },
            ],
            span: 8,
        },
        stride: 8,
        length: 1,
    };

    let compute_package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "seed").unwrap();
    let compute = compile_runtime_package_spirv_compute_with_resources(
        &db,
        &compute_package,
        [1, 1, 1],
        &[resource(0, Access::ReadWrite)],
    )
    .expect("Fe-authored compute stage");
    let compute_wgsl = compute.wgsl.as_deref().expect("compute WGSL");
    assert!(compute_wgsl.contains("var<storage, read_write> orbit"));
    assert!(compute_wgsl.contains(".re_bits = 1065353216u"));
    assert!(compute_wgsl.contains(".im_bits = 3221225472u"));

    let fragment_package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "paint").unwrap();
    let fragment = compile_runtime_package_spirv_render_with_resources(
        &db,
        &fragment_package,
        &[resource(2, Access::Read)],
    )
    .expect("Fe-authored fragment stage");
    let fragment_wgsl = fragment.wgsl.as_deref().expect("fragment WGSL");
    assert!(fragment_wgsl.contains("var<storage> orbit"));
    assert!(fragment_wgsl.contains("].re_bits"));
    assert!(fragment_wgsl.contains("].im_bits"));
}

#[test]
fn mixed_scalar_storage_layout_reconciles_fco_manifest_and_wgsl() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_mixed_storage");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "mixed storage fixture diagnostics:\n{diagnostics}"
    );

    let layout_wasm = build_entry_wasm(&db, top_mod, "layout_receipt");
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, layout_wasm).expect("layout receipt module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("layout receipt instance");
    let layout = instance
        .get_typed_func::<(), (u32, u32, u32, u32)>(&mut store, "layout_receipt")
        .expect("layout receipt export")
        .call(&mut store, ())
        .expect("layout receipt execution");
    assert_eq!(layout, (12, 4, 12, 3));

    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("mixed-storage.fe".to_owned())),
    )
    .expect("mixed scalar storage bundle");
    let [resource] = bundle.manifest.resources.as_slice() else {
        panic!("mixed actor must derive exactly one resource")
    };
    assert_eq!((resource.stride, resource.span), (layout.2, layout.0));
    assert_eq!(
        resource.element,
        WebActorResourceElement::Record {
            fields: vec![
                fe_codegen::WebActorResourceField {
                    name: "x".to_owned(),
                    offset: 0,
                    scalar: fe_codegen::WebScalarKind::F32,
                },
                fe_codegen::WebActorResourceField {
                    name: "material".to_owned(),
                    offset: 4,
                    scalar: fe_codegen::WebScalarKind::U32,
                },
                fe_codegen::WebActorResourceField {
                    name: "y".to_owned(),
                    offset: 8,
                    scalar: fe_codegen::WebScalarKind::F32,
                },
            ],
            span: layout.0,
        }
    );
    let manifest_json = serde_json::to_value(&bundle.manifest).unwrap();
    let fields = manifest_json["resources"][0]["element"]["Record"]["fields"]
        .as_array()
        .expect("record fields JSON");
    assert_eq!(fields[0]["scalar"], "f32");
    assert!(
        fields[1].get("scalar").is_none(),
        "legacy u32 fields must retain their compact manifest spelling"
    );
    assert_eq!(fields[2]["scalar"], "f32");

    let compute_wgsl = &bundle.pass_wgsl[0].source;
    let fragment_wgsl = &bundle.pass_wgsl[1].source;
    assert!(compute_wgsl.contains("x: f32"));
    assert!(compute_wgsl.contains("material: u32"));
    assert!(compute_wgsl.contains("y: f32"));
    assert!(fragment_wgsl.contains("material: u32"));
}

#[test]
fn signed_storage_fails_closed_until_carrier_bitcasts_are_explicit() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_signed_storage");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "signed storage fixture diagnostics:\n{diagnostics}"
    );

    let error = match WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("signed-storage.fe".to_owned())),
    ) {
        Ok(_) => panic!("signed storage must not inherit the unsigned browser carrier silently"),
        Err(error) => error,
    };
    let message = error.to_string();
    assert!(
        message.contains("signed `i32` storage requires explicit carrier bitcasts"),
        "expected the signed-carrier boundary error, got: {message}"
    );
}

#[test]
fn typed_resource_policy_projects_to_manifest_and_narrows_physical_access() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_typed_resource_policy");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "typed resource policy diagnostics:\n{diagnostics}"
    );

    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("typed-resource-policy.fe".to_owned())),
    )
    .expect("typed resource policy bundle");
    let [resource] = bundle.manifest.resources.as_slice() else {
        panic!("typed policy actor must derive exactly one resource")
    };
    assert_eq!(
        resource.policy,
        WebResourcePolicy {
            kind: WebResourceKind::Storage,
            access: WebResourceAccess::ReadOnly,
            residency: WebResourceResidency::ActorResident,
            initialization: WebResourceInitialization::Zeroed,
            recovery: WebResourceRecovery::ReplayRecipe,
            visibility: WebResourceVisibility::Fragment,
        }
    );
    let policy_json = &serde_json::to_value(&bundle.manifest).unwrap()["resources"][0]["policy"];
    assert_eq!(policy_json["access"], "read_only");
    assert_eq!(policy_json["visibility"], "fragment");
    assert!(bundle.pass_wgsl[0].source.contains("var<storage> palette"));
    assert!(
        !bundle.pass_wgsl[0]
            .source
            .contains("var<storage, read_write> palette")
    );
}

#[test]
fn readonly_resource_store_is_rejected_by_missing_type_evidence() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_resource_readonly_store_rejected");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.contains("store")
            && (diagnostics.contains("trait bound is not satisfied")
                || diagnostics.contains("not found")
                || diagnostics.contains("doesn't implement")),
        "read-only storage must reject stores through missing GpuWritable evidence:\n{diagnostics}"
    );
}

#[test]
fn invalid_immutable_policy_fails_closed_before_shader_lowering() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_resource_invalid_immutable");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "invalid policy fixture should reach bundle validation:\n{diagnostics}"
    );

    let error = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("invalid-immutable.fe".to_owned())),
    )
    .expect_err("immutable writable storage must fail closed");
    assert!(
        error
            .to_string()
            .contains("immutable residency requires read-only access"),
        "unexpected invalid immutable policy error: {error}"
    );
}

#[test]
fn content_addressed_resource_is_verified_selected_and_materialized() {
    const BYTES: &[u8] = b"0123456789abcde\n";
    const SHA256: &str = "dc08b6f2c7aaeca6d88cd9c82797b328160ccb3b1a84243b8eadb296744426c4";

    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_content_addressed_resource");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "content asset diagnostics:\n{diagnostics}"
    );

    let missing = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("content-addressed.fe".to_owned())),
    )
    .expect_err("content-addressed resource without bytes must fail closed");
    assert!(
        missing.to_string().contains(SHA256),
        "missing asset error must name its exact identity: {missing}"
    );

    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("content-addressed.fe".to_owned()))
            .with_resource_asset(BYTES.to_vec()),
    )
    .expect("verified content-addressed bundle");
    assert_eq!(bundle.manifest.protocol_version, 7);
    let [resource] = bundle.manifest.resources.as_slice() else {
        panic!("content actor must derive exactly one resource")
    };
    assert_eq!(
        resource.policy.initialization,
        WebResourceInitialization::ContentAddressed {
            sha256: SHA256.to_owned(),
        }
    );
    let artifact = resource.artifact.as_ref().expect("resource artifact");
    assert_eq!(artifact.path, format!("resources/sha256-{SHA256}.bin"));
    assert_eq!(artifact.bytes, BYTES.len() as u64);
    assert_eq!(artifact.sha256, SHA256);

    let materialized = bundle
        .materialized_files()
        .expect("content materialization");
    let asset = materialized
        .iter()
        .find(|file| file.path() == artifact.path)
        .expect("materialized content-addressed artifact");
    assert_eq!(asset.bytes(), BYTES);
}

#[test]
fn content_addressed_resource_length_is_checked_against_layout() {
    const SHORT_BYTES: &[u8] = b"0123456789abcde";
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_content_addressed_wrong_length");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "short content diagnostics:\n{diagnostics}"
    );

    let error = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("short-content.fe".to_owned()))
            .with_resource_asset(SHORT_BYTES.to_vec()),
    )
    .expect_err("15 content bytes cannot initialize four u32 lanes");
    assert!(
        error.to_string().contains("expects 16 bytes") && error.to_string().contains("contains 15"),
        "unexpected content length error: {error}"
    );
}

#[test]
fn attributed_actor_builds_a_materialized_v7_pass_graph() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_compute_storage");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("known-color.fe".to_owned())),
    )
    .expect("v7 actor pass graph");

    assert_eq!(bundle.manifest.protocol_version, 7);
    assert!(bundle.wasm.is_empty(), "resource graph has no CPU fallback");
    assert_eq!(bundle.manifest.artifacts.wasm, None);
    assert_eq!(bundle.manifest.resources.len(), 1);
    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(
        bundle.manifest.passes[0].layout.mode,
        WebBundleMode::Compute
    );
    assert_eq!(bundle.manifest.passes[0].dispatch, Some([1, 1, 1]));
    assert_eq!(bundle.manifest.passes[1].layout.mode, WebBundleMode::Render);
    let paths = bundle
        .materialized_files()
        .expect("materialized graph")
        .into_iter()
        .map(|file| file.path().to_owned())
        .collect::<Vec<_>>();
    assert!(paths.contains(&"passes/000-compute.wgsl".to_owned()));
    assert!(paths.contains(&"passes/001-fragment.wgsl".to_owned()));
    assert!(!paths.contains(&"module.wasm".to_owned()));
}

#[test]
fn compute_only_actor_derives_and_materializes_without_a_fake_fragment() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_compute_only");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected compute-only actor diagnostics:\n{diagnostics}"
    );

    let derived = resolve_web_entry(&db, top_mod, None, None).expect("compute derivation");
    assert_eq!(
        derived,
        ("write_receipt".to_owned(), WebBundleMode::Compute)
    );
    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::compute(derived.0, Some("compute-only.fe".to_owned())),
    )
    .expect("compute-only actor graph");

    assert!(bundle.wasm.is_empty());
    assert_eq!(bundle.manifest.surface, None);
    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(bundle.manifest.passes[0].source_entry, "seed");
    assert_eq!(bundle.manifest.passes[1].source_entry, "write_receipt");
    for pass in &bundle.manifest.passes {
        assert_eq!(pass.dispatch, Some([1, 1, 1]));
        assert_eq!(pass.layout.mode, WebBundleMode::Compute);
    }
    assert_eq!(
        bundle.manifest.artifacts.wgsl,
        bundle.manifest.passes[1].shader
    );
    assert!(bundle.wgsl.contains("@compute"));
    assert!(!bundle.wgsl.contains("@fragment"));
}

#[test]
fn serial_and_parallel_actor_stage_demands_materialize_identically() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_compute_storage");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let serial = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("stage-demand-parity.fe".to_owned()))
            .with_stage_compile_jobs(1),
    )
    .expect("serial actor stage demands");
    let parallel = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("stage-demand-parity.fe".to_owned()))
            .with_stage_compile_jobs(2),
    )
    .expect("parallel actor stage demands");

    assert_eq!(
        serial.materialized_files().expect("serial files"),
        parallel.materialized_files().expect("parallel files"),
        "Salsa demand completion order must not alter any bundle byte",
    );
}

#[test]
fn nominal_readback_derives_one_binary_actor_boundary_without_manifest_semantics() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_typed_readback");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "typed readback fixture diagnostics:\n{diagnostics}"
    );

    let program = actor_gpu_program(&db, top_mod)
        .expect("typed readback GPU plan")
        .expect("typed readback actor");
    assert_eq!(program.resources.len(), 1);
    assert_eq!(program.resources[0].kind, GpuResource::Readback);
    assert_eq!(program.resources[0].name, "output");

    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("typed-readback.fe".to_owned())),
    )
    .expect("typed readback bundle");
    assert!(!bundle.wasm.is_empty());
    assert_eq!(bundle.manifest.resources.len(), 1);
    let manifest = serde_json::to_value(&bundle.manifest).unwrap();
    let resource = manifest["resources"][0].as_object().unwrap();
    assert!(
        !resource.contains_key("kind") && !resource.contains_key("readback"),
        "readback meaning belongs to the binary Fe ABI, not JSON: {resource:?}"
    );

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &bundle.wasm).expect("readback Wasm module");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance =
        wasmtime::Instance::new(&mut store, &module, &[]).expect("readback Wasm instance");
    let binding = instance
        .get_typed_func::<(), i32>(&mut store, "fe_gpu_readback_binding_v1")
        .expect("fixed readback binding export");
    assert_eq!(binding.call(&mut store, ()).unwrap(), 0);
    let replace = instance
        .get_typed_func::<i32, ()>(&mut store, "fe_surface_state_replace_v1")
        .expect("fixed resident state replacement export");
    let allocate = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "fe_cabi_alloc")
        .expect("fixed canonical allocator export");
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("fixed canonical memory export");
    let accept = instance
        .get_typed_func::<(i32, i32, i64), i32>(&mut store, "fe_gpu_readback_transition_v1")
        .expect("fixed typed readback transition");
    assert!(
        accept.call(&mut store, (0, 16, 0)).is_err(),
        "readback must fail closed until complete actor state is seeded"
    );
    let pointer = allocate.call(&mut store, (16, 4)).unwrap();
    memory
        .write(
            &mut store,
            pointer as usize,
            &[17, 0, 0, 0, 19, 0, 0, 0, 23, 0, 0, 0, 29, 0, 0, 0],
        )
        .unwrap();
    replace.call(&mut store, 7).unwrap();
    assert_eq!(accept.call(&mut store, (pointer, 16, 0)).unwrap(), 8);

    replace.call(&mut store, 7).unwrap();
    memory
        .write(&mut store, pointer as usize + 8, &[24, 0, 0, 0])
        .unwrap();
    assert_eq!(
        accept.call(&mut store, (pointer, 16, 0)).unwrap(),
        0,
        "one changed GPU word must be observed by authored Fe"
    );

    replace.call(&mut store, 7).unwrap();
    assert_eq!(
        accept.call(&mut store, (pointer, 12, 0)).unwrap(),
        0,
        "a truncated GPU message must fail closed in authored Fe"
    );
}

#[test]
fn nominal_readback_rejects_a_different_message_identity() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_typed_readback_mismatch");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "typed readback mismatch fixture diagnostics:\n{diagnostics}"
    );

    let error = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("typed-readback-mismatch.fe".to_owned())),
    )
    .expect_err("different nominal message types must fail closed");
    let message = format!("{error}");
    assert!(
        message.contains("different nominal message types"),
        "unexpected typed readback mismatch diagnostic: {message}"
    );
}

#[test]
fn nominal_compute_invocation_maps_to_physical_builtins_without_parameter_storage() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_compute_invocation");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "compute invocation fixture diagnostics:\n{diagnostics}"
    );

    let program = actor_gpu_program(&db, top_mod)
        .expect("compute invocation derivation")
        .expect("GPU actor");
    assert_eq!(
        program.stages[0].kind,
        WebActorStageKind::Compute {
            workgroup_size: [2, 2, 1],
            dispatch: [2, 2, 1],
            repeat: 1,
            taper: None,
            cooperation: None,
            cycle: None,
            invocation_context: true,
        }
    );

    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("compute-invocation.fe".to_owned())),
    )
    .expect("typed compute invocation pass graph");
    let pass = &bundle.manifest.passes[0];
    assert_eq!(pass.dispatch, Some([2, 2, 1]));
    assert_eq!(pass.layout.workgroup_size, [2, 2, 1]);
    assert!(
        pass.layout
            .bindings
            .iter()
            .all(|binding| binding.role != fe_codegen::WebBindingRole::Input),
        "compute invocation must not synthesize host-populated scalar input storage"
    );
    let trap = pass
        .layout
        .bindings
        .iter()
        .find(|binding| binding.name == "trap")
        .expect("checked resource indexing requires a trap channel");
    assert_eq!(trap.role, fe_codegen::WebBindingRole::Output);
    assert_eq!(trap.stride, 4);
    assert_eq!(trap.span, 16 * 4);
    assert_eq!(
        pass.layout
            .builtin_inputs
            .iter()
            .map(|input| input.source)
            .collect::<Vec<_>>(),
        vec![
            WebBuiltinSource::GlobalInvocationIdX,
            WebBuiltinSource::GlobalInvocationIdY,
            WebBuiltinSource::GlobalInvocationIdZ,
            WebBuiltinSource::LocalInvocationIdX,
            WebBuiltinSource::LocalInvocationIdY,
            WebBuiltinSource::LocalInvocationIdZ,
            WebBuiltinSource::WorkgroupIdX,
            WebBuiltinSource::WorkgroupIdY,
            WebBuiltinSource::WorkgroupIdZ,
            WebBuiltinSource::NumWorkgroupsX,
            WebBuiltinSource::NumWorkgroupsY,
            WebBuiltinSource::NumWorkgroupsZ,
            WebBuiltinSource::LocalInvocationIndex,
        ]
    );
    let wgsl = &bundle.pass_wgsl[0].source;
    for builtin in [
        "@builtin(global_invocation_id)",
        "@builtin(local_invocation_id)",
        "@builtin(workgroup_id)",
        "@builtin(num_workgroups)",
        "@builtin(local_invocation_index)",
    ] {
        assert!(wgsl.contains(builtin), "missing {builtin}:\n{wgsl}");
    }
}

#[test]
fn repeated_dispatch_is_derived_from_its_nominal_fe_policy() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_repeated_dispatch");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "repeated dispatch fixture diagnostics:\n{diagnostics}"
    );

    let program = actor_gpu_program(&db, top_mod)
        .expect("repeated dispatch derivation")
        .expect("GPU actor");
    assert_eq!(
        program.stages[0].kind,
        WebActorStageKind::Compute {
            workgroup_size: [1, 1, 1],
            dispatch: [1, 1, 1],
            repeat: 4,
            taper: None,
            cooperation: None,
            cycle: None,
            invocation_context: false,
        }
    );

    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("repeated-dispatch.fe".to_owned())),
    )
    .expect("repeated dispatch pass graph");
    let pass = &bundle.manifest.passes[0];
    assert_eq!(pass.source_entry, "advance");
    assert_eq!(pass.dispatch, Some([1, 1, 1]));
    assert_eq!(pass.repeat, 4);

    let encoded = serde_json::to_value(&bundle.manifest).expect("manifest JSON");
    assert_eq!(encoded["passes"][0]["repeat"], 4);
    assert!(
        encoded["passes"][1].get("repeat").is_none(),
        "ordinary single-execution passes keep the additive field absent"
    );
}

#[test]
fn cycled_dispatch_derives_one_ordered_actor_body_from_nominal_fe_types() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_cycled_dispatch");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "cycled dispatch fixture diagnostics:\n{diagnostics}"
    );

    let program = actor_gpu_program(&db, top_mod)
        .expect("cycled dispatch derivation")
        .expect("GPU actor");
    let cycle = WebActorPassCycle {
        group: "ProtocolRound".to_owned(),
        repeat: 3,
    };
    assert_eq!(
        program.stages[0].kind,
        WebActorStageKind::Compute {
            workgroup_size: [1, 1, 1],
            dispatch: [1, 1, 1],
            repeat: 1,
            taper: None,
            cooperation: None,
            cycle: Some(cycle.clone()),
            invocation_context: false,
        }
    );
    assert_eq!(
        program.stages[1].kind,
        WebActorStageKind::Compute {
            workgroup_size: [1, 1, 1],
            dispatch: [1, 1, 1],
            repeat: 3,
            taper: Some(fe_codegen::WebDispatchTaper {
                shifts: [0, 0, 0],
                repeat_decrement: 1,
            }),
            cooperation: Some(fe_codegen::WebDispatchCooperation { repeat_batch: 2 }),
            cycle: Some(cycle),
            invocation_context: false,
        }
    );

    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("cycled-dispatch.fe".to_owned())),
    )
    .expect("cycled dispatch pass graph");
    let passes = &bundle.manifest.passes;
    assert_eq!(passes.len(), 3);
    assert_eq!(passes[0].repeat, 1);
    assert_eq!(passes[1].repeat, 3);
    assert_eq!(
        passes[1].cooperation,
        Some(fe_codegen::WebDispatchCooperation { repeat_batch: 2 })
    );
    assert_eq!(
        passes[1].taper,
        Some(fe_codegen::WebDispatchTaper {
            shifts: [0, 0, 0],
            repeat_decrement: 1,
        })
    );
    let first_cycle = passes[0].cycle.expect("first cycle member");
    let second_cycle = passes[1].cycle.expect("second cycle member");
    assert_eq!(first_cycle.group, 0);
    assert_eq!(first_cycle.repeat, 3);
    assert_eq!(second_cycle, first_cycle);
    assert_eq!(passes[2].cycle, None);

    let encoded = serde_json::to_value(&bundle.manifest).expect("manifest JSON");
    assert_eq!(encoded["passes"][0]["cycle"]["group"], 0);
    assert_eq!(encoded["passes"][0]["cycle"]["repeat"], 3);
    assert_eq!(encoded["passes"][1]["cycle"], encoded["passes"][0]["cycle"]);
    assert_eq!(encoded["passes"][1]["taper"]["repeat_decrement"], 1);
    assert_eq!(encoded["passes"][1]["cooperation"]["repeat_batch"], 2);
    assert!(encoded["passes"][2].get("cycle").is_none());
}

#[test]
fn compute_invocation_context_must_be_the_first_behavior_argument() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_compute_invocation_misplaced");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "misplaced compute invocation fixture diagnostics:\n{diagnostics}"
    );
    let error = actor_gpu_program(&db, top_mod).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must be the compute behavior's first argument"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn desugar_reproduces_the_handwritten_kernel_byte_for_byte() {
    // The `actor`-desugared `paint` and the hand-written free `paint` are built
    // as the same-named wasm entry from two sibling ingots and must be byte
    // identical: the flattened parameters, the `self.<field>` rewrite, and the
    // dropped placement row together reproduce exactly what a hand-written free
    // kernel emits.
    let mut db = DriverDataBase::default();
    let actor_url = ingot_root("tests/fixtures/actor_repro_actor");
    let free_url = ingot_root("tests/fixtures/actor_repro_free");
    assert!(
        !driver::init_ingot(&mut db, &actor_url),
        "actor fixture diagnostics"
    );
    assert!(
        !driver::init_ingot(&mut db, &free_url),
        "free fixture diagnostics"
    );

    let actor_mod = ingot_top_mod(&db, &actor_url);
    let free_mod = ingot_top_mod(&db, &free_url);
    let actor_diags = db.run_on_top_mod(actor_mod).format_diags(&db);
    assert!(
        actor_diags.is_empty(),
        "actor fixture diagnostics:\n{actor_diags}"
    );
    let free_diags = db.run_on_top_mod(free_mod).format_diags(&db);
    assert!(
        free_diags.is_empty(),
        "free fixture diagnostics:\n{free_diags}"
    );

    let actor_wasm = build_entry_wasm(&db, actor_mod, "paint");
    let free_wasm = build_entry_wasm(&db, free_mod, "paint");
    assert_eq!(
        actor_wasm, free_wasm,
        "actor-desugared `paint` must reproduce the hand-written free kernel byte for byte"
    );
}
