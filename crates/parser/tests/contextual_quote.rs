use fe_parser::{RecoveryMode, parse_source_file};

/// `quote` starts a quote only when its body opens on the same line, so a
/// call to a function named `quote` may be followed by a block statement.
#[test]
fn quote_call_followed_by_a_block_on_the_next_line_is_a_call() {
    let source = "fn quote(_ x: u256) -> u256 { x }\nfn f(a: u256) -> u256 {\n    quote(a)\n    { let _: u256 = 1 }\n    a\n}\n";
    let (_, errors) = parse_source_file(source, RecoveryMode::Recover);
    assert!(errors.is_empty(), "{errors:?}");
}

/// A local named `quote` followed by a block on the next line reads as it
/// does on master, without a panic.
#[test]
fn quote_value_followed_by_a_block_on_the_next_line_does_not_panic() {
    let source = "fn g(quote: u256) -> u256 {\n    quote\n    { let _: u256 = 1 }\n    0\n}\n";
    parse_source_file(source, RecoveryMode::Recover);
}
