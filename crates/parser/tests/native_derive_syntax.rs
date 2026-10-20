use fe_parser::{
    RecoveryMode, SyntaxNode,
    ast::{self, prelude::*},
};

#[test]
fn derive_protocol_retains_its_static_const_method() {
    let source = include_str!("../../../ingots/core/src/derive.fe");
    let (green, errors) = fe_parser::parse_source_file(source, RecoveryMode::Recover);
    let root = ast::Root::cast(SyntaxNode::new_root(green)).unwrap();
    assert!(errors.is_empty(), "{errors:?}\n{:#?}", root.syntax());
    let definition = root
        .syntax()
        .descendants()
        .find_map(ast::Trait::cast)
        .unwrap();
    let methods = definition
        .item_list()
        .unwrap()
        .into_iter()
        .filter_map(|item| match item.kind() {
            ast::TraitItemKind::Func(function) => {
                function.sig().name().map(|name| name.text().to_string())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(methods, ["derive"]);
}

#[test]
fn native_declared_and_named_multiline_headers_keep_provider_paths() {
    let source = "impl Score for Pair with (\n Fields\n)\nimpl Score for Pair as Zero with (mod_ :: Provider)\n";
    let (green, errors) = fe_parser::parse_source_file(source, RecoveryMode::Recover);
    let root = ast::Root::cast(SyntaxNode::new_root(green)).unwrap();
    assert!(errors.is_empty(), "{errors:?}");
    let headers = root
        .syntax()
        .descendants()
        .filter_map(ast::ImplTrait::cast)
        .collect::<Vec<_>>();
    assert_eq!(headers.len(), 2);
    assert!(headers[0].selection_name().is_none());
    assert_eq!(headers[1].selection_name().unwrap().text(), "Zero");
    assert!(
        headers
            .iter()
            .all(|header| header.derive_provider().is_some())
    );
}

#[test]
fn constraint_kind_bounds_only_parse_as_unparenthesized_codomain_or_bare_bound() {
    for valid in [
        "trait Foo<G: Constraint> {}\n",
        "trait Foo<G: * -> Constraint> {}\n",
        "trait Foo<G: * -> * -> Constraint> {}\n",
        "trait Foo<G: (* -> *) -> Constraint> {}\n",
    ] {
        let (_, errors) = fe_parser::parse_source_file(valid, RecoveryMode::Recover);
        assert!(
            errors.is_empty(),
            "expected valid kind bound for `{valid}`, got {errors:?}"
        );
    }

    for invalid in [
        "trait Foo<G: Constraint -> *> {}\n",
        "trait Foo<G: (Constraint)> {}\n",
        "trait Foo<G: (Constraint) -> Constraint> {}\n",
        "trait Foo<G: * -> Constraint -> *> {}\n",
    ] {
        let (_, errors) = fe_parser::parse_source_file(invalid, RecoveryMode::Recover);
        assert!(!errors.is_empty(), "expected parse error for `{invalid}`");
    }
}
