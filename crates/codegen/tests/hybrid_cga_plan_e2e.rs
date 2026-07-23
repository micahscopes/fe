use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

fn compile_to_wasm(source: &str) -> Vec<u8> {
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///wasm_cga_semantic_plan_hybrid.fe").unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected fixture diagnostics:\n{diagnostics}"
    );
    let bytes = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("hybrid semantic plan should compile to Wasm")
        .into_bytecode()
        .expect("Wasm output should be bytecode");
    wasmparser::validate(&bytes).expect("hybrid plan emitted invalid Wasm");
    bytes
}

fn gp_sign_cl41(a: usize, b: usize) -> f32 {
    let mut negative = false;
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
    if negative { -1.0 } else { 1.0 }
}

fn raw_80_oracle(sphere: [f32; 4], point: [f32; 5]) -> [f32; 32] {
    let sb = [1usize, 2, 8, 16];
    let pb = [1usize, 2, 4, 8, 16];
    let mut out = [0.0; 32];
    for (li, &l) in sb.iter().enumerate() {
        for (pi, &p) in pb.iter().enumerate() {
            for (ri, &r) in sb.iter().enumerate() {
                let sign = gp_sign_cl41(l, p) * gp_sign_cl41(l ^ p, r);
                out[l ^ p ^ r] += sign * sphere[li] * point[pi] * sphere[ri];
            }
        }
    }
    out
}

#[test]
fn typed_schedule_interpretation_matches_raw_80_oracle() {
    let source = include_str!("fixtures/wasm_cga_semantic_plan_hybrid.fe");
    let wasm = compile_to_wasm(source);
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(
        module.imports().next().is_none(),
        "specialized plan should have no imports"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let eval = instance
        .get_typed_func::<(i32, f32, f32, f32, f32, f32, f32, f32, f32, f32), i32>(
            &mut store,
            "cga_semantic_plan_hybrid",
        )
        .expect("hybrid plan export ABI");

    for (sphere, point) in [
        ([0.5, -0.25, -0.875, 0.125], [2.0, 0.5, -0.75, 1.25, 1.75]),
        ([1.0, 0.5, -1.0, 0.25], [-0.5, 2.0, 0.25, -1.5, 1.0]),
    ] {
        let expected = raw_80_oracle(sphere, point);
        for (output, want) in expected.into_iter().enumerate() {
            let got = eval
                .call(
                    &mut store,
                    (
                        output as i32,
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
            assert_eq!(got, (want * 256.0) as i32, "output blade {output}");
        }
    }

    // Cross-check the specialization against the separately authored generic
    // Fe baseline. That implementation recursively interprets two full
    // `MvTF<5>` geometric products; it does not use this schedule or its
    // support-pruning functions.
    let generic_source = include_str!("fixtures/spirv/cga_sandwich_authored_generic_mvt5.fe");
    let generic_wasm = compile_to_wasm(generic_source);
    let generic_module = wasmtime::Module::new(&engine, &generic_wasm).unwrap();
    let mut generic_store = wasmtime::Store::new(&engine, ());
    let generic_instance =
        wasmtime::Instance::new(&mut generic_store, &generic_module, &[]).unwrap();
    let generic = generic_instance
        .get_typed_func::<(i32, i32, f32, f32, f32, f32, f32), i32>(
            &mut generic_store,
            "cga_sandwich_authored_generic_mvt5",
        )
        .expect("generic Fe MvTF<5> sandwich ABI");

    for (x, y, z, cx, cy) in [
        (2.5f32, 0.25f32, 0.0f32, 0.5f32, 0.25f32),
        (0.5f32, 2.25f32, 0.0f32, 0.5f32, 0.25f32),
    ] {
        let radius2 = x * x + y * y + z * z;
        let center2 = cx * cx + cy * cy;
        let sphere = [cx, cy, center2 * 0.5 - 1.0, center2 * 0.5];
        let point = [x, y, z, (radius2 - 1.0) * 0.5, (radius2 + 1.0) * 0.5];
        for output in 0..32 {
            let specialized = eval
                .call(
                    &mut store,
                    (
                        output, sphere[0], sphere[1], sphere[2], sphere[3], point[0], point[1],
                        point[2], point[3], point[4],
                    ),
                )
                .unwrap();
            let baseline = generic
                .call(
                    &mut generic_store,
                    (output % 8, output / 8, x, y, z, cx, cy),
                )
                .unwrap();
            assert_eq!(specialized, baseline, "generic Fe baseline blade {output}");
        }
    }
}
