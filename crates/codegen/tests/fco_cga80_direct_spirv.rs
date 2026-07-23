use std::time::Instant;

use common::InputDb;
use driver::DriverDataBase;
use url::Url;

fn source() -> String {
    let sparse_clifford_api = fe_codegen::standalone_ctfe_ingot_source(include_str!(
        "../../../ingots/sparse_clifford/src/lib.fe"
    ));
    let base = include_str!("fixtures/fco_cga80_direct_lanes.fe");
    let (prefix, rest) = base
        .split_once("// BEGIN_PUBLIC_ORACLES")
        .expect("public-oracle begin marker");
    let (_, suffix) = rest
        .split_once("// END_PUBLIC_ORACLES")
        .expect("public-oracle end marker");
    format!(
        r#"{sparse_clifford_api}
{prefix}{suffix}
extern {{
    fn __i32_from_f32(_: f32) -> i32
    const fn __bitcast<From, To>(_: From) -> To
}}

pub fn cga_fco_direct_render(
    px: i32, py: i32,
    s1: f32, s2: f32, s8: f32, s16: f32,
    p1: f32, p2: f32, p4: f32, p8: f32, p16: f32,
) -> u32 {{
    let image = <ConformalVector as Sandwich>::sandwich(
        ConformalVector {{ e1: s1, e2: s2, e4: 0.0, e8: s8, e16: s16 }},
        ConformalVector {{ e1: p1, e2: p2, e4: p4, e8: p8, e16: p16 }},
    )
    let lanes = image.e1 + image.e2 + image.e4 + image.e8 + image.e16
    let dither = __i32_from_f32(lanes * 8.0) + px - py
    __bitcast(dither + 255 * 256 + 64 * 65536 + -16777216)
}}
"#
    )
}

#[test]
fn canonical_provider_lanes_reach_call_free_browser_wgsl() {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///fco_cga80_direct_render.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);

    let analysis_started = Instant::now();
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    let analysis_elapsed = analysis_started.elapsed();
    assert!(
        diagnostics.is_empty(),
        "unexpected direct-render diagnostics:\n{diagnostics}"
    );

    let backend_started = Instant::now();
    let package =
        mir::build_wasm_runtime_package(&db, top_mod).expect("direct CGA render runtime package");
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("direct CGA provider lanes should compile through Render SPIR-V");
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
        "only fullscreen vertex and direct fragment entry points should remain:\n{wgsl}"
    );
    assert!(
        !wgsl.contains("loop {"),
        "the specialized 80→32 plan must leave no runtime loop:\n{wgsl}"
    );
    assert!(
        !wgsl.contains("i64") && !wgsl.contains("u64") && !wgsl.contains("i256"),
        "browser WGSL must not contain wide integer types:\n{wgsl}"
    );
    assert!(
        wgsl.len() <= 12_000,
        "direct five-lane WGSL unexpectedly grew to {} bytes",
        wgsl.len()
    );
    eprintln!(
        "canonical FCO CGA browser path: analysis {:?}, MIR+SPIR-V {:?}, {} bytes / {} lines WGSL",
        analysis_elapsed,
        backend_elapsed,
        wgsl.len(),
        wgsl.lines().count()
    );
}
