use common::InputDb;
use driver::DriverDataBase;
use sonatina_codegen::isa::spirv::SpirvArtifact;
use url::Url;

fn compile(source: &str, name: &str) -> Result<SpirvArtifact, fe_codegen::LowerError> {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}.fe")).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let diagnostics = db.run_on_top_mod(top_mod).format_diags(&db);
    assert!(
        diagnostics.is_empty(),
        "unexpected diagnostics:\n{diagnostics}"
    );
    let package = mir::build_wasm_runtime_package(&db, top_mod).expect("runtime package");
    fe_codegen::compile_runtime_package_spirv(&db, &package)
}

fn compile_with_planned_root_arity(source: &str, name: &str) -> (SpirvArtifact, usize) {
    let mut db = DriverDataBase::default();
    let url = Url::parse(&format!("file:///{name}.fe")).unwrap();
    db.workspace()
        .touch(&mut db, url.clone(), Some(source.to_string()));
    let file = db.workspace().get(&db, &url).unwrap();
    let top_mod = db.top_mod(file);
    let package = mir::build_wasm_runtime_package(&db, top_mod).expect("runtime package");
    let first_root = package.root_objects(&db)[0].sections(&db)[0]
        .entry
        .instance(&db);
    let planned_arity = first_root.body(&db).signature.params.len();
    let artifact = fe_codegen::compile_runtime_package_spirv(&db, &package)
        .expect("planned root should compile");
    (artifact, planned_arity)
}

#[test]
fn inline_never_residual_call_fails_closed_with_callee_name() {
    let source = r#"
#[inline(never)]
fn retained_helper(x: i32) -> i32 { x * 3 + 1 }
pub fn inline_never_entry(x: i32) -> i32 { retained_helper(x: x) }
"#;
    let error = match compile(source, "spirv_inline_never_guard") {
        Ok(_) => panic!("SPIR-V must reject a deliberately retained call"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("not call-free"), "unexpected error: {error}");
    assert!(
        error.contains("retained_helper"),
        "callee missing from error: {error}"
    );
}

#[test]
fn multiple_public_roots_select_planned_entry_deterministically() {
    let source = r#"
pub fn z_first_entry(x: i32) -> i32 { x + 11 }
pub fn a_second_entry(x: i32, y: i32) -> i32 { x + y + 22 }
"#;
    let (first, first_planned_arity) =
        compile_with_planned_root_arity(source, "spirv_multiple_roots_guard");
    let (second, second_planned_arity) =
        compile_with_planned_root_arity(source, "spirv_multiple_roots_guard_repeat");
    assert_eq!(
        first.words, second.words,
        "entry selection must be deterministic"
    );
    let input_members = first
        .layout
        .bindings
        .iter()
        .map(|binding| binding.members.len())
        .sum::<usize>();
    assert_eq!(first_planned_arity, second_planned_arity);
    assert_eq!(input_members, first_planned_arity);
    let wgsl = first.wgsl.as_deref().expect("WGSL side artifact");
    let (selected, rejected) = if first_planned_arity == 1 {
        ("11", "22")
    } else {
        ("22", "11")
    };
    assert!(
        wgsl.contains(selected),
        "planned root body missing:\n{wgsl}"
    );
    assert!(
        !wgsl.contains(rejected),
        "other root leaked into WGSL:\n{wgsl}"
    );
}

#[test]
fn excessive_non_always_inlinee_stays_capped_and_fails_closed() {
    let mut source = String::from("fn oversized_helper(x: i32) -> i32 {\n");
    for index in 0..70 {
        source.push_str(&format!("if x == {index} {{ return {index} }}\n"));
    }
    source.push_str("x\n}\npub fn oversized_entry(x: i32) -> i32 { oversized_helper(x: x) }\n");

    let error = match compile(&source, "spirv_ordinary_growth_guard") {
        Ok(_) => panic!("oversized ordinary inlinee must remain capped"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("not call-free"), "unexpected error: {error}");
    assert!(
        error.contains("oversized_helper"),
        "callee missing from error: {error}"
    );
}
