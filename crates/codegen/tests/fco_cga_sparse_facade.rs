use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{
    WasmCompileOptions, compile_runtime_package_spirv_render,
    compile_runtime_package_wasm_with_options,
};
use url::Url;

const CANONICAL: &str = include_str!("fixtures/fco_cga80_direct_lanes.fe");
const SPARSE_CLIFFORD_API: &str = include_str!("../../../ingots/sparse_clifford/src/lib.fe");
const BODY: &str = include_str!("fixtures/spirv/fco_cga80_direct_de_body.fe");
const ENTRY: &str = "cga_schedule32_vec5_de_render";

fn composed_source() -> String {
    let (prefix, rest) = CANONICAL
        .split_once("// BEGIN_PUBLIC_ORACLES")
        .expect("canonical public-oracle begin marker");
    let (_, suffix) = rest
        .split_once("// END_PUBLIC_ORACLES")
        .expect("canonical public-oracle end marker");
    format!("{SPARSE_CLIFFORD_API}\n{prefix}{suffix}\n{BODY}")
}

#[test]
fn semantic_sparse_facade_erases_to_the_direct_schedule32_kernel_shape() {
    assert!(CANONICAL.contains("type ConformalPoint = ConformalVector"));
    assert!(CANONICAL.contains("type ConformalSphere = ConformalVector"));
    assert!(CANONICAL.contains("struct ConformalVector {"));
    assert!(BODY.contains("let point: ConformalPoint = ConformalPoint {"));
    assert!(BODY.contains("let sphere: ConformalSphere = ConformalSphere {"));
    assert_eq!(
        BODY.matches("<ConformalVector as Sandwich>::sandwich")
            .count(),
        1,
        "the compact semantic records should feed one specialized aggregate method",
    );
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

    // Baseline from the direct nine-scalar body before introducing the facade.
    // Equality of this complete arithmetic signature proves the semantic
    // records and generic wrapper did not add runtime algebra or calls.
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
        (79, 82, 13, 4, 3, 2),
        "semantic sparse facade changed the direct kernel arithmetic shape:\n{wgsl}",
    );

    let wasm =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .expect("sparse facade should compile through the browser Wasm backend");
    wasmparser::validate(&wasm.bytes).expect("facade Wasm must validate");
    eprintln!("single-sandwich Wasm bytes: {}", wasm.bytes.len());
    assert!(
        wasm.bytes.len() <= 1456,
        "aggregate facade exceeded the 1456-byte baseline: {} bytes",
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
