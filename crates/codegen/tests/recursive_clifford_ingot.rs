use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{WasmCompileOptions, compile_runtime_package_wasm_with_options};
use hir::hir_def::HirIngot;
use url::Url;

const CONSUMER_SOURCE: &str = include_str!("fixtures/recursive_clifford_consumer_ingot/src/lib.fe");

fn raw_80(sphere: [f32; 4], point: [f32; 5]) -> [f32; 32] {
    let sphere_blades = [1usize, 2, 8, 16];
    let point_blades = [1usize, 2, 4, 8, 16];
    let mut out = [0.0; 32];
    for (li, &left) in sphere_blades.iter().enumerate() {
        for (pi, &middle) in point_blades.iter().enumerate() {
            for (ri, &right) in sphere_blades.iter().enumerate() {
                let mut negative = false;
                for (a, b) in [(left, middle), (left ^ middle, right)] {
                    for bit in 0..5 {
                        if a & (1 << bit) != 0 {
                            if (b & ((1 << bit) - 1)).count_ones() & 1 != 0 {
                                negative = !negative;
                            }
                            if bit == 4 && b & (1 << bit) != 0 {
                                negative = !negative;
                            }
                        }
                    }
                }
                let product = sphere[li] * point[pi] * sphere[ri];
                out[left ^ middle ^ right] += if negative { -product } else { product };
            }
        }
    }
    out
}

fn consumer_url() -> Url {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/recursive_clifford_consumer_ingot");
    Url::from_directory_path(path.canonicalize().unwrap()).unwrap()
}

fn assert_consumer_diagnostics(db: &DriverDataBase, top_mod: hir::hir_def::TopLevelMod<'_>) {
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(db);
    assert!(
        diagnostics.is_empty(),
        "unexpected recursive-Clifford ingot diagnostics:\n{diagnostics}"
    );
}

#[test]
fn consumer_uses_public_authored_recurrence_without_semantic_codegen() {
    assert!(CONSUMER_SOURCE.contains("use sparse_clifford::{"));
    assert!(CONSUMER_SOURCE.contains("CliffordGp"));
    assert!(CONSUMER_SOURCE.contains("Cl41Metric"));
    for forbidden in [
        "include_str",
        "SparsePlan",
        "Term<",
        "gp_negative",
        "survivor",
        "python",
    ] {
        assert!(
            !CONSUMER_SOURCE.contains(forbidden),
            "ordinary consumer must not reconstruct algebra through `{forbidden}`"
        );
    }
}

#[test]
fn public_authored_cl41_recurrence_matches_raw_80_in_wasm() {
    let url = consumer_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("consumer ingot");
    let top_mod = ingot.root_mod(&db);
    assert_consumer_diagnostics(&db, top_mod);
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "authored_cl41_wasm")
        .expect("public authored recurrence should build a runtime package");
    let wasm =
        compile_runtime_package_wasm_with_options(&db, &package, WasmCompileOptions::default())
            .expect("public authored recurrence should compile to Wasm")
            .bytes;
    wasmparser::validate(&wasm).expect("authored recurrence Wasm validates");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let authored = instance
        .get_typed_func::<(i32, f32, f32, f32, f32, f32, f32, f32, f32, f32), i32>(
            &mut store,
            "authored_cl41_wasm",
        )
        .unwrap();

    for (sphere, point) in [
        ([0.5, -0.25, -0.875, 0.125], [2.0, 0.5, -0.75, 1.25, 1.75]),
        ([1.0, 0.5, -1.0, 0.25], [-0.5, 2.0, 0.25, -1.5, 1.0]),
    ] {
        let expected = raw_80(sphere, point);
        for (blade, coefficient) in expected.into_iter().enumerate() {
            let actual = authored
                .call(
                    &mut store,
                    (
                        blade as i32,
                        sphere[0],
                        sphere[1],
                        sphere[2],
                        sphere[3],
                        point[0],
                        point[1],
                        point[2],
                        point[3],
                        point[4],
                    ),
                )
                .unwrap();
            assert_eq!(actual, (coefficient * 256.0) as i32, "blade {blade}");
        }
    }
}

#[test]
fn public_authored_cl41_recurrence_emits_browser_profile_wgsl() {
    let url = consumer_url();
    let mut db = DriverDataBase::default();
    assert!(
        !driver::init_ingot(&mut db, &url),
        "ingot initialization diagnostics"
    );
    let ingot = db
        .workspace()
        .containing_ingot(&db, url)
        .expect("consumer ingot");
    let top_mod = ingot.root_mod(&db);
    assert_consumer_diagnostics(&db, top_mod);
    let package = mir::build_wasm_runtime_package_for_entry(&db, top_mod, "authored_cl41_render")
        .expect("public authored recurrence Render package");
    let artifact = fe_codegen::compile_runtime_package_spirv_render(&db, &package)
        .expect("public authored recurrence Render SPIR-V/WGSL");
    let wgsl = artifact.wgsl.expect("Render compilation emits WGSL");
    let module = naga::front::wgsl::parse_str(&wgsl)
        .unwrap_or_else(|error| panic!("emitted WGSL should reparse: {error:?}\n{wgsl}"));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|error| panic!("browser-profile WGSL invalid: {error:?}\n{wgsl}"));
    assert!(
        !wgsl.contains(" == 5") && !wgsl.contains("== 5"),
        "closed Cl41 metric selection must specialize away:\n{wgsl}"
    );
}
