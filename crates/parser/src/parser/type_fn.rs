//! Recursive type function declarations. Arm right-hand sides use the type
//! grammar; definition checking in HIR establishes recursion and parameter
//! requirements.
use super::{
    ErrProof, Parser, Recovery, define_scope,
    param::{ItemBlock, parse_generic_params_opt, parse_kind_bound, parse_where_clause_opt},
    token_stream::TokenStream,
    type_::parse_type,
};
use crate::{ExpectedKind, SyntaxKind};

define_scope! { pub(super) RecursiveTypeFnScope, RecursiveTypeFn }
impl super::Parse for RecursiveTypeFnScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.set_newline_as_trivia(false);
        debug_assert!(parser.is_ident("recursive"));
        parser.bump(); // contextual `recursive`
        if !parser.bump_if(SyntaxKind::TypeKw) {
            parser.error("expected `type` after `recursive`");
        }
        if !parser.bump_if(SyntaxKind::FnKw) {
            parser.error("expected `fn` after `recursive type`");
        }

        parser.set_scope_recovery_stack(&[
            SyntaxKind::Ident,
            SyntaxKind::Lt,
            SyntaxKind::LParen,
            SyntaxKind::Arrow,
            SyntaxKind::WhereKw,
            SyntaxKind::LBrace,
        ]);

        if parser.find_and_pop(
            SyntaxKind::Ident,
            ExpectedKind::Name(SyntaxKind::RecursiveTypeFn),
        )? {
            parser.bump();
        }

        parser.expect_and_pop_recovery_stack()?;
        parse_generic_params_opt(parser, false)?;

        // The value-parameter list is mandatory and must be empty.
        if parser.find_and_pop(
            SyntaxKind::LParen,
            ExpectedKind::Syntax(SyntaxKind::RecursiveTypeFn),
        )? {
            parser.bump();
            if parser.current_kind() != Some(SyntaxKind::RParen) {
                parser.error(
                    "`recursive type fn` takes no value parameters; the parameter list must be empty `()`",
                );
            }
            parser.bump_or_recover(
                SyntaxKind::RParen,
                "expected `)` to close the empty parameter list",
            )?;
        }

        // The return kind is required.
        if parser.find_and_pop(
            SyntaxKind::Arrow,
            ExpectedKind::Syntax(SyntaxKind::TypeFnRetKind),
        )? {
            parser.bump();
            parser.parse(TypeFnRetKindScope::default())?;
        }

        // The `where` clause and body may begin on a following line.
        parser.set_newline_as_trivia(true);
        parser.expect_and_pop_recovery_stack()?;
        parse_where_clause_opt(parser, ItemBlock::Required)?;

        parser.set_newline_as_trivia(true);
        if parser.find_and_pop(
            SyntaxKind::LBrace,
            ExpectedKind::Body(SyntaxKind::RecursiveTypeFn),
        )? {
            parser.parse(TypeFnBodyScope::default())?;
        }

        Ok(())
    }
}

define_scope! { TypeFnRetKindScope, TypeFnRetKind }
impl super::Parse for TypeFnRetKindScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.set_newline_as_trivia(false);
        parse_kind_bound(parser)
    }
}

define_scope! { TypeFnBodyScope, TypeFnBody }
impl super::Parse for TypeFnBodyScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::LBrace);
        parser.set_newline_as_trivia(true);

        // The body is exactly one `match` on the subject.
        if parser.find(
            SyntaxKind::MatchKw,
            ExpectedKind::Body(SyntaxKind::TypeFnBody),
        )? {
            parser.parse(TypeFnMatchScope::default())?;
        }

        parser.set_newline_as_trivia(true);
        parser.bump_or_recover(
            SyntaxKind::RBrace,
            "expected `}` to close the recursive type fn body",
        )
    }
}

define_scope! { TypeFnMatchScope, TypeFnMatch }
impl super::Parse for TypeFnMatchScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.set_newline_as_trivia(false);
        parser.bump_expected(SyntaxKind::MatchKw);

        // The subject is a bare identifier (the final `const` parameter). That
        // it names exactly that parameter is checked in HIR.
        if parser.current_kind() == Some(SyntaxKind::Ident) {
            parser.bump();
        } else {
            parser.error("expected the recursive type fn subject (the final `const` parameter)");
        }

        if parser.find(
            SyntaxKind::LBrace,
            ExpectedKind::Body(SyntaxKind::TypeFnMatch),
        )? {
            parser.parse(TypeFnArmListScope::default())?;
        }
        Ok(())
    }
}

define_scope! {
    TypeFnArmListScope,
    TypeFnArmList,
    (SyntaxKind::Newline, SyntaxKind::RBrace, SyntaxKind::Comma)
}
impl super::Parse for TypeFnArmListScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::LBrace);

        loop {
            parser.set_newline_as_trivia(true);
            if matches!(parser.current_kind(), Some(SyntaxKind::RBrace) | None) {
                break;
            }

            parser.parse(TypeFnArmScope::default())?;
            parser.set_newline_as_trivia(false);

            parser.expect(
                &[SyntaxKind::Comma, SyntaxKind::Newline, SyntaxKind::RBrace],
                None,
            )?;
            let comma = parser.bump_if(SyntaxKind::Comma);
            let nl = parser.bump_if(SyntaxKind::Newline);
            if !(comma || nl) {
                break;
            }
        }

        parser.bump_or_recover(SyntaxKind::RBrace, "expected `}` to close the match arms")
    }
}

define_scope! { TypeFnArmScope, TypeFnArm }
impl super::Parse for TypeFnArmScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.set_newline_as_trivia(false);

        parser.parse(TypeFnArmPatScope::default())?;

        parser.set_scope_recovery_stack(&[SyntaxKind::FatArrow]);
        if parser.find_and_pop(SyntaxKind::FatArrow, ExpectedKind::Unspecified)? {
            parser.bump();
        }

        // An arm's right-hand side is a type. `if`, `let` and nested `match`
        // are value forms; name them instead of reporting an unexpected token.
        match parser.current_kind() {
            Some(SyntaxKind::MatchKw) => {
                parser.error("nested `match` is not allowed in a recursive type fn body");
            }
            Some(SyntaxKind::IfKw) => {
                parser.error("`if` is not allowed in a recursive type fn body");
            }
            Some(SyntaxKind::LetKw) => {
                parser.error("`let` is not allowed in a recursive type fn body");
            }
            _ => {}
        }

        parse_type(parser, None)?;
        Ok(())
    }
}

define_scope! { TypeFnArmPatScope, TypeFnArmPat }
impl super::Parse for TypeFnArmPatScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.set_newline_as_trivia(false);
        match parser.current_kind() {
            Some(SyntaxKind::Int) | Some(SyntaxKind::Underscore) => {
                parser.bump();
                Ok(())
            }
            _ => parser.error_and_recover(
                "expected an integer literal or `_` as a recursive type fn arm pattern",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use rowan::ast::AstNode;

    use super::*;
    use crate::{ast, lexer::Lexer, parser::RecoveryMode};

    fn parse(source: &str) -> (ast::ItemList, Vec<crate::ParseError>) {
        let mut parser = Parser::new(Lexer::new(source), RecoveryMode::Recover);
        let _ = parser.parse(super::super::ItemListScope::default());
        let (node, errors) = parser.finish_to_node();
        (ast::ItemList::cast(node).unwrap(), errors)
    }

    #[test]
    fn recursive_type_declaration_retains_name_kind_and_type_arms() {
        let (items, errors) = parse(
            "pub recursive type fn Tree<T, const O: usize, const N: usize>() -> (*) {\n\
             match N {\n0 => Leaf<T>\n_ => Node<Term<O>, Tree<T, O, {N - 1}>>\n}\n}",
        );
        assert!(errors.is_empty(), "{errors:?}");
        let ast::ItemKind::RecursiveTypeFn(def) = items.into_iter().next().unwrap().kind().unwrap()
        else {
            panic!("expected a recursive type declaration")
        };
        assert_eq!(def.name().unwrap().text(), "Tree");
        assert!(def.ret_kind().unwrap().kind_bound().is_some());
        let body = def.body().unwrap().match_().unwrap();
        assert_eq!(body.subject().unwrap().text(), "N");
        let arms: Vec<_> = body.arms().unwrap().into_iter().collect();
        assert_eq!(arms.len(), 2);
        assert_eq!(arms[0].pat().unwrap().int_token().unwrap().text(), "0");
        assert!(arms[1].pat().unwrap().underscore_token().is_some());
        assert!(arms.iter().all(|arm| arm.ty().is_some()));
    }

    #[test]
    fn recursive_type_declaration_accepts_kinds_bounds_and_staged_payload_syntax() {
        for source in [
            "recursive type fn Shape<F, const N: usize>() -> (*) where F: * -> * {\nmatch N {\n0 => Leaf\n_ => F<Shape<F, {N / 2}>>\n}\n}",
            "recursive type fn Plan<const N: usize>() -> (*) {\nmatch N {\n0 => End\n_ => Add<Term<{payload(N - 1)}>, Plan<{N - 1}>>\n}\n}",
            "recursive type fn Flat<const N: usize>() -> * { match N { 0 => bool, _ => Flat<{N - 1}> } }",
            "recursive type fn Lift<const N: usize>() -> * -> *\n{\n    match N {\n        0 => Option\n        _ => Lift<{N - 1}>,\n    }\n}",
            "mod m {\n    pub recursive type fn Tree<const N: usize>() -> (*) {\n        match N {\n            0 => bool\n            _ => Tree<{N - 1}>\n        }\n    }\n}",
        ] {
            let (_, errors) = parse(source);
            assert!(errors.is_empty(), "{source}: {errors:?}");
        }
    }

    #[test]
    fn recursive_type_declaration_rejects_value_bodies_and_parameters() {
        for source in [
            "recursive type fn Bad<const N: usize>(x: usize) -> (*) { match N { 0 => bool, _ => Bad<{N - 1}> } }",
            "recursive type fn Bad<const N: usize>() -> (*) { let x = N }",
            "recursive type fn Bad<const N: usize>() -> (*) { match N { 0 => if true { bool } else { u256 }, _ => Bad<{N - 1}> } }",
            "recursive type fn Bad<const N: usize>() -> (*) { match N { 0 => match N { _ => bool }, _ => Bad<{N - 1}> } }",
            "recursive type fn Bad<const N: usize>() -> (*) { match N { 0 => let, _ => Bad<{N - 1}> } }",
            "recursive type fn Bad<const N: usize>() -> (*) { match N { true => bool, _ => Bad<{N - 1}> } }",
            "recursive type fn Bad<const N: usize>() { match N { 0 => bool, _ => Bad<{N - 1}> } }",
            "recursive type fn Bad<const N: usize> -> (*) { match N { 0 => bool, _ => Bad<{N - 1}> } }",
            "recursive fn Bad<const N: usize>() -> (*) { match N { 0 => bool, _ => Bad<{N - 1}> } }",
            "recursive type Bad<const N: usize>() -> (*) { match N { 0 => bool, _ => Bad<{N - 1}> } }",
            "recursive type fn Bad<const N: usize>() -> (*) { match { 0 => bool, _ => Bad<{N - 1}> } }",
            "recursive type fn Bad<const N: usize>() -> (*) { N }",
            "recursive type fn Bad<const N: usize>() -> (*) { match N { 0 bool, _ => Bad<{N - 1}> } }",
        ] {
            let (_, errors) = parse(source);
            assert!(
                !errors.is_empty(),
                "accepted malformed declaration: {source}"
            );
        }
    }

    #[test]
    fn recursive_is_an_ordinary_name_outside_item_heads() {
        let (_, errors) = parse(
            "fn recursive(recursive: u8) -> u8 {\n    let recursive = recursive\n    recursive\n}\nstruct recursive {}",
        );
        assert!(errors.is_empty(), "{errors:?}");
    }
}
