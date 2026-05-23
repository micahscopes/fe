#[test]
fn origin_export_key_constructor_rejects_raw_strings() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/origin_export_key_raw_strings.rs");
}

#[test]
fn typed_fact_relation_queries_reject_raw_column_strings() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/typed_fact_relation_raw_column_strings.rs");
}

#[test]
fn typed_fact_relation_constructor_rejects_raw_schema_parts() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/typed_fact_relation_raw_constructor.rs");
}

#[test]
fn typed_fact_relation_count_rejects_raw_relation_strings() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/typed_fact_relation_count_raw_relation.rs");
}
