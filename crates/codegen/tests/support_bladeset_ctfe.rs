use common::InputDb;
use driver::DriverDataBase;
use fe_codegen::{BackendKind, OptLevel, layout_for};
use url::Url;

const API: &str = include_str!("../../../ingots/sparse_clifford/src/lib.fe");
const PROBES: &str = include_str!("fixtures/support_bladeset_ctfe.fe");

fn gp_oracle(left: u32, right: u32, dimension: u32, metric: u32) -> u32 {
    if dimension > 5 {
        return 0;
    }
    let count = 1u32 << dimension;
    let mut output = 0u32;
    for l in 0..count {
        if left & (1 << l) == 0 {
            continue;
        }
        for r in 0..count {
            if right & (1 << r) == 0 {
                continue;
            }
            let shared = l & r;
            if shared & metric == shared {
                output |= 1 << (l ^ r);
            }
        }
    }
    output
}

fn grade_oracle(bits: u32, dimension: u32, grade: u32) -> u32 {
    if dimension > 5 {
        return 0;
    }
    let count = 1u32 << dimension;
    (0..count).fold(0, |output, blade| {
        if bits & (1 << blade) != 0 && blade.count_ones() == grade {
            output | (1 << blade)
        } else {
            output
        }
    })
}

#[test]
fn reusable_bladeset_ctfe_matches_independent_support_oracles() {
    let mut db = DriverDataBase::default();
    let source = format!("{API}\n{PROBES}");
    let url = Url::parse("file:///support_bladeset_ctfe.fe").unwrap();
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected BladeSet diagnostics:\n{diagnostics}"
    );

    let wasm = BackendKind::Wasm
        .create()
        .compile(&db, top_mod, layout_for(BackendKind::Wasm), OptLevel::O0)
        .expect("BladeSet fixture should compile to Wasm")
        .into_bytecode()
        .expect("Wasm bytecode");
    wasmparser::validate(&wasm).expect("BladeSet Wasm must validate");

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm).unwrap();
    assert!(
        module.imports().next().is_none(),
        "support algebra must be closed"
    );
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let ctfe_case = instance
        .get_typed_func::<i32, i32>(&mut store, "support_ctfe_case")
        .unwrap();

    let expected = [
        gp_oracle(22, 22, 3, 7),
        gp_oracle(146, 73, 3, 7),
        // CGA Point*Sphere, derived without a CGA-specific Fe case/table.
        gp_oracle(65_814, 65_798, 5, 31),
        gp_oracle(16, 16, 3, 3),
        gp_oracle(255, 255, 3, 0),
        grade_oracle(105, 3, 2),
        grade_oracle(u32::MAX, 5, 1),
        grade_oracle(65_798, 5, 1),
        grade_oracle(255, 3, 4),
        grade_oracle(u32::MAX, 6, 1),
    ];
    for (index, want) in expected.into_iter().enumerate() {
        let got = ctfe_case.call(&mut store, index as i32).unwrap() as u32;
        assert_eq!(got, want, "CTFE support case {index}");
    }

    let mut runtime_bitwise_ops = 0;
    let mut defined_functions = 0;
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.unwrap() {
            wasmparser::Payload::FunctionSection(reader) => {
                defined_functions += reader.count();
            }
            wasmparser::Payload::CodeSectionEntry(body) => {
                let mut ops = body.get_operators_reader().unwrap();
                while !ops.eof() {
                    use wasmparser::Operator;
                    if matches!(
                        ops.read().unwrap(),
                        Operator::I32And
                            | Operator::I32Or
                            | Operator::I32Xor
                            | Operator::I32Shl
                            | Operator::I32ShrU
                    ) {
                        runtime_bitwise_ops += 1;
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        defined_functions, 1,
        "only the constant result selector should reach runtime Wasm",
    );
    assert_eq!(
        runtime_bitwise_ops, 0,
        "support planning must erase completely instead of falling back at runtime",
    );
}
