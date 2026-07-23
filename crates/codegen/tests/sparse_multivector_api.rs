use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{compile_runtime_package_wasm_with_options, WasmCompileOptions};
use url::Url;

const SUPPORT: &str = include_str!("fixtures/support_bladeset_api.fe");
const VALUE_API: &str = include_str!("fixtures/sparse_multivector_api.fe");
const CGA: &str = include_str!("fixtures/sparse_cga_value.fe");

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
