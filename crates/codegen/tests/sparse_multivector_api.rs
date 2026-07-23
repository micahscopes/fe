use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{compile_runtime_package_wasm_with_options, WasmCompileOptions};
use url::Url;

const SUPPORT: &str = include_str!("fixtures/support_bladeset_api.fe");
const VALUE_API: &str = include_str!("fixtures/sparse_multivector_api.fe");
const CGA: &str = include_str!("fixtures/sparse_cga_value.fe");
const WIDE_VALUE_PROBE: &str = r#"
fn takes_sparse_index7(_ value: SparseIndex<7>) {}
fn selected_rank7_is_recursive(
    value: <SparseSelect<1, 7> as SparseSelectOut>::Out,
) {
    takes_sparse_index7(value)
}

fn sparse_eight(
    a: f32, b: f32, c: f32, d: f32,
    e: f32, f: f32, g: f32, h: f32,
) -> SparseStorage<8> {
    SparseCell {
        head: a,
        tail: SparseCell {
            head: b,
            tail: SparseCell {
                head: c,
                tail: SparseCell {
                    head: d,
                    tail: SparseCell {
                        head: e,
                        tail: SparseCell {
                            head: f,
                            tail: SparseCell {
                                head: g,
                                tail: SparseCell { head: h, tail: SparseNil {} },
                            },
                        },
                    },
                },
            },
        },
    }
}

pub fn sparse_eight_last(
    a: f32, b: f32, c: f32, d: f32,
    e: f32, f: f32, g: f32, h: f32,
) -> f32 {
    <SparseIndex<7> as SparsePresentCoefficient<SparseStorage<8>>>::read_present(
        value: sparse_eight(a: a, b: b, c: c, d: d, e: e, f: f, g: g, h: h),
    )
}

pub fn sparse_eight_missing(
    a: f32, b: f32, c: f32, d: f32,
    e: f32, f: f32, g: f32, h: f32,
) -> f32 {
    <SparseMissing as SparseCoefficient<SparseStorage<8>>>::read(
        value: sparse_eight(a: a, b: b, c: c, d: d, e: e, f: f, g: g, h: h),
    )
}
"#;

fn with_top_mod<T>(
    source: String,
    url: &str,
    f: impl for<'db> FnOnce(&'db DriverDataBase, hir::hir_def::TopLevelMod<'db>) -> T,
) -> T {
    let mut db = DriverDataBase::default();
    let url = Url::parse(url).unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    f(&db, top_mod)
}

#[test]
fn support_driven_sparse_cga_values_and_absent_accessor_execute_in_wasm() {
    let source = format!("{SUPPORT}\n{VALUE_API}\n{CGA}");
    let wasm = with_top_mod(source, "file:///sparse_cga_value.fe", |db, top_mod| {
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(db);
        assert!(
            diagnostics.is_empty(),
            "unexpected sparse-value diagnostics:\n{diagnostics}"
        );
        let package = mir::build_wasm_runtime_package_for_entry(
            db,
            top_mod,
            "sparse_cga_sphere_default_zero",
        )
        .expect("actual sparse sphere default-zero entry should lower");
        compile_runtime_package_wasm_with_options(db, &package, WasmCompileOptions::default())
            .expect("actual sparse sphere default-zero entry should compile")
            .bytes
    });
    wasmparser::validate(&wasm).expect("sparse-value Wasm must validate");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let probe = instance
        .get_typed_func::<(f32, f32), f32>(&mut store, "sparse_cga_sphere_default_zero")
        .unwrap();
    assert_eq!(probe.call(&mut store, (2.0, 4.0)).unwrap(), 0.0);
}

#[test]
fn present_only_access_rejects_an_absent_blade() {
    let source = format!(
        "{SUPPORT}\n{VALUE_API}\n{CGA}\n\
         pub fn rejected(value: CgaInversionSphere) -> f32 {{\n\
             <CgaSphereLookup<4, 4> as \
                 SparsePresentCoefficient<CgaInversionSphere>>::read_present(\
                     value: value\
                 )\n\
         }}\n"
    );
    let rejected = with_top_mod(source, "file:///sparse_cga_rejection.fe", |db, top_mod| {
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(db);
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        mir::build_wasm_runtime_package_for_entry(db, top_mod, "rejected")
            .map(|_| ())
            .map_err(|error| error.to_string())
    });
    assert!(
        rejected.is_err(),
        "an absent present-only coefficient must fail before executable Wasm exists"
    );
}

#[test]
fn recursive_rank_and_support_sized_storage_execute_beyond_five_lanes() {
    assert!(
        !VALUE_API.contains("SparseFound<"),
        "the reusable API must not retain bounded rank-specific accessors"
    );
    let source = format!("{VALUE_API}\n{WIDE_VALUE_PROBE}");
    let wasm = with_top_mod(source, "file:///sparse_wide_value.fe", |db, top_mod| {
        let diagnostics = db.run_on_top_mod(top_mod).format_diags(db);
        assert!(
            diagnostics.is_empty(),
            "unexpected recursive sparse-value diagnostics:\n{diagnostics}"
        );
        let package = mir::build_wasm_runtime_package(db, top_mod)
            .expect("recursive eight-lane sparse entries should lower");
        compile_runtime_package_wasm_with_options(db, &package, WasmCompileOptions::default())
            .expect("recursive eight-lane sparse entries should compile")
            .bytes
    });
    wasmparser::validate(&wasm).expect("recursive sparse-value Wasm must validate");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(module.imports().next().is_none());
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    type Inputs = (f32, f32, f32, f32, f32, f32, f32, f32);
    let last = instance
        .get_typed_func::<Inputs, f32>(&mut store, "sparse_eight_last")
        .unwrap();
    let missing = instance
        .get_typed_func::<Inputs, f32>(&mut store, "sparse_eight_missing")
        .unwrap();
    let values: Inputs = (1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0);
    assert_eq!(last.call(&mut store, values).unwrap(), 8.0);
    assert_eq!(missing.call(&mut store, values).unwrap(), 0.0);
}
