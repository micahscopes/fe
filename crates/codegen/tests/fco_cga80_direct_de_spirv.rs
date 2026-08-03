use std::time::Instant;

use common::InputDb;
use driver::DriverDataBase;
use url::Url;

fn source() -> String {
    let sparse_clifford_api = fe_codegen::standalone_ctfe_ingot_source(include_str!(
        "../../../ingots/sparse_clifford/src/lib.fe"
    ));
    let canonical50_api = fe_codegen::standalone_ctfe_ingot_source(include_str!(
        "../../../ingots/canonical_cl41_schedule/src/lib.fe"
    ));
    // Both ingot sources are inlined into ONE file, so canonical50's
    // cross-ingot references cannot resolve: there is no `sparse_clifford`
    // ingot in a single-file compile. Drop its import header and strip the
    // qualified prefix, exactly as fco_cga80_direct_lanes.rs and
    // fco_cga_sparse_facade.rs already do. Without this the composed source
    // reports `sparse_clifford is not found` at its `use` line and at every
    // `sparse_clifford::` path.
    let (_, canonical50_api) = canonical50_api
        .split_once("// Bounded symbolic coefficient interpretation")
        .expect("canonical standalone source begins after its ingot import");
    let canonical50_api = format!(
        "// Bounded symbolic coefficient interpretation{}",
        canonical50_api.replace("sparse_clifford::", "")
    );
    let base = include_str!("fixtures/composed/fco_cga80_direct_lanes.fe");
    let (_, provider_and_oracles) = base
        .split_once("// BEGIN_PROVIDER_EMITTER")
        .expect("provider begin marker");
    let provider_and_oracles = format!("// BEGIN_PROVIDER_EMITTER{provider_and_oracles}");
    let (provider, rest) = provider_and_oracles
        .split_once("// BEGIN_PUBLIC_ORACLES")
        .expect("public-oracle begin marker");
    let (_, suffix) = rest
        .split_once("// END_PUBLIC_ORACLES")
        .expect("public-oracle end marker");
    format!(
        "{sparse_clifford_api}\n{canonical50_api}\n{provider}{suffix}\n{}",
        include_str!("fixtures/spirv/fco_cga80_direct_de_body.fe")
    )
}

#[test]
fn canonical_provider_drives_full_conformal_inversion_de_wgsl() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///fco_cga80_direct_de_render.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);

    let analysis_started = Instant::now();
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    let analysis_elapsed = analysis_started.elapsed();
    assert!(
        diagnostics.is_empty(),
        "unexpected direct-DE diagnostics:\n{diagnostics}"
    );

    let backend_started = Instant::now();
    // Name the render entry. `28d74b524` added a SECOND `pub fn` to this shared
    // body fixture (`cga_schedule32_all_blades`, an evidence-only scalar view),
    // and `build_wasm_runtime_package` admits every `pub` entry-module function
    // as a runtime root. Two roots reach the render backend, which then refuses
    // with "coordinate args 0 and 1 must both be i32" against the wrong one.
    // fco_cga_sparse_facade.rs consumes the same fixture and already names its
    // entry this way; the assertions below (fullscreen vertex + fragment, one
    // DE loop) presuppose exactly one render entry.
    let package =
        mir::build_wasm_runtime_package_for_entry(&db, top_mod, "cga_schedule32_vec5_de_render")
            .expect("direct CGA DE runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("direct CGA DE should compile through Render SPIR-V");
    let backend_elapsed = backend_started.elapsed();
    assert_eq!(artifact.words.first().copied(), Some(0x0723_0203));
    let wgsl = artifact.wgsl.as_deref().expect("WGSL side artifact");
    let module = naga::front::wgsl::parse_str(wgsl).expect("WGSL reparses");
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .expect("WGSL validates with browser-default capabilities");

    assert_eq!(
        wgsl.matches("fn ").count(),
        2,
        "provider helpers must inline into fullscreen vertex + fragment only:\n{wgsl}"
    );
    assert_eq!(
        wgsl.matches("loop {").count(),
        1,
        "only the 72-step DE ray-march loop may survive; algebra scheduling is compile-time:\n{wgsl}"
    );
    assert!(
        !wgsl.contains("i64") && !wgsl.contains("u64") && !wgsl.contains("i256"),
        "browser WGSL must not contain wide integer types:\n{wgsl}"
    );
    assert!(
        wgsl.len() <= 12_000 && wgsl.lines().count() <= 300,
        "direct DE WGSL unexpectedly grew to {} bytes / {} lines",
        wgsl.len(),
        wgsl.lines().count()
    );
    eprintln!(
        "canonical FCO CGA DE: analysis {:?}, MIR+SPIR-V {:?}, {} bytes / {} lines WGSL",
        analysis_elapsed,
        backend_elapsed,
        wgsl.len(),
        wgsl.lines().count()
    );
}
