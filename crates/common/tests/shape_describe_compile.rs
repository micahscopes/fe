#[test]
fn shape_describe_compile_failures_are_enforced() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/shape_describe_empty_skip_reason.rs");
    cases.compile_fail("tests/ui/shape_describe_empty_kind_or_label.rs");
    cases.compile_fail("tests/ui/shape_describe_duplicate_attrs.rs");
    cases.compile_fail("tests/ui/shape_describe_multiple_policies.rs");
    cases.compile_fail("tests/ui/shape_describe_missing_policy.rs");
    cases.compile_fail("tests/ui/shape_describe_unknown_dimension.rs");
    cases.compile_fail("tests/ui/shape_describe_unknown_attrs.rs");
}
