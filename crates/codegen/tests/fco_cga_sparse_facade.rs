use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WasmCompileOptions, compile_runtime_package_spirv_render,
    compile_runtime_package_wasm_with_options,
};
use url::Url;

const CANONICAL: &str = include_str!("fixtures/composed/fco_cga80_direct_lanes.fe");
const SPARSE_CLIFFORD_API: &str = include_str!("../../../ingots/sparse_clifford/src/lib.fe");
const CANONICAL50_API: &str = include_str!("../../../ingots/canonical_cl41_schedule/src/lib.fe");
const BODY: &str = include_str!("fixtures/spirv/fco_cga80_direct_de_body.fe");
const ENTRY: &str = "cga_schedule32_vec5_de_render";

fn composed_source() -> String {
    let (_, provider_and_oracles) = CANONICAL
        .split_once("// BEGIN_PROVIDER_EMITTER")
        .expect("canonical provider begin marker");
    let provider_and_oracles = format!("// BEGIN_PROVIDER_EMITTER{provider_and_oracles}");
    let (provider, rest) = provider_and_oracles
        .split_once("// BEGIN_PUBLIC_ORACLES")
        .expect("canonical public-oracle begin marker");
    let (_, suffix) = rest
        .split_once("// END_PUBLIC_ORACLES")
        .expect("canonical public-oracle end marker");
    let sparse_api = fe_codegen::standalone_ctfe_ingot_source(SPARSE_CLIFFORD_API);
    let canonical50_api = fe_codegen::standalone_ctfe_ingot_source(CANONICAL50_API);
    // Both ingot sources are inlined into ONE file, so canonical50's cross-ingot
    // references cannot resolve: there is no `sparse_clifford` ingot in a
    // single-file compile. Drop its import header and strip the qualified
    // prefix, exactly as fco_cga80_direct_lanes.rs:37-43 already does. Without
    // this the composed source reports `sparse_clifford is not found` at its
    // `use` line and at every `sparse_clifford::` path.
    let (_, canonical50_api) = canonical50_api
        .split_once("// Bounded symbolic coefficient interpretation")
        .expect("canonical standalone source begins after its ingot import");
    let canonical50_api = format!(
        "// Bounded symbolic coefficient interpretation{}",
        canonical50_api.replace("sparse_clifford::", "")
    );
    format!("{sparse_api}\n{canonical50_api}\n{provider}{suffix}\n{BODY}")
}

#[test]
fn semantic_sparse_facade_erases_to_the_direct_schedule32_kernel_shape() {
    assert!(CANONICAL.contains("type ConformalPoint = ConformalVector"));
    assert!(CANONICAL.contains("type ConformalSphere = ConformalVector"));
    assert!(CANONICAL.contains("struct ConformalVector {"));
    assert!(BODY.contains("let point: ConformalPoint = ConformalPoint {"));
    assert!(BODY.contains("let sphere: ConformalSphere = ConformalSphere {"));
    // This used to assert exactly ONE call site. That was a proxy for "one
    // specialized aggregate method", and the proxy broke when 28d74b524 added a
    // second caller to expose reflected Schedule32 coefficients. Two callers of
    // one aggregate still satisfy the property; a call count never measured it.
    //
    // Assert the property directly instead. BODY must FEED an aggregate, not
    // define one: it contains no `impl`/`trait`/`fn` for Sandwich at all, and
    // every use goes through the single `ConformalVector` aggregate.
    let sandwich_calls = BODY.matches("as Sandwich>::sandwich").count();
    assert!(
        sandwich_calls >= 1,
        "the compact semantic records must feed the specialized aggregate method",
    );
    assert_eq!(
        BODY.matches("<ConformalVector as Sandwich>::sandwich").count(),
        sandwich_calls,
        "every aggregate call must go through the one ConformalVector aggregate",
    );
    for defines in ["impl Sandwich", "trait Sandwich", "fn sandwich"] {
        assert!(
            !BODY.contains(defines),
            "the body must delegate to the aggregate, not define its own `{defines}`",
        );
    }
    assert!(
        !BODY.contains("#[inline(always)]"),
        "the facade's zero-cost claim must not depend on backend-specific inlining",
    );

    let source = composed_source();
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///fco_cga_sparse_facade.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_owned()));
    let file = db.workspace().get(&db, &url).expect("facade source");
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected sparse-facade diagnostics:\n{diagnostics}"
    );

    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, ENTRY)
        .expect("sparse facade should build the flagship runtime package");
    let artifact = compile_runtime_package_spirv_render(&db, &package)
        .expect("sparse facade should compile through the browser render backend");
    let wgsl = artifact.wgsl.expect("browser WGSL");
    naga::front::wgsl::parse_str(&wgsl).expect("facade WGSL must reparse");

    // What this check is FOR, per its original comment: proving the semantic
    // records and generic wrapper add no runtime calls and no hidden work.
    // Assert that invariant directly, because it is the part that must not
    // drift: two functions, no more.
    assert_eq!(
        wgsl.matches("fn ").count(),
        2,
        "the facade must not introduce a runtime call:\n{wgsl}",
    );

    // The arithmetic counts are a drift tripwire, not an invariant, so they are
    // re-baselined whenever the SCHEDULE legitimately changes. They did change:
    // the tuple below was (79, 82, 13, 4, 3, 2) as of f3210f41b (2026-07-23),
    // measured against the direct nine-scalar body, and 9c15dc1c2 (2026-07-24)
    // then replaced that body with the typed canonical50 schedule. `+` roughly
    // doubling is what summing 50 canonical monomials looks like; the call
    // count, division count and sqrt count are unchanged, which is why the
    // invariant above still holds and this is a re-baseline rather than a
    // regression.
    let arithmetic_shape = (
        wgsl.matches(" * ").count(),
        wgsl.matches(" + ").count(),
        wgsl.matches(" - ").count(),
        wgsl.matches(" / ").count(),
        wgsl.matches("sqrt(").count(),
        wgsl.matches("fn ").count(),
    );
    assert_eq!(
        arithmetic_shape,
        (91, 173, 12, 4, 3, 2),
        "canonical50 sparse facade changed the kernel arithmetic shape:\n{wgsl}",
    );

    // Measure what actually ships. `WasmCompileOptions::default()` leaves
    // Sonatina's pipeline OFF, so this used to size an unoptimized module: with
    // the canonical50 schedule that is 28,641 bytes against a 1,456-byte
    // baseline taken when the body was the small nine-scalar direct kernel.
    // Sizing an artifact nobody ships is not a size gate.
    let wasm = compile_runtime_package_wasm_with_options(
        &db,
        &package,
        WasmCompileOptions::default().with_optimization(),
    )
    .expect("sparse facade should compile through the browser Wasm backend");
    wasmparser::validate(&wasm.bytes).expect("facade Wasm must validate");
    eprintln!("single-sandwich Wasm bytes: {}", wasm.bytes.len());
    // 1,723 is the measured optimized size of the canonical50 aggregate, and it
    // is SMALLER than the 2,147-byte schedule it replaced. The old 1,456 ceiling
    // measured the pre-canonical50 nine-scalar body. Unoptimized this module is
    // 28,641 bytes, so the gate is only meaningful with the pipeline on.
    // Ratchet down, never up.
    assert!(
        wasm.bytes.len() <= 1723,
        "aggregate facade exceeded the 1723-byte optimized baseline: {} bytes",
        wasm.bytes.len(),
    );
    let defined_functions = wasmparser::Parser::new(0)
        .parse_all(&wasm.bytes)
        .filter_map(|payload| match payload.expect("valid Wasm payload") {
            wasmparser::Payload::FunctionSection(reader) => Some(reader.count()),
            _ => None,
        })
        .sum::<u32>();
    assert!(
        defined_functions <= 6,
        "the aggregate semantic facade introduced Wasm helper functions: {defined_functions}",
    );
}
