use super::{
    Checkpoint, ErrProof, Parser, Recovery,
    attr::{self, parse_attr_list, parse_inner_attr_list},
    define_scope,
    expr::parse_expr,
    expr_atom::BlockExprScope,
    func::FuncDefScope,
    param::{
        FuncParamListScope, TraitRefScope, TypeBoundListScope, WhereBracePolicy,
        parse_generic_params_opt, parse_kind_bound, parse_where_clause_opt,
    },
    parse_list,
    path::PathScope,
    struct_::{RecordFieldDefListScope, RecordFieldDefScope},
    token_stream::{LexicalToken, TokenStream},
    type_::{TupleTypeScope, parse_type},
    use_tree::{UsePathScope, UseTreeScope},
};
use crate::{
    ExpectedKind, SyntaxKind,
    parser::{
        func::{FuncScope, UsesClauseScope},
        pat::parse_recv_arm_pat,
    },
};

define_scope! {
    #[doc(hidden)]
    pub ItemListScope {inside_mod: bool},
    ItemList,
    (
        ModKw,
        FnKw,
        StructKw,
        ContractKw,
        EnumKw,
        TraitKw,
        ImplKw,
        UseKw,
        ConstKw,
        StaticAssertKw,
        ExternKw,
        TypeKw,
        PubKw,
        UnsafeKw,
        DocComment,
        Pound
    )
}
impl super::Parse for ItemListScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        use crate::SyntaxKind::*;

        if self.inside_mod {
            parser.bump_expected(LBrace);
            parser.set_scope_recovery_stack(&[RBrace]);
        }

        parse_inner_attr_list(parser)?;

        loop {
            parser.set_newline_as_trivia(true);
            if self.inside_mod && parser.bump_if(RBrace) {
                break;
            }
            if parser.current_kind().is_none() {
                if self.inside_mod {
                    parser.add_error(crate::ParseError::expected(
                        &[RBrace],
                        Some(ExpectedKind::ClosingBracket {
                            bracket: RBrace,
                            parent: Mod,
                        }),
                        parser.current_pos,
                    ));
                }
                break;
            }

            let ok = parser.parse_ok(ItemScope::default())?;
            if parser.current_kind().is_none() || (self.inside_mod && parser.bump_if(RBrace)) {
                break;
            }
            if ok {
                parser.set_newline_as_trivia(false);
                if parser.find(
                    Newline,
                    ExpectedKind::Separator {
                        separator: Newline,
                        element: Item,
                    },
                )? {
                    parser.bump();
                }
            }
        }
        Ok(())
    }
}

define_scope! {
    #[doc(hidden)]
    pub(super) ItemScope,
    Item
}
impl super::Parse for ItemScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        use crate::SyntaxKind::*;

        let mut checkpoint = attr::parse_attr_list(parser)?;
        let modifiers = parse_item_modifiers(parser, &mut checkpoint);

        if modifiers.is_unsafe && !is_fn_item_head(parser) {
            parser.error("expected `fn` after `unsafe` keyword");
        } else if modifiers.is_pub
            && matches!(
                parser.current_kind(),
                Some(ImplKw | ExternKw | StaticAssertKw)
            )
        {
            let error_msg = format!(
                "`pub` can't be used for `{}`",
                parser.current_token().unwrap().text()
            );
            parser.error(&error_msg);
        }

        if parser.is_ident("derive") {
            if modifiers.is_pub || modifiers.is_unsafe {
                parser.error("derive declarations do not support item modifiers");
            }
            parser.parse_cp(DeriveDeclScope::default(), checkpoint)?;
            return Ok(());
        }

        if parser.is_ident("with") {
            if modifiers.is_pub || modifiers.is_unsafe {
                parser.error("derive provider selection scopes do not support item modifiers");
            }
            parser.parse_cp(DeriveProviderScopeScope::default(), checkpoint)?;
            return Ok(());
        }

        // `recursive type fn` is recognized contextually (like `derive`/`with`):
        // `recursive` is not a reserved keyword, so an item head that starts with
        // the identifier `recursive` is a recursive type fn. `pub` is allowed
        // (the visibility rule, spec 1.7); `unsafe` is not.
        if parser.is_ident("recursive") {
            if modifiers.is_unsafe {
                parser.error("`recursive type fn` cannot be `unsafe`");
            }
            parser.parse_cp(RecursiveTypeFnScope::default(), checkpoint)?;
            return Ok(());
        }

        // `actor` is recognized contextually (like `recursive`/`derive`/`with`):
        // it is not a reserved keyword, so no identifier named `actor` breaks. An
        // item head that starts with the identifier `actor` is an actor
        // definition. `pub` is allowed; `unsafe` is not.
        if parser.is_ident("actor") {
            if modifiers.is_unsafe {
                parser.error("`actor` cannot be `unsafe`");
            }
            parser.parse_cp(ActorScope::default(), checkpoint)?;
            return Ok(());
        }

        parser.expect(
            &[
                ModKw,
                FnKw,
                StructKw,
                ContractKw,
                MsgKw,
                EnumKw,
                TraitKw,
                ImplKw,
                UseKw,
                ConstKw,
                StaticAssertKw,
                ExternKw,
                TypeKw,
            ],
            Some(ExpectedKind::Syntax(SyntaxKind::Item)),
        )?;

        match parser.current_kind() {
            Some(ModKw) => parser.parse_cp(ModScope::default(), checkpoint),
            Some(FnKw) => parser.parse_cp(FuncScope::default(), checkpoint),
            Some(StructKw) => parser.parse_cp(super::struct_::StructScope::default(), checkpoint),
            Some(ContractKw) => parser.parse_cp(ContractScope::default(), checkpoint),
            Some(MsgKw) => parser.parse_cp(MsgScope::default(), checkpoint),
            Some(EnumKw) => parser.parse_cp(EnumScope::default(), checkpoint),
            Some(TraitKw) => parser.parse_cp(TraitScope::default(), checkpoint),
            Some(ImplKw) => parser.parse_cp(ImplScope::default(), checkpoint),
            Some(UseKw) => parser.parse_cp(UseScope::default(), checkpoint),
            Some(ConstKw) => {
                if is_fn_item_head(parser) {
                    parser.parse_cp(FuncScope::default(), checkpoint)
                } else {
                    parser.parse_cp(ConstScope::default(), checkpoint)
                }
            }
            Some(StaticAssertKw) => parser.parse_cp(StaticAssertScope::default(), checkpoint),
            Some(ExternKw) => parser.parse_cp(ExternScope::default(), checkpoint),
            Some(TypeKw) => parser.parse_cp(TypeAliasScope::default(), checkpoint),
            _ => unreachable!(),
        }?;

        Ok(())
    }
}

fn is_fn_item_head<S: TokenStream>(parser: &mut Parser<S>) -> bool {
    match parser.current_kind() {
        Some(SyntaxKind::FnKw) => true,
        Some(SyntaxKind::ConstKw) => matches!(
            parser.peek_n_non_trivia(2).as_slice(),
            [SyntaxKind::ConstKw, SyntaxKind::FnKw]
        ),
        _ => false,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ParsedVis {
    #[default]
    Private,
    Public,
    PubIngot,
    PubSuper,
    PubIn,
}

#[derive(Debug, Clone, Copy, Default)]
struct ItemModifiers {
    is_pub: bool,
    is_unsafe: bool,
}

/// Parse a visibility restriction after `pub(`: `ingot)`, `super)`, or `in path)`.
/// The `pub` keyword and `(` have already been bumped. Emits a `VisRestriction`
/// CST node wrapping the restriction tokens and closing `)`.
pub(super) fn parse_vis_restriction<S: TokenStream>(parser: &mut Parser<S>) -> ParsedVis {
    let cp = parser.checkpoint();

    let vis = match parser.current_kind() {
        Some(SyntaxKind::IngotKw) => {
            parser.bump();
            ParsedVis::PubIngot
        }
        Some(SyntaxKind::SuperKw) => {
            parser.bump();
            ParsedVis::PubSuper
        }
        Some(SyntaxKind::InKw) => {
            parser.error_msg_on_current_token(
                "`pub(in path)` is not yet supported; use `pub(ingot)` or `pub(super)`",
            );
            parser.bump();
            // Parse the module path so the CST is well-formed for future use.
            let _ = parser.parse(UsePathScope::default());
            ParsedVis::PubIn
        }
        _ => {
            parser.error_msg_on_current_token("expected `ingot`, `super`, or `in` after `pub(`");
            ParsedVis::Public
        }
    };

    // Expect closing `)`
    if !parser.bump_if(SyntaxKind::RParen) {
        parser.error_msg_on_current_token("expected `)` to close visibility restriction");
    }

    if !parser.is_dry_run() {
        parser
            .builder
            .start_node_at(cp, SyntaxKind::VisRestriction.into());
        parser.builder.finish_node();
    }

    vis
}

fn parse_item_modifiers<S: TokenStream>(
    parser: &mut Parser<S>,
    checkpoint: &mut Option<Checkpoint>,
) -> ItemModifiers {
    let mut modifiers = ItemModifiers::default();

    loop {
        match parser.current_kind() {
            Some(SyntaxKind::PubKw) => {
                if checkpoint.is_none() {
                    *checkpoint = Some(parser.checkpoint());
                }

                if modifiers.is_pub {
                    parser.unexpected_token_error(format!(
                        "duplicate {} modifier",
                        SyntaxKind::PubKw.describe(),
                    ));
                } else if modifiers.is_unsafe {
                    parser
                        .unexpected_token_error("`pub` modifier must come before `unsafe`".into());
                    modifiers.is_pub = true;
                } else {
                    parser.bump();
                    modifiers.is_pub = true;

                    // Check for visibility restriction: pub(ingot), pub(super), pub(in path).
                    // The return value is discarded — parse_vis_restriction's main
                    // job is emitting the VisRestriction CST node. Actual visibility
                    // is determined during HIR lowering from the CST.
                    if parser.current_kind() == Some(SyntaxKind::LParen) {
                        parser.bump(); // (
                        let _ = parse_vis_restriction(parser);
                    }
                }
            }
            Some(SyntaxKind::UnsafeKw) => {
                if checkpoint.is_none() {
                    *checkpoint = Some(parser.checkpoint());
                }

                if modifiers.is_unsafe {
                    parser.unexpected_token_error(format!(
                        "duplicate {} modifier",
                        SyntaxKind::UnsafeKw.describe(),
                    ));
                } else {
                    parser.bump();
                    modifiers.is_unsafe = true;
                }
            }
            _ => break,
        }
    }

    modifiers
}

define_scope! { ModScope, Mod }
impl super::Parse for ModScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::ModKw);

        parser.set_scope_recovery_stack(&[
            SyntaxKind::Ident,
            SyntaxKind::LBrace,
            SyntaxKind::RBrace,
        ]);

        if parser.find_and_pop(SyntaxKind::Ident, ExpectedKind::Name(SyntaxKind::Mod))? {
            parser.bump();
        }
        if parser.find_and_pop(SyntaxKind::LBrace, ExpectedKind::Body(SyntaxKind::Mod))? {
            parser.parse(ItemListScope::new(true))?;
        }
        Ok(())
    }
}

define_scope! { ContractScope, Contract }
impl super::Parse for ContractScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::ContractKw);

        parser.set_scope_recovery_stack(&[
            SyntaxKind::Ident,
            SyntaxKind::UsesKw,
            SyntaxKind::LBrace,
        ]);

        if parser.find_and_pop(SyntaxKind::Ident, ExpectedKind::Name(SyntaxKind::Contract))? {
            parser.bump();
        }

        // Optional `uses` clause after the contract name
        if parser.current_kind() == Some(SyntaxKind::UsesKw) {
            parser.parse(UsesClauseScope::default())?;
        }
        parser.pop_recovery_stack(); // remove `UsesKw` from recovery stack

        if parser.find_and_pop(SyntaxKind::LBrace, ExpectedKind::Body(SyntaxKind::Contract))? {
            parser.bump_expected(SyntaxKind::LBrace);

            parser.parse(ContractFieldsScope::default())?;

            // Optional `init` block (possibly preceded by attributes). If the
            // leading attributes actually belong to the first `recv` block, we
            // must reuse the same checkpoint instead of dropping it here.
            let mut checkpoint = parse_attr_list(parser)?;
            if parser.is_ident("init") {
                parser.parse_cp(ContractInitScope::default(), checkpoint)?;
                checkpoint = parse_attr_list(parser)?;
            }

            // Zero or more `recv` blocks (each possibly preceded by attributes)
            loop {
                if parser.is_ident("recv") {
                    parser.parse_cp(ContractRecvScope::default(), checkpoint)?;
                    checkpoint = parse_attr_list(parser)?;
                } else {
                    break;
                }
            }

            // If trailing attributes were consumed but no init/recv follows,
            // emit a parse error so they don't silently vanish.
            if checkpoint.is_some() {
                let _ =
                    parser.error_msg_on_current_token("expected `init` or `recv` after attribute");
            }

            parser.bump_or_recover(
                SyntaxKind::RBrace,
                "expected `}` to close the contract body",
            )?;
        }
        Ok(())
    }
}

// Parses the leading contract fields inside the contract body.
// Comma separators are optional; items can be delimited by commas or newlines.
define_scope! { ContractFieldsScope, SyntaxKind::ContractFields }
impl super::Parse for ContractFieldsScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        // Keep consuming field definitions while they parse cleanly.
        // Stop when we reach `init`, `recv`, or `}`.
        loop {
            // Stop conditions
            match parser.current_kind() {
                Some(SyntaxKind::RBrace) | None => break,
                Some(SyntaxKind::Pound) | Some(SyntaxKind::DocComment) => {
                    // Lookahead: break only if the attribute/doc comment is followed by
                    // `init` or `recv`, otherwise it's a field attribute
                    // (e.g. `#[field_attr] total: u256`).
                    let is_init_or_recv_attr = parser.dry_run(|p| {
                        let _ = attr::parse_attr_list(p);
                        p.is_ident("init") || p.is_ident("recv")
                    });
                    if is_init_or_recv_attr {
                        break;
                    }
                }
                Some(SyntaxKind::Ident)
                    if matches!(parser.current_token().unwrap().text(), "init" | "recv") =>
                {
                    break;
                }
                _ => {}
            }

            parser.parse(RecordFieldDefScope::default())?;

            // Optional comma between fields
            let _ = parser.bump_if(SyntaxKind::Comma);
        }
        Ok(())
    }
}

// Parses the `init` block within a contract.
define_scope! { ContractInitScope, SyntaxKind::ContractInit }
impl super::Parse for ContractInitScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        debug_assert!(parser.is_ident("init"));
        // bump `init`
        parser.bump();

        // Parameter list
        if parser.current_kind() == Some(SyntaxKind::LParen) {
            parser.parse(FuncParamListScope::new(false))?;
        }

        // Optional `uses` clause
        let nt = parser.set_newline_as_trivia(true);
        if parser.current_kind() == Some(SyntaxKind::UsesKw) {
            parser.parse(UsesClauseScope::default())?;
        }
        parser.set_newline_as_trivia(nt);

        // Body block
        if parser.current_kind() == Some(SyntaxKind::LBrace) {
            parser.parse(BlockExprScope::default())?;
        }
        Ok(())
    }
}

// Parses a `recv` block within a contract, in either form:
// - `recv Type { ... }`
// - `recv { ... }`
define_scope! { ContractRecvScope, SyntaxKind::ContractRecv }
impl super::Parse for ContractRecvScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        debug_assert!(parser.is_ident("recv"));
        parser.bump();

        // Optional message root path before the block
        if parser.current_kind() != Some(SyntaxKind::LBrace) {
            parser.or_recover(|p| p.parse(PathScope::default()))?;
        }

        if parser.current_kind() == Some(SyntaxKind::LBrace) {
            parser.parse(RecvArmListScope::default())?;
        }
        Ok(())
    }
}

define_scope! { RecvArmListScope, RecvArmList }
impl super::Parse for RecvArmListScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::LBrace);
        while parser.current_kind() != Some(SyntaxKind::RBrace) && parser.current_kind().is_some() {
            parser.parse(RecvArmScope::default())?;
        }
        parser.bump_or_recover(SyntaxKind::RBrace, "expected `}` to close recv block")?;
        Ok(())
    }
}

// Parses: `Pattern -> RetTy uses (...) { body }`
define_scope! { RecvArmScope, RecvArm }
impl super::Parse for RecvArmScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parse_attr_list(parser)?;

        parser.set_newline_as_trivia(false);

        parse_recv_arm_pat(parser)?;

        parser.set_newline_as_trivia(true);

        // Optional return type
        if parser.bump_if(SyntaxKind::Arrow) {
            parse_type(parser, None)?;
        }

        // Optional uses clause
        if parser.current_kind() == Some(SyntaxKind::UsesKw) {
            parser.parse(UsesClauseScope::default())?;
        }

        // Body block
        if parser.current_kind() == Some(SyntaxKind::LBrace) {
            parser.parse(BlockExprScope::default())?;
        }

        Ok(())
    }
}
// Parses an `actor` definition:
//
//   actor Name uses (<row>) {
//       <field>*
//       <fn behavior>*
//   }
//
// The body admits record fields (the actor's state) and `fn` behaviors. In HIR
// lowering the whole item is desugared to a plain struct plus flattened free
// functions (`crates/hir/src/core/lower/actor.rs`); nothing about `actor`
// survives into name resolution or type checking, so the construct is pure
// sugar. v1 keeps the body deliberately small: fields plus behaviors, no `init`
// or `recv` compartments yet.
define_scope! { ActorScope, Actor }
impl super::Parse for ActorScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        debug_assert!(parser.is_ident("actor"));
        parser.bump(); // contextual `actor`

        parser.set_scope_recovery_stack(&[
            SyntaxKind::Ident,
            SyntaxKind::UsesKw,
            SyntaxKind::LBrace,
        ]);

        if parser.find_and_pop(SyntaxKind::Ident, ExpectedKind::Name(SyntaxKind::Actor))? {
            parser.bump();
        }

        // Optional placement `uses` clause after the actor name.
        if parser.current_kind() == Some(SyntaxKind::UsesKw) {
            parser.parse(UsesClauseScope::default())?;
        }
        parser.pop_recovery_stack(); // remove `UsesKw` from recovery stack

        if parser.find_and_pop(SyntaxKind::LBrace, ExpectedKind::Body(SyntaxKind::Actor))? {
            parser.bump_expected(SyntaxKind::LBrace);

            loop {
                parser.set_newline_as_trivia(true);
                match parser.current_kind() {
                    Some(SyntaxKind::RBrace) | None => break,
                    // A behavior with no leading attributes/doc. Behaviors parse
                    // like impl methods so a `self` receiver is admitted; the
                    // desugar flattens it away. A `const fn` behavior (the
                    // reserved `view()` surface projection) is admitted too:
                    // `is_fn_item_head` recognizes the `const fn` head and
                    // `FuncScope` consumes the `const` modifier.
                    Some(SyntaxKind::FnKw) => {
                        parser.parse(FuncScope::new(FuncDefScope::Impl))?;
                    }
                    Some(SyntaxKind::ConstKw) if is_fn_item_head(parser) => {
                        parser.parse(FuncScope::new(FuncDefScope::Impl))?;
                    }
                    // Leading attributes or a doc comment: they belong to a
                    // behavior `fn` (including `const fn`) when one follows,
                    // otherwise to a field.
                    Some(SyntaxKind::Pound) | Some(SyntaxKind::DocComment) => {
                        let precedes_fn = parser.dry_run(|p| {
                            let _ = attr::parse_attr_list(p);
                            is_fn_item_head(p)
                        });
                        if precedes_fn {
                            let checkpoint = parse_attr_list(parser)?;
                            parser.parse_cp(FuncScope::new(FuncDefScope::Impl), checkpoint)?;
                        } else {
                            parser.parse(RecordFieldDefScope::default())?;
                            let _ = parser.bump_if(SyntaxKind::Comma);
                        }
                    }
                    // Otherwise a state field.
                    _ => {
                        parser.parse(RecordFieldDefScope::default())?;
                        let _ = parser.bump_if(SyntaxKind::Comma);
                    }
                }
            }

            parser.bump_or_recover(SyntaxKind::RBrace, "expected `}` to close the actor body")?;
        }
        Ok(())
    }
}

define_scope! { MsgScope, Msg }
impl super::Parse for MsgScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::MsgKw);

        parser.set_scope_recovery_stack(&[SyntaxKind::Ident, SyntaxKind::LBrace]);

        if parser.find_and_pop(SyntaxKind::Ident, ExpectedKind::Name(SyntaxKind::Msg))? {
            parser.bump();
        }
        if parser.find_and_pop(SyntaxKind::LBrace, ExpectedKind::Body(SyntaxKind::Msg))? {
            parser.parse(MsgVariantListScope::default())?;
        }
        Ok(())
    }
}

define_scope! { MsgVariantListScope, MsgVariantList, (Comma, RBrace) }
impl super::Parse for MsgVariantListScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parse_list(
            parser,
            true,
            SyntaxKind::MsgVariantList,
            (SyntaxKind::LBrace, SyntaxKind::RBrace),
            |parser| parser.parse(MsgVariantScope::default()),
        )
    }
}

define_scope! { MsgVariantScope, MsgVariant }
impl super::Parse for MsgVariantScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.set_newline_as_trivia(false);

        // Parse attribute list
        parse_attr_list(parser)?;

        // Parse variant name
        parser.bump_or_recover(SyntaxKind::Ident, "expected identifier for message variant")?;

        // Parse optional parameters
        if parser.current_kind() == Some(SyntaxKind::LBrace) {
            parser.parse(MsgVariantParamsScope::default())?;
        }

        // Parse optional return type
        if parser.bump_if(SyntaxKind::Arrow) {
            parse_type(parser, None)?;
        }

        Ok(())
    }
}

define_scope! { MsgVariantParamsScope, MsgVariantParams, (Comma, RBrace) }
impl super::Parse for MsgVariantParamsScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parse_list(
            parser,
            true,
            SyntaxKind::MsgVariantParams,
            (SyntaxKind::LBrace, SyntaxKind::RBrace),
            |parser| parser.parse(RecordFieldDefScope::default()),
        )
    }
}

define_scope! { EnumScope, Enum }
impl super::Parse for EnumScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::EnumKw);

        parser.set_scope_recovery_stack(&[
            SyntaxKind::Ident,
            SyntaxKind::Lt,
            SyntaxKind::WhereKw,
            SyntaxKind::LBrace,
        ]);

        if parser.find_and_pop(SyntaxKind::Ident, ExpectedKind::Name(SyntaxKind::Enum))? {
            parser.bump();
        }

        parser.pop_recovery_stack();
        parse_generic_params_opt(parser, false)?;

        parser.pop_recovery_stack();
        parse_where_clause_opt(parser, WhereBracePolicy::Lookahead)?;

        if parser.find_and_pop(SyntaxKind::LBrace, ExpectedKind::Body(SyntaxKind::Enum))? {
            parser.parse(VariantDefListScope::default())?;
        }
        Ok(())
    }
}

define_scope! { VariantDefListScope, VariantDefList, (Comma, RBrace) }
impl super::Parse for VariantDefListScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parse_list(
            parser,
            true,
            SyntaxKind::VariantDefList,
            (SyntaxKind::LBrace, SyntaxKind::RBrace),
            |parser| parser.parse(VariantDefScope::default()),
        )
    }
}

define_scope! { VariantDefScope, VariantDef }
impl super::Parse for VariantDefScope {
    type Error = Recovery<ErrProof>;
    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parse_attr_list(parser)?;
        parser.bump_or_recover(SyntaxKind::Ident, "expected ident for the variant name")?;

        if parser.current_kind() == Some(SyntaxKind::LParen) {
            parser.parse(TupleTypeScope::default())?;
        } else if parser.current_kind() == Some(SyntaxKind::LBrace) {
            parser.parse(RecordFieldDefListScope::default())?;
        }
        Ok(())
    }
}

define_scope! { TraitScope, Trait }
impl super::Parse for TraitScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::TraitKw);
        parser.set_scope_recovery_stack(&[
            SyntaxKind::Ident,
            SyntaxKind::Lt,
            SyntaxKind::Colon,
            SyntaxKind::WhereKw,
            SyntaxKind::LBrace,
        ]);
        if parser.find_and_pop(SyntaxKind::Ident, ExpectedKind::Name(SyntaxKind::Trait))? {
            parser.bump();
        }

        parser.expect_and_pop_recovery_stack()?;
        parse_generic_params_opt(parser, false)?;

        parser.expect_and_pop_recovery_stack()?;
        if parser.current_kind() == Some(SyntaxKind::Colon) {
            parser.parse(SuperTraitListScope::default())?;
        }

        parser.expect_and_pop_recovery_stack()?;
        parse_where_clause_opt(parser, WhereBracePolicy::Lookahead)?;

        if parser.find(SyntaxKind::LBrace, ExpectedKind::Body(SyntaxKind::Trait))? {
            parser.parse(TraitItemListScope::default())?;
        }
        Ok(())
    }
}

define_scope! {SuperTraitListScope, SuperTraitList, (Plus)}
impl super::Parse for SuperTraitListScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::Colon);
        loop {
            parser.parse_or_recover(TraitRefScope::default())?;
            if !parser.bump_if(SyntaxKind::Plus) {
                break;
            }
        }
        Ok(())
    }
}

define_scope! { TraitItemListScope, TraitItemList, (RBrace, Newline, FnKw, TypeKw, ConstKw) }
impl super::Parse for TraitItemListScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parse_trait_item_block(parser, FuncDefScope::TraitDef)
    }
}

define_scope! { TraitTypeItemScope, TraitTypeItem }
impl super::Parse for TraitTypeItemScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.set_newline_as_trivia(false);
        parser.bump_expected(SyntaxKind::TypeKw);

        parser.set_scope_recovery_stack(&[SyntaxKind::Ident, SyntaxKind::Eq]);
        if parser.find_and_pop(
            SyntaxKind::Ident,
            ExpectedKind::Name(SyntaxKind::TraitTypeItem),
        )? {
            parser.bump();
        }

        parse_generic_params_opt(parser, false)?;

        if parser.current_kind() == Some(SyntaxKind::Colon) {
            parser.parse(TypeBoundListScope::new(false))?;
        }

        if parser.current_kind() == Some(SyntaxKind::Eq) {
            parser.bump();
            parse_type(parser, None)?;
        }

        Ok(())
    }
}

define_scope! { TraitConstItemScope, TraitConstItem }
impl super::Parse for TraitConstItemScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.set_newline_as_trivia(false);
        parser.bump_expected(SyntaxKind::ConstKw);

        parser.set_scope_recovery_stack(&[SyntaxKind::Ident, SyntaxKind::Colon, SyntaxKind::Eq]);

        if parser.find_and_pop(
            SyntaxKind::Ident,
            ExpectedKind::Name(SyntaxKind::TraitConstItem),
        )? {
            parser.bump();
        }

        if parser.find_and_pop(
            SyntaxKind::Colon,
            ExpectedKind::TypeSpecifier(SyntaxKind::TraitConstItem),
        )? {
            parser.bump();
            parse_type(parser, None)?;
        }

        parser.set_newline_as_trivia(true);
        if parser.bump_if(SyntaxKind::Eq) {
            parse_expr(parser)?;
        }
        Ok(())
    }
}

define_scope! { ImplScope, Impl, (ForKw, LBrace) }
impl super::Parse for ImplScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::ImplKw);

        parse_generic_params_opt(parser, false)?;

        let is_impl_trait = parser.dry_run(|parser| {
            parser.parse(TraitRefScope::default()).is_ok()
                && parser
                    .find(SyntaxKind::ForKw, ExpectedKind::Unspecified)
                    .is_ok_and(|x| x)
        });

        if is_impl_trait {
            self.set_kind(SyntaxKind::ImplTrait);
            parser.set_scope_recovery_stack(&[
                SyntaxKind::ForKw,
                SyntaxKind::WhereKw,
                SyntaxKind::LBrace,
            ]);

            parser.parse_or_recover(TraitRefScope::default())?;
            if parser.find_and_pop(SyntaxKind::ForKw, ExpectedKind::Unspecified)? {
                parser.bump();
            }
        } else {
            parser.set_scope_recovery_stack(&[SyntaxKind::WhereKw, SyntaxKind::LBrace]);
        }

        parse_type(parser, None)?;

        // Optional trailing `as Name` alias on a trait impl (FCO T-Nway).
        // Only the `for`-bearing trait-impl form may be aliased; an inherent
        // `impl Type {}` has no trait to select among. Consumed here — between
        // the for-type and the where-clause/body — so the remaining recovery
        // stack (`where`/`{`) is left intact. When absent, parsing is
        // byte-identical to before.
        if is_impl_trait && parser.current_kind() == Some(SyntaxKind::AsKw) {
            parser.parse(ImplTraitAliasScope::default())?;
        }

        // Optional trailing `with <path>` permit clause on a trait impl (FCO
        // T3). `with` is a contextual identifier (not a reserved keyword), so
        // it is recognized via `is_ident` exactly like the `with` derive-
        // provider scope. The path references a permit value; it is parsed and
        // stored unresolved (no name resolution / no selection at this
        // increment). Consumed in the SAME slot as `as Name`, after any alias
        // and before the where-clause/body, so the remaining recovery stack
        // (`where`/`{`) is left intact. Both clauses may appear in the order
        // `as Name with a` (the `as` arm above runs first); supplying `with a`
        // alone is the common form. When absent, parsing is byte-identical.
        if is_impl_trait && parser.is_ident("with") {
            parser.parse(ImplTraitWithScope::default())?;
        }

        parser.expect_and_pop_recovery_stack()?;
        parse_where_clause_opt(parser, WhereBracePolicy::Lookahead)?;

        if parser.find_and_pop(
            SyntaxKind::LBrace,
            ExpectedKind::Body(SyntaxKind::ImplTrait),
        )? {
            if is_impl_trait {
                parser.parse(ImplTraitItemListScope::default())?;
            } else {
                parser.parse(ImplItemListScope::default())?;
            }
        }
        Ok(())
    }
}

define_scope! { ImplTraitAliasScope, ImplTraitAlias }
impl super::Parse for ImplTraitAliasScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.set_newline_as_trivia(false);
        parser.bump_expected(SyntaxKind::AsKw);
        parser.set_scope_recovery_stack(&[SyntaxKind::Ident]);
        if parser.find_and_pop(SyntaxKind::Ident, ExpectedKind::Name(SyntaxKind::ImplTrait))? {
            parser.bump();
        }
        Ok(())
    }
}

define_scope! { ImplTraitWithScope, ImplTraitWith }
impl super::Parse for ImplTraitWithScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        debug_assert!(parser.is_ident("with"));
        parser.set_newline_as_trivia(false);
        // `with` is a contextual identifier; consume it as the clause head.
        parser.bump();
        // The permit path is parsed but NOT resolved at this increment.
        parser.parse_or_recover(PathScope::default())?;
        Ok(())
    }
}

define_scope! { ImplTraitItemListScope, TraitItemList, (RBrace, FnKw, TypeKw, ConstKw) }
impl super::Parse for ImplTraitItemListScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parse_trait_item_block(parser, FuncDefScope::Impl)
    }
}

define_scope! { ImplItemListScope, ImplItemList, (RBrace, ConstKw, FnKw) }
impl super::Parse for ImplItemListScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parse_fn_item_block(parser, FuncDefScope::Impl, true)
    }
}

define_scope! { UseScope, Use }
impl super::Parse for UseScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::UseKw);
        parser.parse(UseTreeScope::default())
    }
}

define_scope! { ConstScope, Const }
impl super::Parse for ConstScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parse_attr_list(parser)?;

        parser.bump_expected(SyntaxKind::ConstKw);
        parser.set_newline_as_trivia(false);
        parser.set_scope_recovery_stack(&[SyntaxKind::Ident, SyntaxKind::Colon, SyntaxKind::Eq]);

        if parser.find_and_pop(SyntaxKind::Ident, ExpectedKind::Name(SyntaxKind::Const))? {
            parser.bump();
        }
        if parser.find_and_pop(
            SyntaxKind::Colon,
            ExpectedKind::TypeSpecifier(SyntaxKind::Const),
        )? {
            parser.bump();
            parse_type(parser, None)?;
        }

        parser.set_newline_as_trivia(true);
        if parser.find_and_pop(SyntaxKind::Eq, ExpectedKind::Unspecified)? {
            parser.bump();
            parse_expr(parser)?;
        }
        Ok(())
    }
}

define_scope! { DeriveDeclScope, DeriveDecl }
impl super::Parse for DeriveDeclScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        debug_assert!(parser.is_ident("derive"));
        parser.bump();
        parser.set_newline_as_trivia(false);
        parser.set_scope_recovery_stack(&[SyntaxKind::ForKw, SyntaxKind::Ident]);

        parser.parse_or_recover(PathScope::default())?;

        if parser.find_and_pop(SyntaxKind::ForKw, ExpectedKind::Unspecified)? {
            parser.bump();
            parser.parse_or_recover(PathScope::default())?;
        }

        if parser.is_ident("using") {
            parser.bump();
            parser.parse_or_recover(PathScope::default())?;
        }

        Ok(())
    }
}

define_scope! { DeriveProviderScopeScope, DeriveProviderScope }
impl super::Parse for DeriveProviderScopeScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        debug_assert!(parser.is_ident("with"));
        parser.bump();
        parser.set_newline_as_trivia(false);
        parser.set_scope_recovery_stack(&[SyntaxKind::LBrace, SyntaxKind::RBrace]);

        parser.parse_or_recover(PathScope::default())?;
        parser.parse(ItemListScope::new(true))?;

        Ok(())
    }
}

define_scope! { StaticAssertScope, StaticAssert }
impl super::Parse for StaticAssertScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::StaticAssertKw);
        parser.set_newline_as_trivia(true);
        parser.set_scope_recovery_stack(&[SyntaxKind::LParen, SyntaxKind::RParen]);

        if parser.find_and_pop(
            SyntaxKind::LParen,
            ExpectedKind::Syntax(SyntaxKind::StaticAssert),
        )? {
            parser.bump();
            if parser.current_kind() != Some(SyntaxKind::RParen) {
                parse_expr(parser)?;
            }
        }

        if parser.find_and_pop(
            SyntaxKind::RParen,
            ExpectedKind::ClosingBracket {
                bracket: SyntaxKind::RParen,
                parent: SyntaxKind::StaticAssert,
            },
        )? {
            parser.bump();
        }
        Ok(())
    }
}

define_scope! { ExternScope, Extern }
impl super::Parse for ExternScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.bump_expected(SyntaxKind::ExternKw);

        parser.set_scope_recovery_stack(&[SyntaxKind::LBrace]);
        if parser.find(SyntaxKind::LBrace, ExpectedKind::Body(SyntaxKind::Extern))? {
            parser.parse(ExternItemListScope::default())?;
        }
        Ok(())
    }
}

define_scope! { ExternItemListScope, ExternItemList, (PubKw, UnsafeKw, ConstKw, FnKw) }
impl super::Parse for ExternItemListScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parse_fn_item_block(parser, FuncDefScope::Extern, false)
    }
}

define_scope! { TypeAliasScope, TypeAlias }
impl super::Parse for TypeAliasScope {
    type Error = Recovery<ErrProof>;

    fn parse<S: TokenStream>(&mut self, parser: &mut Parser<S>) -> Result<(), Self::Error> {
        parser.set_newline_as_trivia(false);
        parser.bump_expected(SyntaxKind::TypeKw);

        parser.set_scope_recovery_stack(&[SyntaxKind::Ident, SyntaxKind::Lt, SyntaxKind::Eq]);
        if parser.find_and_pop(SyntaxKind::Ident, ExpectedKind::Name(SyntaxKind::TypeAlias))? {
            parser.bump();
        }

        parser.pop_recovery_stack();
        parse_generic_params_opt(parser, true)?;

        if parser.find_and_pop(SyntaxKind::Eq, ExpectedKind::Unspecified)? {
            parser.bump();
            parser.bump_continuation_newlines();
            parse_type(parser, None)?;
        }
        Ok(())
    }
}

// `recursive type fn Name<Params.., const N: usize>() -> (KIND) where .. {
//     match N { LIT => TYPE .. _ => TYPE }
// }`
//
// The hand-written parser is authoritative; the tree-sitter grammar is a
// deferred follow-up (like const-predicates). This scope parses the item shape
// and enforces the grammar-level restrictions with named diagnostics (empty
// value-parameter list, integer/`_` arm patterns, mandatory `=>`, a single
// `match` body, no `if`/`let`/nested-`match` at an arm head). The deeper
// definition-time laws (exactly one `const` subject declared last, self-call
// only by DefId, the `{N - k}`/`{N / k}` subject-shape whitelist, exhaustiveness
// and termination) are checked in HIR well-formedness, mirroring how
// `ConstGenericParam` trait bounds are parsed permissively and "checked in hir".
define_scope! { RecursiveTypeFnScope, RecursiveTypeFn }
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

        // The value-parameter list is mandatory and must be empty (reserved for
        // future value-level inputs, spec 1.1 rule 3).
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

        parser.expect_and_pop_recovery_stack()?;
        if parser.bump_if(SyntaxKind::Arrow) {
            parser.parse(TypeFnRetKindScope::default())?;
        }

        // The `where` clause and body may begin on a following line.
        parser.set_newline_as_trivia(true);
        parser.expect_and_pop_recovery_stack()?;
        parse_where_clause_opt(parser, WhereBracePolicy::Lookahead)?;

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

        // The body is exactly one `match` on the subject (spec 1.1 rule 2).
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

        // The subject is a bare identifier (the `const` parameter). That it
        // names exactly the declared subject is verified in HIR.
        if parser.current_kind() == Some(SyntaxKind::Ident) {
            parser.bump();
        } else {
            parser.error("expected the recursive type fn subject (the `const` parameter)");
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

        // The arm right-hand side is a type. Partial constructs are not
        // expressible, but `if`/`let`/`match` heads are rejected with named
        // diagnostics so the "no if/let/nested match" rule is not silently
        // accepted (spec 1.1 rule 2).
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
            Some(SyntaxKind::Int) => {
                parser.bump();
                Ok(())
            }
            Some(SyntaxKind::Underscore) => {
                parser.bump();
                Ok(())
            }
            _ => parser.error_and_recover(
                "expected an integer literal or `_` recursive type fn arm pattern",
            ),
        }
    }
}

/// This function is used to parse items in `impl` and `extern` blocks,
/// which only allow `fn` definitions (and, in `impl` blocks, associated
/// `const` items).
fn parse_fn_item_block<S: TokenStream>(
    parser: &mut Parser<S>,
    fn_def_scope: FuncDefScope,
    allow_consts: bool,
) -> Result<(), Recovery<ErrProof>> {
    parser.bump_expected(SyntaxKind::LBrace);
    loop {
        parser.set_newline_as_trivia(true);
        if matches!(parser.current_kind(), Some(SyntaxKind::RBrace) | None) {
            break;
        }

        let mut checkpoint = attr::parse_attr_list(parser)?;
        let modifiers = parse_item_modifiers(parser, &mut checkpoint);

        let is_fn_head = is_fn_item_head(parser);

        if modifiers.is_unsafe && !is_fn_head {
            parser.error("expected `fn` after `unsafe` keyword");
        }

        if is_fn_head {
            parser.parse_cp(FuncScope::new(fn_def_scope), checkpoint)?;
        } else if allow_consts && parser.current_kind() == Some(SyntaxKind::ConstKw) {
            parser.parse_cp(TraitConstItemScope::default(), checkpoint)?;
        } else {
            let proof = if allow_consts {
                parser.error("only `fn` or `const` is allowed in this block")
            } else {
                parser.error("only `fn` is allowed in this block")
            };
            if parser.current_kind() == Some(SyntaxKind::ConstKw) {
                parser.bump();
            }
            parser.try_recover().map_err(|r| r.add_err_proof(proof))?;
        }
    }

    parser.bump_or_recover(SyntaxKind::RBrace, "expected `}` to close the block")
}

fn parse_trait_item_block<S: TokenStream>(
    parser: &mut Parser<S>,
    fn_def_scope: FuncDefScope,
) -> Result<(), Recovery<ErrProof>> {
    parser.bump_expected(SyntaxKind::LBrace);
    loop {
        parser.set_newline_as_trivia(true);
        if matches!(parser.current_kind(), Some(SyntaxKind::RBrace) | None) {
            break;
        }

        let checkpoint = attr::parse_attr_list(parser)?;

        while parser.current_kind().is_some_and(|k| k.is_modifier_head()) {
            let kind = parser.current_kind().unwrap();
            parser.unexpected_token_error(format!(
                "{} modifier is not allowed in this block",
                kind.describe()
            ));
        }

        match parser.current_kind() {
            Some(SyntaxKind::FnKw) => {
                parser.parse_cp(FuncScope::new(fn_def_scope), checkpoint)?;
            }
            Some(SyntaxKind::TypeKw) => {
                parser.parse_cp(TraitTypeItemScope::default(), checkpoint)?;
            }
            Some(SyntaxKind::ConstKw) if is_fn_item_head(parser) => {
                parser.parse_cp(FuncScope::new(fn_def_scope), checkpoint)?;
            }
            Some(SyntaxKind::ConstKw) => {
                parser.parse_cp(TraitConstItemScope::default(), checkpoint)?;
            }
            _ => {
                let proof = parser.error_msg_on_current_token(
                    "only `fn`, `type`, or `const` is allowed in this block",
                );
                parser.try_recover().map_err(|r| r.add_err_proof(proof))?;
            }
        }
    }

    parser.bump_or_recover(SyntaxKind::RBrace, "expected `}` to close the block")
}
