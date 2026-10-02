//! Read-only addresses into the existing checked code representation.

use crate::{
    analysis::{
        HirAnalysisDb,
        semantic::{CodeNodeSite, QuotedCode},
        ty::ty_check::{TypedBody, ValuePathRef, check_func_body},
    },
    core::meta::FrozenTemplateSource,
    hir_def::{
        ArithBinOp, BinOp, Body, CallArg, CompBinOp, Cond, Expr, ExprId, Field, LogicalBinOp,
        MatchArm, Partial, Pat, PatId, RecordPatField, Stmt, StmtId, UnOp,
    },
};

pub(super) fn checked_body<'db>(
    db: &'db dyn HirAnalysisDb,
    code: QuotedCode<'db>,
) -> Result<(Body<'db>, &'db TypedBody<'db>), &'static str> {
    let site = match code.source(db) {
        crate::analysis::semantic::CodeSource::Template(site) => site,
        crate::analysis::semantic::CodeSource::Empty => {
            return Err("empty code has no checked body");
        }
        crate::analysis::semantic::CodeSource::VariantBinding { .. } => {
            return Err("compiler-issued binder code has no checked body");
        }
    };
    if site.source != FrozenTemplateSource::CheckedBody || !code.holes(db).is_empty() {
        return Err(
            "code inspection requires a checked body; untyped quote templates are unsupported",
        );
    }
    let func = site
        .owner
        .containing_func(db)
        .ok_or("checked code has no function owner")?;
    let body = func.body(db).ok_or("checked code owner has no body")?;
    if site.owner != body || site.expr != body.expr(db) || func.has_effects(db) {
        return Err("invalid checked-body code origin");
    }
    let (errors, typed) = check_func_body(db, func);
    if !errors.is_empty() {
        return Err("code owner does not type check");
    }
    Ok((body, typed))
}

/// Child access borrows the source's existing lists. Wide blocks and records
/// do not allocate a new list or scan all children for each indexed access.
pub(super) enum Children<'db> {
    Fixed([Option<CodeNodeSite>; 3]),
    Exprs(&'db [ExprId]),
    Stmts(&'db [StmtId]),
    Pats(&'db [PatId]),
    Args(Option<ExprId>, &'db [CallArg<'db>]),
    Fields(&'db [Field<'db>]),
    PatternFields(&'db [RecordPatField<'db>]),
    Match(ExprId, &'db [MatchArm]),
}

impl Children<'_> {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Fixed(sites) => sites.iter().flatten().count(),
            Self::Exprs(sites) => sites.len(),
            Self::Stmts(sites) => sites.len(),
            Self::Pats(sites) => sites.len(),
            Self::Args(first, sites) => usize::from(first.is_some()) + sites.len(),
            Self::Fields(sites) => sites.len(),
            Self::PatternFields(sites) => sites.len(),
            Self::Match(_, arms) => 1 + arms.len() * 2,
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<CodeNodeSite> {
        match self {
            Self::Fixed(sites) => sites.iter().flatten().nth(index).copied(),
            Self::Exprs(sites) => sites.get(index).copied().map(CodeNodeSite::Expr),
            Self::Stmts(sites) => sites.get(index).copied().map(CodeNodeSite::Stmt),
            Self::Pats(sites) => sites.get(index).copied().map(CodeNodeSite::Pat),
            Self::Args(Some(first), _) if index == 0 => Some(CodeNodeSite::Expr(*first)),
            Self::Args(first, sites) => sites
                .get(index - usize::from(first.is_some()))
                .map(|arg| CodeNodeSite::Expr(arg.expr)),
            Self::Fields(sites) => sites.get(index).map(|field| CodeNodeSite::Expr(field.expr)),
            Self::PatternFields(sites) => {
                sites.get(index).map(|field| CodeNodeSite::Pat(field.pat))
            }
            Self::Match(first, _) if index == 0 => Some(CodeNodeSite::Expr(*first)),
            Self::Match(_, arms) => arms.get((index - 1) / 2).map(|arm| {
                if index % 2 == 1 {
                    CodeNodeSite::Pat(arm.pat())
                } else {
                    CodeNodeSite::Expr(arm.body())
                }
            }),
        }
    }
}

pub(super) struct Description<'db> {
    pub kind: &'static str,
    pub children: Children<'db>,
}

fn fixed(kind: &'static str, sites: &[CodeNodeSite]) -> Description<'static> {
    let mut children = [None; 3];
    for (slot, site) in children.iter_mut().zip(sites) {
        *slot = Some(*site);
    }
    Description {
        kind,
        children: Children::Fixed(children),
    }
}

fn present<T>(value: &Partial<T>) -> Result<&T, &'static str> {
    match value {
        Partial::Present(value) => Ok(value),
        Partial::Absent => Err("code node is incomplete"),
    }
}

pub(super) fn describe<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    typed: &'db TypedBody<'db>,
    site: CodeNodeSite,
) -> Result<Description<'db>, &'static str> {
    use CodeNodeSite::{Cond as C, Expr as E, Pat as P, Stmt as S};
    let missing = "code node is missing from its checked body";
    Ok(match site {
        E(id) => match present(body.exprs(db).get(id).ok_or(missing)?)? {
            Expr::Lit(crate::hir_def::LitKind::Int(_)) => fixed("Integer", &[]),
            Expr::Lit(crate::hir_def::LitKind::Bool(_)) => fixed("Boolean", &[]),
            Expr::Lit(crate::hir_def::LitKind::String(_)) => fixed("String", &[]),
            Expr::Block(stmts) => Description {
                kind: "Block",
                children: Children::Stmts(stmts),
            },
            Expr::Bin(lhs, rhs, op) => fixed(binary_kind(*op), &[E(*lhs), E(*rhs)]),
            Expr::Un(value, op) => fixed(
                match op {
                    UnOp::Plus => "Positive",
                    UnOp::Minus => "Negative",
                    UnOp::Not => "Not",
                    UnOp::BitNot => "BitNot",
                    UnOp::Mut => "BorrowMut",
                    UnOp::Ref => "Borrow",
                    UnOp::Deref => "Deref",
                },
                &[E(*value)],
            ),
            Expr::Cast(value, _) => fixed("Cast", &[E(*value)]),
            Expr::Call(callee, args) => Description {
                kind: "Call",
                children: Children::Args(Some(*callee), args),
            },
            Expr::Assert(args) => Description {
                kind: "Assert",
                children: Children::Args(None, args),
            },
            Expr::MethodCall(receiver, _, _, args) => Description {
                kind: "MethodCall",
                children: Children::Args(Some(*receiver), args),
            },
            Expr::Path(_) => fixed(
                match typed.expr_binding(id) {
                    Some(crate::analysis::ty::ty_check::LocalBinding::Param { .. }) => "Parameter",
                    Some(crate::analysis::ty::ty_check::LocalBinding::Local { .. }) => "Local",
                    Some(crate::analysis::ty::ty_check::LocalBinding::EffectParam { .. }) => {
                        return Err("effect parameters are unsupported in code inspection");
                    }
                    None if matches!(
                        typed.value_path_ref(id),
                        Some(ValuePathRef::TypeConst(_))
                    ) =>
                    {
                        "ConstParameter"
                    }
                    None if typed.expr_const_ref(id).is_some() => "Constant",
                    None => "Function",
                },
                &[],
            ),
            Expr::RecordInit(_, fields) => Description {
                kind: "Record",
                children: Children::Fields(fields),
            },
            Expr::Field(receiver, _) => fixed("Field", &[E(*receiver)]),
            Expr::Tuple(values) => Description {
                kind: "Tuple",
                children: Children::Exprs(values),
            },
            Expr::Array(values) => Description {
                kind: "Array",
                children: Children::Exprs(values),
            },
            Expr::ArrayRep(value, _) => fixed("ArrayRepeat", &[E(*value)]),
            Expr::If(cond, then, otherwise) => {
                let mut desc = fixed("If", &[C(*cond), E(*then)]);
                if let Children::Fixed(children) = &mut desc.children {
                    children[2] = otherwise.map(E);
                }
                desc
            }
            Expr::Match(value, Partial::Present(arms))
                if arms.iter().all(|arm| arm.parts().is_some()) =>
            {
                Description {
                    kind: "Match",
                    children: Children::Match(*value, arms),
                }
            }
            Expr::Assign(lhs, rhs) => fixed("Assign", &[E(*lhs), E(*rhs)]),
            Expr::AugAssign(lhs, rhs, _) => fixed("AugAssign", &[E(*lhs), E(*rhs)]),
            Expr::With(..)
            | Expr::Quote(_)
            | Expr::Hole(_)
            | Expr::UnsupportedMacroCall
            | Expr::Match(..) => {
                return Err("unsupported checked expression in code inspection");
            }
        },
        S(id) => match present(body.stmts(db).get(id).ok_or(missing)?)? {
            Stmt::Let(pat, _, value) => {
                let mut desc = fixed("Let", &[P(*pat)]);
                if let Children::Fixed(children) = &mut desc.children {
                    children[1] = value.map(E);
                }
                desc
            }
            Stmt::For(pat, iterable, value, _) => fixed("For", &[P(*pat), E(*iterable), E(*value)]),
            Stmt::While(cond, value) => fixed("While", &[C(*cond), E(*value)]),
            Stmt::Break => fixed("Break", &[]),
            Stmt::Continue => fixed("Continue", &[]),
            Stmt::Return(Some(value)) => fixed("Return", &[E(*value)]),
            Stmt::Return(None) => fixed("Return", &[]),
            Stmt::Expr(value) => fixed("Statement", &[E(*value)]),
        },
        C(id) => match present(body.conds(db).get(id).ok_or(missing)?)? {
            Cond::Expr(value) => fixed("Condition", &[E(*value)]),
            Cond::Let(pat, value) => fixed("ConditionLet", &[P(*pat), E(*value)]),
            Cond::Bin(lhs, rhs, op) => fixed(
                match op {
                    LogicalBinOp::And => "ConditionAnd",
                    LogicalBinOp::Or => "ConditionOr",
                },
                &[C(*lhs), C(*rhs)],
            ),
        },
        P(id) => match present(body.pats(db).get(id).ok_or(missing)?)? {
            Pat::Hole(_) => return Err("unexpanded pattern splice has no checked node"),
            Pat::WildCard => fixed("Wildcard", &[]),
            Pat::Rest => fixed("Rest", &[]),
            Pat::Path(..) if typed.pat_binding(id).is_some() => fixed("Binding", &[]),
            Pat::Path(..) | Pat::Lit(_) => fixed("PatternValue", &[]),
            Pat::Tuple(values) => Description {
                kind: "PatternTuple",
                children: Children::Pats(values),
            },
            Pat::PathTuple(_, values) => Description {
                kind: "PatternVariant",
                children: Children::Pats(values),
            },
            Pat::Record(_, fields) => Description {
                kind: "PatternRecord",
                children: Children::PatternFields(fields),
            },
            Pat::Or(lhs, rhs) => fixed("PatternOr", &[P(*lhs), P(*rhs)]),
        },
    })
}

pub(super) fn binary_kind(op: BinOp) -> &'static str {
    match op {
        BinOp::Arith(op) => match op {
            ArithBinOp::Add => "Add",
            ArithBinOp::Sub => "Sub",
            ArithBinOp::Mul => "Mul",
            ArithBinOp::Div => "Div",
            ArithBinOp::Rem => "Rem",
            ArithBinOp::Pow => "Pow",
            ArithBinOp::LShift => "LShift",
            ArithBinOp::RShift => "RShift",
            ArithBinOp::BitAnd => "BitAnd",
            ArithBinOp::BitOr => "BitOr",
            ArithBinOp::BitXor => "BitXor",
            ArithBinOp::Range => "Range",
        },
        BinOp::Comp(op) => match op {
            CompBinOp::Eq => "Eq",
            CompBinOp::NotEq => "NotEq",
            CompBinOp::Lt => "Lt",
            CompBinOp::LtEq => "LtEq",
            CompBinOp::Gt => "Gt",
            CompBinOp::GtEq => "GtEq",
        },
        BinOp::Logical(LogicalBinOp::And) => "And",
        BinOp::Logical(LogicalBinOp::Or) => "Or",
        BinOp::Index => "Index",
    }
}
