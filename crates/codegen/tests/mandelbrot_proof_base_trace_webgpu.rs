//! Structural gate for production-width sparse AIR base-row placement.

use std::path::{Path, PathBuf};

use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WebBindingRole, WebBuildOptions, WebBundleMode, resolve_web_entry};
use hir::hir_def::HirIngot;
use url::Url;

const THREADS: u32 = 64;
const TRACE_ROWS: u32 = 4_096;
const BASE_FIELDS: u32 = 260;
const BASE_TRACE_WORDS: u32 = TRACE_ROWS * BASE_FIELDS;
const LDE_ROWS: u32 = TRACE_ROWS * 2;
const BASE_LDE_WORDS: u32 = LDE_ROWS * BASE_FIELDS;
const INPUT_GRID_LANES: u32 = BASE_FIELDS * TRACE_ROWS / 2;
const OUTPUT_GRID_LANES: u32 = BASE_FIELDS * LDE_ROWS / 2;

#[test]
fn production_sparse_product_denominators_have_a_focused_browser_webgpu_gate() {
    let dir = repo_root()
        .join("crates/codegen/tests/fixtures/mandelbrot_proof_interaction_bus_webgpu_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "focused interaction fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("focused interaction fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "focused interaction source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("mandelbrot_sparse_product_interaction".into())),
    )
    .expect("focused interaction fixture should compile into a WebBundle");

    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(bundle.manifest.resources.len(), 4);
    let write = &bundle.manifest.passes[0];
    assert_eq!(write.source_entry, "prepare_product_denominators");
    assert_eq!(write.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(write.dispatch, Some([TRACE_ROWS / THREADS, 1, 1]));
    assert_eq!(write.repeat, 1);
    assert_eq!(bundle.manifest.passes[1].source_entry, "paint");
    assert!(write.layout.bindings.len() <= 5);

    let shader = &bundle.pass_wgsl[0].source;
    if let Some(path) = std::env::var_os("FE_TEST_WGSL_DUMP") {
        std::fs::write(&path, shader)
            .unwrap_or_else(|error| panic!("could not write WGSL dump to {path:?}: {error}"));
    }
    let module = naga::front::wgsl::parse_str(shader)
        .unwrap_or_else(|error| panic!("product-denominator WGSL parse failed: {error:?}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("product-denominator WGSL validation failed: {error:?}"));
    assert!(
        shader.len() < 1_000_000,
        "focused product-denominator shader is unexpectedly large: {} bytes",
        shader.len(),
    );
}

#[test]
fn production_sparse_round_interaction_fits_the_private_heap() {
    let dir = repo_root()
        .join("crates/codegen/tests/fixtures/mandelbrot_proof_round_interaction_webgpu_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "focused round interaction fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("focused round interaction fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "focused round interaction source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("mandelbrot_sparse_round_interaction".into())),
    )
    .expect("the streaming round interaction should fit the private heap");

    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(bundle.manifest.resources.len(), 4);
    let write = &bundle.manifest.passes[0];
    assert_eq!(write.source_entry, "write_round_locals");
    assert_eq!(write.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(write.dispatch, Some([TRACE_ROWS / THREADS, 1, 1]));
    assert_eq!(write.repeat, 1);
    assert_eq!(bundle.manifest.passes[1].source_entry, "paint");

    let shader = &bundle.pass_wgsl[0].source;
    eprintln!("streaming round-interaction WGSL: {} bytes", shader.len());
    let module = naga::front::wgsl::parse_str(shader)
        .unwrap_or_else(|error| panic!("round-interaction WGSL parse failed: {error:?}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("round-interaction WGSL validation failed: {error:?}"));
    assert!(
        shader.len() < 1_000_000,
        "streaming round-interaction shader is unexpectedly large: {} bytes",
        shader.len(),
    );
}

#[test]
fn production_sparse_linear_interaction_fits_the_private_heap() {
    let dir = repo_root()
        .join("crates/codegen/tests/fixtures/mandelbrot_proof_linear_interaction_webgpu_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "focused linear interaction fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("focused linear interaction fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "focused linear interaction source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("mandelbrot_sparse_linear_interaction".into())),
    )
    .expect("the streaming linear interaction should fit the private heap");

    assert_eq!(bundle.manifest.passes.len(), 2);
    assert_eq!(bundle.manifest.resources.len(), 4);
    let write = &bundle.manifest.passes[0];
    assert_eq!(write.source_entry, "write_linear_locals");
    assert_eq!(write.layout.workgroup_size, [THREADS, 1, 1]);
    assert_eq!(write.dispatch, Some([TRACE_ROWS / THREADS, 1, 1]));
    assert_eq!(write.repeat, 1);
    assert_eq!(bundle.manifest.passes[1].source_entry, "paint");

    let shader = &bundle.pass_wgsl[0].source;
    if let Some(path) = std::env::var_os("FE_TEST_WGSL_DUMP") {
        std::fs::write(&path, shader)
            .unwrap_or_else(|error| panic!("could not write WGSL dump to {path:?}: {error}"));
    }
    eprintln!("streaming linear-interaction WGSL: {} bytes", shader.len());
    let module = naga::front::wgsl::parse_str(shader)
        .unwrap_or_else(|error| panic!("linear-interaction WGSL parse failed: {error:?}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("linear-interaction WGSL validation failed: {error:?}"));
    assert!(
        shader.len() < 1_000_000,
        "streaming linear-interaction shader is unexpectedly large: {} bytes",
        shader.len(),
    );
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/codegen should have a repository root")
        .to_path_buf()
}

#[test]
fn production_sparse_base_trace_lowers_to_browser_webgpu() {
    let dir =
        repo_root().join("crates/codegen/tests/fixtures/mandelbrot_proof_base_trace_webgpu_ingot");
    let mut db = DriverDataBase::default();
    let url = Url::from_directory_path(&dir)
        .unwrap_or_else(|_| panic!("invalid ingot path {}", dir.display()));
    assert!(
        !driver::init_ingot(&mut db, &url),
        "sparse base trace fixture initialization diagnostics",
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("sparse base trace fixture should resolve to one ingot");
    let top_mod = ingot.root_mod(&db);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "sparse base trace source diagnostics:\n{diagnostics}",
    );
    let (entry, mode) = resolve_web_entry(&db, top_mod, None, None)
        .expect("the actor should derive its typed WebGPU entry");
    assert_eq!(mode, WebBundleMode::Render);
    let bundle = fe_codegen::WebBundle::compile(
        &db,
        top_mod,
        WebBuildOptions::render(entry, Some("mandelbrot_sparse_base_trace".into())),
    )
    .expect("sparse base trace fixture should compile into a WebBundle");

    assert_eq!(bundle.manifest.passes.len(), 41);
    assert_eq!(bundle.manifest.resources.len(), 7);
    let producer_names = [
        "derive_products",
        "derive_primary_linears",
        "derive_final_linears",
    ];
    let producer_widths = [3, 2, 2];
    for ((pass, name), width) in bundle.manifest.passes[..3]
        .iter()
        .zip(producer_names)
        .zip(producer_widths)
    {
        assert_eq!(pass.source_entry, name);
        assert_eq!(pass.layout.workgroup_size, [width, 1, 1]);
        assert_eq!(pass.dispatch, Some([1, 1, 1]));
        assert_eq!(pass.repeat, 1);
    }
    let transition_finish = &bundle.manifest.passes[3];
    assert_eq!(transition_finish.source_entry, "finish_transition");
    assert_eq!(transition_finish.layout.workgroup_size, [1, 1, 1]);
    assert_eq!(transition_finish.dispatch, Some([1, 1, 1]));

    let control_names = ["write_control", "write_control_plan", "write_control_link"];
    for (write, name) in bundle.manifest.passes[4..7].iter().zip(control_names) {
        assert_eq!(write.source_entry, name);
        assert_eq!(write.layout.workgroup_size, [THREADS, 1, 1]);
        assert_eq!(write.dispatch, Some([TRACE_ROWS / THREADS, 1, 1]));
        assert_eq!(write.repeat, 1);
    }

    let witness_names = [
        "write_radix_witness",
        "write_carry_witness",
        "write_product_witness",
        "write_round_witness",
        "write_linear_witness",
        "write_boundary_witness",
        "write_padding_witness",
    ];
    let witness_groups = [28, 14, 2, 1, 2, 1, 20];
    for ((write, name), groups) in bundle.manifest.passes[7..14]
        .iter()
        .zip(witness_names)
        .zip(witness_groups)
    {
        assert_eq!(write.source_entry, name);
        assert_eq!(write.layout.workgroup_size, [THREADS, 1, 1]);
        assert_eq!(write.dispatch, Some([groups, 1, 1]));
        assert_eq!(write.repeat, 1);
    }

    let plan_names = [
        "write_arithmetic_plan",
        "write_arithmetic_link",
        "write_round_plan",
        "write_linear_plan",
        "write_boundary_plan",
    ];
    for (write, name) in bundle.manifest.passes[14..19].iter().zip(plan_names) {
        assert_eq!(write.source_entry, name);
        assert_eq!(write.layout.workgroup_size, [THREADS, 1, 1]);
        assert_eq!(write.dispatch, Some([TRACE_ROWS / THREADS, 1, 1]));
        assert_eq!(write.repeat, 1);
    }
    let finish = &bundle.manifest.passes[19];
    assert_eq!(finish.source_entry, "finish_base_trace");
    assert_eq!(finish.layout.workgroup_size, [1, 1, 1]);
    assert_eq!(finish.dispatch, Some([1, 1, 1]));

    let lde_names = [
        "prepare_lde_inverse",
        "advance_lde_inverse",
        "validate_lde_inverse",
        "prepare_lde_forward",
        "advance_lde_forward",
        "validate_lde_forward",
        "finish_lde",
    ];
    assert_eq!(
        bundle.manifest.passes[20..27]
            .iter()
            .map(|pass| pass.source_entry.as_str())
            .collect::<Vec<_>>(),
        lde_names,
    );
    assert_eq!(
        bundle.manifest.passes[20].layout.workgroup_size,
        [THREADS, 1, 1]
    );
    assert_eq!(bundle.manifest.passes[20].dispatch, Some([8_320, 1, 1]));
    assert_eq!(bundle.manifest.passes[20].repeat, 1);
    assert_eq!(bundle.manifest.passes[21].dispatch, Some([8_320, 1, 1]));
    assert_eq!(bundle.manifest.passes[21].repeat, 12);
    assert_eq!(bundle.manifest.passes[22].dispatch, Some([5, 1, 1]));
    assert_eq!(bundle.manifest.passes[22].repeat, 1);
    assert_eq!(bundle.manifest.passes[23].dispatch, Some([16_640, 1, 1]));
    assert_eq!(bundle.manifest.passes[24].dispatch, Some([16_640, 1, 1]));
    assert_eq!(bundle.manifest.passes[24].repeat, 13);
    assert_eq!(bundle.manifest.passes[25].dispatch, Some([5, 1, 1]));
    assert_eq!(bundle.manifest.passes[26].layout.workgroup_size, [1, 1, 1]);
    assert_eq!(bundle.manifest.passes[26].dispatch, Some([1, 1, 1]));
    let commitment_names = [
        "prepare_base_lde_commitments",
        "advance_base_lde_commitments",
        "prepare_base_lde_tree",
        "advance_base_lde_tree",
        "finish_base_lde_tree",
    ];
    assert_eq!(
        bundle.manifest.passes[27..32]
            .iter()
            .map(|pass| pass.source_entry.as_str())
            .collect::<Vec<_>>(),
        commitment_names,
    );
    assert_eq!(bundle.manifest.passes[27].layout.workgroup_size, [64, 1, 1]);
    assert_eq!(bundle.manifest.passes[27].dispatch, Some([128, 1, 1]));
    assert_eq!(bundle.manifest.passes[27].repeat, 1);
    assert_eq!(bundle.manifest.passes[28].layout.workgroup_size, [64, 1, 1]);
    assert_eq!(bundle.manifest.passes[28].dispatch, Some([128, 1, 1]));
    assert_eq!(bundle.manifest.passes[28].repeat, 34);
    assert_eq!(bundle.manifest.passes[29].layout.workgroup_size, [64, 1, 1]);
    assert_eq!(bundle.manifest.passes[29].dispatch, Some([64, 1, 1]));
    assert_eq!(bundle.manifest.passes[30].layout.workgroup_size, [64, 1, 1]);
    assert_eq!(bundle.manifest.passes[30].dispatch, Some([64, 1, 1]));
    assert_eq!(bundle.manifest.passes[30].repeat, 13);
    assert_eq!(bundle.manifest.passes[31].layout.workgroup_size, [1, 1, 1]);
    assert_eq!(bundle.manifest.passes[31].dispatch, Some([1, 1, 1]));
    let challenge_names = [
        "prepare_base_lde_challenges",
        "advance_base_lde_challenges",
        "finish_base_lde_challenges",
    ];
    assert_eq!(
        bundle.manifest.passes[32..35]
            .iter()
            .map(|pass| pass.source_entry.as_str())
            .collect::<Vec<_>>(),
        challenge_names,
    );
    assert_eq!(bundle.manifest.passes[32].layout.workgroup_size, [64, 1, 1]);
    assert_eq!(bundle.manifest.passes[32].dispatch, Some([2, 1, 1]));
    assert_eq!(bundle.manifest.passes[32].repeat, 1);
    assert_eq!(bundle.manifest.passes[33].layout.workgroup_size, [64, 1, 1]);
    assert_eq!(bundle.manifest.passes[33].dispatch, Some([2, 1, 1]));
    assert_eq!(bundle.manifest.passes[33].repeat, 89);
    assert_eq!(bundle.manifest.passes[34].layout.workgroup_size, [1, 1, 1]);
    assert_eq!(bundle.manifest.passes[34].dispatch, Some([1, 1, 1]));
    let interaction_names = [
        "write_interaction_product_locals",
        "write_interaction_round_locals",
        "write_interaction_linear_locals",
        "write_interaction_boundary_locals",
    ];
    for (write, name) in bundle.manifest.passes[35..39].iter().zip(interaction_names) {
        assert_eq!(write.source_entry, name);
        assert_eq!(write.layout.workgroup_size, [THREADS, 1, 1]);
        assert_eq!(write.dispatch, Some([TRACE_ROWS / THREADS, 1, 1]));
        assert_eq!(write.repeat, 1);
    }
    assert_eq!(
        bundle.manifest.passes[39].source_entry,
        "finish_interaction_locals",
    );
    assert_eq!(bundle.manifest.passes[39].layout.workgroup_size, [1, 1, 1]);
    assert_eq!(bundle.manifest.passes[39].dispatch, Some([1, 1, 1]));
    assert_eq!(bundle.manifest.passes[40].source_entry, "paint");

    let resource_length = |name: &str| {
        bundle
            .manifest
            .resources
            .iter()
            .find(|resource| resource.name == name)
            .map(|resource| resource.length)
    };
    assert_eq!(resource_length("transition_workspace"), Some(218));
    assert_eq!(resource_length("base_trace"), Some(BASE_TRACE_WORDS));
    assert_eq!(resource_length("validity"), Some(TRACE_ROWS));
    assert_eq!(
        resource_length("lde_inverse_values"),
        Some(BASE_TRACE_WORDS)
    );
    assert_eq!(
        resource_length("lde_inverse_progress"),
        Some(INPUT_GRID_LANES)
    );
    assert_eq!(resource_length("lde_values"), Some(BASE_LDE_WORDS));
    assert_eq!(resource_length("lde_progress"), Some(OUTPUT_GRID_LANES));
    assert!(
        bundle.manifest.passes.iter().all(|pass| {
            pass.layout
                .bindings
                .iter()
                .filter(|binding| binding.role == WebBindingRole::Resource)
                .count()
                <= 7
        }),
        "every sparse AIR pass must leave one portable storage binding for compiler state",
    );
    assert!(
        bundle
            .manifest
            .passes
            .iter()
            .all(|pass| pass.layout.bindings.len() <= 8),
        "actor resources and compiler state must fit the browser storage-buffer limit",
    );
    for (pass, shader) in bundle.manifest.passes.iter().zip(&bundle.pass_wgsl) {
        let module = naga::front::wgsl::parse_str(&shader.source)
            .unwrap_or_else(|error| panic!("{} WGSL parse failed: {error:?}", pass.source_entry));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::default(),
        )
        .validate(&module)
        .unwrap_or_else(|error| {
            panic!(
                "{} WGSL browser validation failed: {error:?}",
                pass.source_entry
            )
        });
    }
}
