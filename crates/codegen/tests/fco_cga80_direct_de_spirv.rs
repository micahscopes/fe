use std::time::Instant;

use common::InputDb;
use driver::DriverDataBase;
use url::Url;

fn source() -> String {
    let sparse_clifford_api = include_str!("../../../ingots/sparse_clifford/src/lib.fe");
    let base = include_str!("fixtures/fco_cga80_direct_lanes.fe");
    let (prefix, rest) = base
        .split_once("// BEGIN_PUBLIC_ORACLES")
        .expect("public-oracle begin marker");
    let (_, suffix) = rest
        .split_once("// END_PUBLIC_ORACLES")
        .expect("public-oracle end marker");
    format!(
        "{sparse_clifford_api}\n{prefix}{suffix}\n{}",
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
    let package =
        mir::build_wasm_runtime_package(&db, top_mod).expect("direct CGA DE runtime package");
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
