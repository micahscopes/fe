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

#[test]
fn runtime_origin_wrappers_do_not_expose_raw_keys() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/runtime_origin_raw_key_escape.rs");
}

#[test]
fn runtime_terminator_origin_requires_typed_site() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/runtime_terminator_origin_raw_block.rs");
}

#[test]
fn runtime_sites_do_not_expose_raw_local_export_keys() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/runtime_site_export_local_key_escape.rs");
}
