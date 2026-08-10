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
    WasmCompileOptions, WebActorResourceElement, WebActorStageKind, WebBuildOptions, WebBundle,
    WebBundleMode, actor_gpu_program, actor_web_entry,
    compile_runtime_package_spirv_compute_with_resources,
    compile_runtime_package_spirv_render_with_resources, compile_runtime_package_wasm_with_options,
    resolve_web_entry,
};
use hir::hir_def::{HirIngot, TopLevelMod};
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
                },
                fe_codegen::WebActorResourceField {
                    name: "im_bits".to_owned(),
                    offset: 4,
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
fn attributed_actor_builds_a_materialized_v6_pass_graph() {
    let mut db = DriverDataBase::default();
    let url = ingot_root("tests/fixtures/actor_compute_storage");
    assert!(!driver::init_ingot(&mut db, &url));
    let top_mod = ingot_top_mod(&db, &url);
    let bundle = WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render("paint", Some("known-color.fe".to_owned())),
    )
    .expect("v6 actor pass graph");

    assert_eq!(bundle.manifest.protocol_version, 6);
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
