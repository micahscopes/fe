use super::*;

#[test]
fn frontend_origin_label_map_stores_nominal_labels() {
    let function = FuncRef::from_u32(0);
    let inst = InstId::from_u32(7);
    let mut labels = FrontendOriginLabelMap::new();

    labels.insert_if_absent(
        function,
        inst,
        FrontendOriginLabel::new("runtime.stmt:owner:stmt"),
    );
    labels.insert_if_absent(
        function,
        inst,
        FrontendOriginLabel::new("runtime.stmt:owner:ignored"),
    );

    assert_eq!(
        labels
            .as_sonatina_frontend_provenance()
            .get(&(function, inst))
            .map(String::as_str),
        Some("runtime.stmt:owner:stmt")
    );
}

#[test]
fn bytecode_frontend_origin_label_map_ignores_non_label_sources_without_function_keys() {
    let function = FuncRef::from_u32(2);
    let inst = InstId::from_u32(9);
    let origins = bytecode_package_with_same_id_source(
        function,
        inst,
        SonatinaOriginSource::Synthetic(SonatinaSyntheticOrigin::Prologue),
    );

    let labels = origins
        .try_frontend_origin_label_map(|_| None)
        .expect("synthetic pre-opt sources do not need frontend origin labels");

    assert!(labels.as_sonatina_frontend_provenance().is_empty());
}

#[test]
fn bytecode_frontend_origin_label_map_requires_stable_function_keys_for_runtime_sources() {
    let mut db = DriverDataBase::default();
    let function = FuncRef::from_u32(2);
    let inst = InstId::from_u32(9);
    let runtime_origin = runtime_stmt_origin_for_fixture(&mut db);
    let origins = bytecode_package_with_same_id_source(
        function,
        inst,
        SonatinaOriginSource::RuntimeStmt(runtime_origin),
    );

    let err = origins
        .try_frontend_origin_label_map(|_| None)
        .expect_err("frontend origin label derivation should require stable function keys");

    assert_eq!(err.function(), function);
}

#[test]
#[should_panic(expected = "origin string key must not be empty")]
fn frontend_origin_label_rejects_empty_labels() {
    FrontendOriginLabel::new("");
}
