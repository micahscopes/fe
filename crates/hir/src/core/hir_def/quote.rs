//! Quotation: Fe syntax held as a compile-time value.
//!
//! A `quote { .. }` in a provider body lowers to [`Quote`]. The quoted syntax
//! lives in its own template [`Body`], so passes over the provider's arena
//! never see unchecked quoted nodes. Each `${e}` becomes a numbered slot in
//! the template and an ordinary expression `e` in the provider's arena, which
//! the provider's checker, borrow checker and evaluator handle as any other
//! expression. Quoted items are kept as headers over HIR ids; they never
//! become module items.

use super::{
    AttrListId, Body, EffectParamListId, ExprId, FuncModifiers, FuncParamListId,
    GenericParamListId, IdentId, Partial, TraitRefId, TypeId, Visibility, WherePredicate,
};

/// `quote(open..) { .. }` in its owner body.
#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct Quote<'db> {
    /// Names the fragment's destination binds, in source order.
    pub open: Vec<IdentId<'db>>,
    /// The quoted syntax. Its arena holds only template nodes.
    pub template: Body<'db>,
    pub fragment: QuoteFragment<'db>,
    /// Items declared in the template's blocks (in an expression quote or in a
    /// quoted function body), with the block that declares each.
    pub block_items: Vec<QuoteBlockItem<'db>>,
    /// The splices, in template order; slot `n` is `holes[n]`.
    pub holes: Vec<QuoteHole>,
}

impl<'db> Quote<'db> {
    /// The kind the syntax decides, or `None` for a body made only of
    /// splices, whose kind is the kind of its splices.
    pub fn syntactic_kind(&self, db: &'db dyn crate::HirDb) -> Option<QuoteKind> {
        match &self.fragment {
            QuoteFragment::Arms(_) => Some(QuoteKind::Arm),
            QuoteFragment::Items(_) => Some(QuoteKind::Item),
            QuoteFragment::Where(_) => Some(QuoteKind::Where),
            QuoteFragment::Expr => (self.splice_only_body(db).is_none()).then_some(QuoteKind::Expr),
        }
    }

    /// For `quote { ${a} ${b} }`, the slots in order.
    pub fn splice_only_body(&self, db: &'db dyn crate::HirDb) -> Option<Vec<u32>> {
        if !matches!(self.fragment, QuoteFragment::Expr) || !self.block_items.is_empty() {
            return None;
        }
        let template = self.template;
        let super::Partial::Present(super::Expr::Block(stmts)) =
            &template.exprs(db)[template.expr(db)]
        else {
            return None;
        };
        if stmts.is_empty() {
            return None;
        }
        stmts
            .iter()
            .map(|stmt| match &template.stmts(db)[*stmt] {
                super::Partial::Present(super::Stmt::Expr(expr)) => {
                    match &template.exprs(db)[*expr] {
                        super::Partial::Present(super::Expr::Hole(slot)) => Some(*slot),
                        _ => None,
                    }
                }
                _ => None,
            })
            .collect()
    }
}

/// Fe syntax that a quote template does not support yet. The quote checker
/// reports each at its own node, so generation never meets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum QuoteUnsupportedForm {
    ArrayRepeat,
    ArrayType,
    ConstGenericArgument,
    Constant,
    ConstantPattern,
    InnerPathGenericArguments,
    LetCondition,
    MethodGenericArguments,
    With,
}

impl std::fmt::Display for QuoteUnsupportedForm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ArrayRepeat => "an array repeat expression `[value; length]`",
            Self::ArrayType => "an array type",
            Self::ConstGenericArgument => "a const generic argument",
            Self::Constant => "a named constant",
            Self::ConstantPattern => "a named constant in a pattern",
            Self::InnerPathGenericArguments => {
                "a value path with generic arguments before its last segment"
            }
            Self::LetCondition => "`if let` or `while let`",
            Self::MethodGenericArguments => "generic arguments on a method call",
            Self::With => "a `with` expression",
        })
    }
}

/// What a fragment is. It is part of the value's type, `Code<K>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum QuoteKind {
    Pat,
    Arm,
    Expr,
    Item,
    Where,
}

impl QuoteKind {
    pub const ALL: [Self; 5] = [Self::Pat, Self::Arm, Self::Expr, Self::Item, Self::Where];

    pub fn meta_item(self) -> crate::core::meta::MetaItem {
        use crate::core::meta::MetaItem;
        match self {
            Self::Pat => MetaItem::Pat,
            Self::Arm => MetaItem::Arm,
            Self::Expr => MetaItem::Expr,
            Self::Item => MetaItem::Item,
            Self::Where => MetaItem::Where,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum QuoteFragment<'db> {
    /// The template's root expression.
    Expr,
    Arms(Vec<super::MatchArm>),
    Items(Vec<QuoteItem<'db>>),
    Where(QuoteWhereClause<'db>),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct QuoteBlockItem<'db> {
    pub block: ExprId,
    pub item: QuoteItem<'db>,
}

/// A splice. `expr` is an ordinary expression of the quote's owner body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub struct QuoteHole {
    pub position: QuoteHolePosition,
    pub expr: ExprId,
}

/// Where a splice stands, which decides the values it accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum QuoteHolePosition {
    /// A code splice in the position corresponding to its fragment kind.
    Code(QuoteKind),
    /// `x.${f}`.
    Member,
    /// `Record { ${f}: value }`.
    FieldLabel,
    /// `${t}` as a type.
    Type,
    /// `fn ${n}`, `const ${n}`.
    Name,
}

/// A declared name, spelled or spliced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum QuoteName<'db> {
    Ident(Partial<IdentId<'db>>),
    Hole(u32),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum QuoteItem<'db> {
    Func(QuoteFunc<'db>),
    Const(QuoteConst<'db>),
    /// `type Name = T` in a quoted trait impl.
    Type(QuoteAssocType<'db>),
    Impl(QuoteImpl<'db>),
    /// `${items}`.
    Hole(u32),
}

/// The complete header of a quoted function and its body.
#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct QuoteFunc<'db> {
    pub name: QuoteName<'db>,
    pub attrs: AttrListId<'db>,
    pub generic_params: GenericParamListId<'db>,
    pub where_clause: QuoteWhereClause<'db>,
    pub params: Partial<FuncParamListId<'db>>,
    pub effects: EffectParamListId<'db>,
    pub ret_ty: Option<TypeId<'db>>,
    pub modifiers: FuncModifiers,
    pub body: Option<ExprId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct QuoteConst<'db> {
    pub attrs: AttrListId<'db>,
    pub name: QuoteName<'db>,
    pub ty: Partial<TypeId<'db>>,
    pub value: Option<ExprId>,
    pub vis: Visibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct QuoteAssocType<'db> {
    pub name: Partial<IdentId<'db>>,
    pub ty: Option<TypeId<'db>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct QuoteImpl<'db> {
    pub attrs: AttrListId<'db>,
    pub generic_params: GenericParamListId<'db>,
    /// `None` for an inherent impl.
    pub trait_ref: Option<Partial<TraitRefId<'db>>>,
    pub ty: Partial<TypeId<'db>>,
    pub selection_name: Option<IdentId<'db>>,
    pub where_clause: QuoteWhereClause<'db>,
    pub items: Vec<QuoteItem<'db>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, salsa::Update)]
pub struct QuoteWhereClause<'db> {
    pub predicates: Vec<QuoteWherePredicate<'db>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub enum QuoteWherePredicate<'db> {
    Bound(WherePredicate<'db>),
    /// A const predicate, `where { N > 0 }`, in the template's arena.
    Const(ExprId),
    /// `where ${w}`.
    Hole(u32),
}
