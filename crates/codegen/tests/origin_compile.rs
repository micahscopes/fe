#[test]
fn end_to_end_origin_owner_keys_do_not_cross_namespaces() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/end_to_end_origin_owner_key_mismatch.rs");
}

#[test]
fn bytecode_source_map_filter_requires_typed_section_key() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/bytecode_source_map_filter_raw_strings.rs");
}

#[test]
fn bytecode_source_map_export_metadata_rejects_raw_strings() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/bytecode_source_map_export_metadata_raw_strings.rs");
}

#[test]
fn bytecode_source_map_entry_requires_constructor() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/bytecode_source_map_entry_private_fields.rs");
}

#[test]
fn bytecode_source_map_entry_rejects_raw_parts() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/bytecode_source_map_entry_raw_parts.rs");
}

#[test]
fn bytecode_source_map_entry_kind_rejects_raw_strings() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/bytecode_source_map_entry_kind_raw_strings.rs");
}

#[test]
fn sonatina_function_export_key_rejects_raw_strings() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/sonatina_function_export_key_raw_strings.rs");
}

#[test]
fn bytecode_object_key_rejects_raw_strings() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/bytecode_object_key_raw_strings.rs");
}

#[test]
fn codegen_origin_fact_export_requires_nominal_graph() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/codegen_origin_fact_export_raw_graph.rs");
}

#[test]
fn frontend_origin_label_map_is_not_raw_sonatina_provenance_map() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/ui/frontend_origin_label_map_raw_sonatina_map.rs");
}
