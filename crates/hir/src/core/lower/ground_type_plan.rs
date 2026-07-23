//! Base-graph normalization facts for provider-time ground type reflection.
//!
//! This deliberately produces resolved HIR definition identities rather than
//! semantic `TyId`s: provider expansion is upstream of merged type analysis.
//! Recursive-type-function validity and subject steps come from the same
//! syntax checker used by ordinary normalization.

use crate::core::hir_def::scope_graph::ScopeId;
use crate::{
    HirDb,
    analysis::ty::type_fn::{
        TypeFnWfData, apply_subject_step, subject_step_from_body, type_fn_syntax_wf_base,
    },
    core::{
        hir_def::{
            Body, ConstGenericArgValue, Expr, GenericArg, IntegerId, ItemKind, LitKind, Partial,
            PathId, Stmt, TopLevelMod, TypeFnDef, TypeFnPat, TypeId, TypeKind,
        },
        lower::base_scope_graph_impl,
    },
};

const PLAN_NODE_LIMIT: usize = 256;
const PLAN_UNFOLD_LIMIT: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GroundTypePlanError {
    Unsupported,
    IllFormedTypeFn,
    NodeLimit,
    UnfoldLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GroundArg<'db> {
    Type(usize),
    Const(IntegerId<'db>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GroundTypeNode<'db> {
    pub constructor: ItemKind<'db>,
    pub args: Vec<GroundArg<'db>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct GroundTypePlan<'db> {
    pub nodes: Vec<GroundTypeNode<'db>>,
    pub root: usize,
}

pub(super) fn base_ground_type_plan<'db>(
    db: &'db dyn HirDb,
    root: TypeId<'db>,
    scope: ScopeId<'db>,
) -> Result<GroundTypePlan<'db>, GroundTypePlanError> {
    Evaluator {
        db,
        top_mod: scope.top_mod(db),
        nodes: Vec::new(),
        unfolds: 0,
    }
    .finish(root)
}

pub(super) fn base_ground_constructor<'db>(
    db: &'db dyn HirDb,
    ty: TypeId<'db>,
    scope: ScopeId<'db>,
) -> Result<ItemKind<'db>, GroundTypePlanError> {
    let TypeKind::Path(Partial::Present(path)) = ty.data(db) else {
        return Err(GroundTypePlanError::Unsupported);
    };
    if path.parent(db).is_some() {
        return Err(GroundTypePlanError::Unsupported);
    }
    let name = path
        .ident(db)
        .to_opt()
        .ok_or(GroundTypePlanError::Unsupported)?;
    let base = base_scope_graph_impl(db, scope.top_mod(db));
    base.items_dfs(db)
        .find(|item| item.name(db) == Some(name))
        .ok_or(GroundTypePlanError::Unsupported)
}

struct Evaluator<'db> {
    db: &'db dyn HirDb,
    top_mod: TopLevelMod<'db>,
    nodes: Vec<GroundTypeNode<'db>>,
    unfolds: usize,
}

#[derive(Clone, Copy)]
struct SubjectEnv<'db> {
    value: IntegerId<'db>,
}

impl<'db> Evaluator<'db> {
    fn finish(mut self, root: TypeId<'db>) -> Result<GroundTypePlan<'db>, GroundTypePlanError> {
        let root = self.eval_ty(root, None)?;
        Ok(GroundTypePlan {
            nodes: self.nodes,
            root,
        })
    }

    fn push(
        &mut self,
        constructor: ItemKind<'db>,
        args: Vec<GroundArg<'db>>,
    ) -> Result<usize, GroundTypePlanError> {
        if self.nodes.len() == PLAN_NODE_LIMIT {
            return Err(GroundTypePlanError::NodeLimit);
        }
        let id = self.nodes.len();
        self.nodes.push(GroundTypeNode { constructor, args });
        Ok(id)
    }

    fn eval_ty(
        &mut self,
        ty: TypeId<'db>,
        env: Option<SubjectEnv<'db>>,
    ) -> Result<usize, GroundTypePlanError> {
        let TypeKind::Path(Partial::Present(path)) = ty.data(self.db) else {
            return Err(GroundTypePlanError::Unsupported);
        };
        let item = self.resolve_item(*path)?;
        match item {
            ItemKind::TypeAlias(alias) => {
                if !path.generic_args(self.db).is_empty(self.db) {
                    return Err(GroundTypePlanError::Unsupported);
                }
                let target = alias
                    .type_ref(self.db)
                    .to_opt()
                    .ok_or(GroundTypePlanError::Unsupported)?;
                self.eval_ty(target, env)
            }
            ItemKind::TypeFn(def) => self.eval_type_fn(def, *path, env),
            ItemKind::Struct(_) | ItemKind::Enum(_) => {
                let mut args = Vec::new();
                for arg in path.generic_args(self.db).data(self.db) {
                    args.push(match arg {
                        GenericArg::Type(arg) => GroundArg::Type(self.eval_ty(
                            arg.ty.to_opt().ok_or(GroundTypePlanError::Unsupported)?,
                            env,
                        )?),
                        GenericArg::Const(arg) => {
                            GroundArg::Const(self.eval_const_arg(arg.value, env)?)
                        }
                        GenericArg::AssocType(_) => return Err(GroundTypePlanError::Unsupported),
                    });
                }
                self.push(item, args)
            }
            _ => Err(GroundTypePlanError::Unsupported),
        }
    }

    fn eval_type_fn(
        &mut self,
        def: TypeFnDef<'db>,
        path: PathId<'db>,
        env: Option<SubjectEnv<'db>>,
    ) -> Result<usize, GroundTypePlanError> {
        self.unfolds += 1;
        if self.unfolds > PLAN_UNFOLD_LIMIT {
            return Err(GroundTypePlanError::UnfoldLimit);
        }
        let syntax = type_fn_syntax_wf_base(self.db, def);
        let data = syntax
            .data
            .as_ref()
            .ok_or(GroundTypePlanError::IllFormedTypeFn)?;
        let args = path.generic_args(self.db).data(self.db);
        if data.subject_idx != 0 || args.len() != 1 {
            // The first vertical slice intentionally accepts a subject-only
            // recurrence. Forwarded type arguments require an explicit ground
            // substitution arena and fail closed until that lands.
            return Err(GroundTypePlanError::Unsupported);
        }
        let GenericArg::Const(subject) = &args[0] else {
            return Err(GroundTypePlanError::Unsupported);
        };
        let value = self.eval_const_arg(subject.value, env)?;
        let arm = select_arm(self.db, data, value).ok_or(GroundTypePlanError::IllFormedTypeFn)?;
        self.eval_ty(arm.rhs_ty, Some(SubjectEnv { value }))
    }

    fn eval_const_arg(
        &self,
        value: ConstGenericArgValue<'db>,
        env: Option<SubjectEnv<'db>>,
    ) -> Result<IntegerId<'db>, GroundTypePlanError> {
        let ConstGenericArgValue::Expr(Partial::Present(body)) = value else {
            return Err(GroundTypePlanError::Unsupported);
        };
        eval_subject_expr(self.db, body, env)
    }

    fn resolve_item(&self, path: PathId<'db>) -> Result<ItemKind<'db>, GroundTypePlanError> {
        // This initial capability is deliberately local/base-only. Qualified
        // paths, imports, projections, and generated items fail closed.
        if path.parent(self.db).is_some() {
            return Err(GroundTypePlanError::Unsupported);
        }
        let name = path
            .ident(self.db)
            .to_opt()
            .ok_or(GroundTypePlanError::Unsupported)?;
        let base = base_scope_graph_impl(self.db, self.top_mod);
        base.items_dfs(self.db)
            .find(|item| item.name(self.db) == Some(name))
            .ok_or(GroundTypePlanError::Unsupported)
    }
}

fn select_arm<'a, 'db>(
    db: &'db dyn HirDb,
    data: &'a TypeFnWfData<'db>,
    value: IntegerId<'db>,
) -> Option<&'a crate::analysis::ty::type_fn::TypeFnArmData<'db>> {
    data.arms
        .iter()
        .find(|arm| matches!(arm.pat, TypeFnPat::Lit(v) if v.data(db) == value.data(db)))
        .or_else(|| {
            data.arms
                .iter()
                .find(|arm| matches!(arm.pat, TypeFnPat::Wild))
        })
}

fn eval_subject_expr<'db>(
    db: &'db dyn HirDb,
    body: Body<'db>,
    env: Option<SubjectEnv<'db>>,
) -> Result<IntegerId<'db>, GroundTypePlanError> {
    fn root<'db>(db: &'db dyn HirDb, body: Body<'db>) -> Option<Expr<'db>> {
        let expr = body.expr(db).data(db, body).clone().to_opt()?;
        if let Expr::Block(stmts) = &expr
            && let [stmt] = stmts.as_slice()
            && let Partial::Present(Stmt::Expr(inner)) = stmt.data(db, body)
        {
            return inner.data(db, body).clone().to_opt();
        }
        Some(expr)
    }

    match root(db, body).ok_or(GroundTypePlanError::Unsupported)? {
        Expr::Lit(LitKind::Int(value)) => Ok(value),
        Expr::Bin(..) => {
            let env = env.ok_or(GroundTypePlanError::Unsupported)?;
            let step = subject_step_from_body(db, body).ok_or(GroundTypePlanError::Unsupported)?;
            let next = apply_subject_step(db, step, env.value.data(db));
            Ok(IntegerId::new(db, next))
        }
        _ => Err(GroundTypePlanError::Unsupported),
    }
}
