#[test]
fn runtime_fact_owner_keys_do_not_cross_namespaces() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/runtime_fact_owner_key_mismatch.rs");
}

#[test]
fn runtime_package_body_origin_requires_typed_symbol() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/runtime_package_body_origin_raw_symbol.rs");
}
