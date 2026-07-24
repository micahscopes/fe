//! Base-graph normalization facts for provider-time ground type reflection.
//!
//! The evaluator is deliberately upstream of semantic analysis. It resolves
//! only source/base-graph definitions, carries resolved HIR item identities,
//! and fails closed on projections, holes, generated items, or unsupported
//! CTFE. Recursive definition validation is shared with ordinary
//! recursive-type-function normalization; staged scalar computation is owned
//! by the shared base const evaluator.

use crate::{
    HirDb,
    analysis::ty::type_fn::{TypeFnWfData, type_fn_syntax_wf_base},
    core::{
        hir_def::{
            ConstGenericArgValue, GenericArg, GenericParam, IdentId, IntegerId, ItemKind, Partial,
            PathId, TopLevelMod, TypeFnDef, TypeFnPat, TypeId, TypeKind, scope_graph::ScopeId,
        },
        lower::{
            base_const_eval::{
                BaseConstEvalError, BaseConstValue, BaseUIntKind, eval_base_const_body,
                eval_base_uint_const_item_as,
            },
            provider::resolve_base_item,
        },
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
    CtfeLimit,
}

impl From<BaseConstEvalError> for GroundTypePlanError {
    fn from(error: BaseConstEvalError) -> Self {
        match error {
            BaseConstEvalError::Unsupported => Self::Unsupported,
            BaseConstEvalError::Limit => Self::CtfeLimit,
        }
    }
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
        root_top_mod: scope.top_mod(db),
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
    resolve_base_item(db, scope.top_mod(db), *path).ok_or(GroundTypePlanError::Unsupported)
}

struct Evaluator<'db> {
    db: &'db dyn HirDb,
    root_top_mod: TopLevelMod<'db>,
    nodes: Vec<GroundTypeNode<'db>>,
    unfolds: usize,
}

#[derive(Clone)]
struct EvalEnv<'db> {
    type_args: Vec<(IdentId<'db>, TypeBinding<'db>)>,
    const_args: Vec<(IdentId<'db>, BaseConstValue<'db>)>,
}

#[derive(Clone, Copy)]
struct TypeBinding<'db> {
    ty: TypeId<'db>,
    top_mod: TopLevelMod<'db>,
}

impl<'db> EvalEnv<'db> {
    fn type_arg(&self, name: IdentId<'db>) -> Option<TypeBinding<'db>> {
        self.type_args
            .iter()
            .rev()
            .find_map(|(bound, value)| (*bound == name).then_some(*value))
    }

    fn const_arg(&self, name: IdentId<'db>) -> Option<BaseConstValue<'db>> {
        self.const_args
            .iter()
            .rev()
            .find_map(|(bound, value)| (*bound == name).then_some(*value))
    }
}

impl<'db> Evaluator<'db> {
    fn finish(mut self, root: TypeId<'db>) -> Result<GroundTypePlan<'db>, GroundTypePlanError> {
        let root = self.eval_ty(root, self.root_top_mod, None)?;
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
        top_mod: TopLevelMod<'db>,
        env: Option<&EvalEnv<'db>>,
    ) -> Result<usize, GroundTypePlanError> {
        let TypeKind::Path(Partial::Present(path)) = ty.data(self.db) else {
            return Err(GroundTypePlanError::Unsupported);
        };
        if path.parent(self.db).is_none()
            && let Some(name) = path.ident(self.db).to_opt()
            && let Some(binding) = env.and_then(|env| env.type_arg(name))
        {
            return self.eval_ty(binding.ty, binding.top_mod, None);
        }

        let item =
            resolve_base_item(self.db, top_mod, *path).ok_or(GroundTypePlanError::Unsupported)?;
        match item {
            ItemKind::TypeAlias(alias) => {
                if !path.generic_args(self.db).is_empty(self.db) {
                    return Err(GroundTypePlanError::Unsupported);
                }
                let target = alias
                    .type_ref(self.db)
                    .to_opt()
                    .ok_or(GroundTypePlanError::Unsupported)?;
                self.eval_ty(target, alias.top_mod(self.db), env)
            }
            ItemKind::TypeFn(def) => self.eval_type_fn(def, *path, top_mod, env),
            ItemKind::Struct(_) | ItemKind::Enum(_) => {
                let params = match item {
                    ItemKind::Struct(item) => item.generic_params(self.db),
                    ItemKind::Enum(item) => item.generic_params(self.db),
                    _ => unreachable!(),
                };
                let params = params.data(self.db);
                let generic_args = path.generic_args(self.db).data(self.db);
                if params.len() != generic_args.len() {
                    return Err(GroundTypePlanError::Unsupported);
                }
                let mut args = Vec::new();
                for (param, arg) in params.iter().zip(generic_args) {
                    args.push(match arg {
                        GenericArg::Type(arg) => {
                            if !matches!(param, GenericParam::Type(_)) {
                                return Err(GroundTypePlanError::Unsupported);
                            }
                            GroundArg::Type(self.eval_ty(
                                arg.ty.to_opt().ok_or(GroundTypePlanError::Unsupported)?,
                                top_mod,
                                env,
                            )?)
                        }
                        GenericArg::Const(arg) => {
                            let GenericParam::Const(param) = param else {
                                return Err(GroundTypePlanError::Unsupported);
                            };
                            let expected =
                                super::base_const_eval::raw_uint_kind(self.db, param.ty.to_opt())
                                    .ok_or(GroundTypePlanError::Unsupported)?;
                            let BaseConstValue::UInt { value, .. } =
                                self.eval_const_arg(arg.value, top_mod, env, expected)?
                            else {
                                return Err(GroundTypePlanError::Unsupported);
                            };
                            GroundArg::Const(value)
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
        call_top_mod: TopLevelMod<'db>,
        parent_env: Option<&EvalEnv<'db>>,
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
        let params = def.hir_generic_params(self.db).data(self.db);
        let args = path.generic_args(self.db).data(self.db);
        if args.len() != params.len() || data.subject_idx >= args.len() {
            return Err(GroundTypePlanError::Unsupported);
        }

        let mut env = EvalEnv {
            type_args: Vec::new(),
            const_args: Vec::new(),
        };
        for (param, arg) in params.iter().zip(args) {
            let name = param
                .name()
                .to_opt()
                .ok_or(GroundTypePlanError::Unsupported)?;
            match (param, arg) {
                (GenericParam::Type(_), GenericArg::Type(arg)) => env.type_args.push((
                    name,
                    TypeBinding {
                        ty: arg.ty.to_opt().ok_or(GroundTypePlanError::Unsupported)?,
                        top_mod: call_top_mod,
                    },
                )),
                (GenericParam::Const(param), GenericArg::Const(arg)) => {
                    let kind = super::base_const_eval::raw_uint_kind(self.db, param.ty.to_opt())
                        .ok_or(GroundTypePlanError::Unsupported)?;
                    let value = self.eval_const_arg(arg.value, call_top_mod, parent_env, kind)?;
                    env.const_args.push((name, value));
                }
                // A forwarded const identifier in a recursive type-function
                // application is initially lowered as a type-shaped generic
                // argument (`SparsePlan<Keep0, ...>`). Resolve that exact bare
                // identifier from the caller's const environment rather than
                // requiring source-level `{Keep0}` noise.
                (GenericParam::Const(param), GenericArg::Type(arg)) => {
                    let kind = super::base_const_eval::raw_uint_kind(self.db, param.ty.to_opt())
                        .ok_or(GroundTypePlanError::Unsupported)?;
                    let ty = arg.ty.to_opt().ok_or(GroundTypePlanError::Unsupported)?;
                    let TypeKind::Path(Partial::Present(path)) = ty.data(self.db) else {
                        return Err(GroundTypePlanError::Unsupported);
                    };
                    let forwarded = if let Some(forwarded) = path
                        .as_ident(self.db)
                        .and_then(|forwarded| parent_env.and_then(|env| env.const_arg(forwarded)))
                    {
                        forwarded
                    } else {
                        let ItemKind::Const(const_) =
                            resolve_base_item(self.db, call_top_mod, *path)
                                .ok_or(GroundTypePlanError::Unsupported)?
                        else {
                            return Err(GroundTypePlanError::Unsupported);
                        };
                        eval_base_uint_const_item_as(self.db, const_, kind)?
                    };
                    env.const_args.push((name, forwarded));
                }
                _ => {
                    return Err(GroundTypePlanError::Unsupported);
                }
            }
        }

        let subject_name = params[data.subject_idx]
            .name()
            .to_opt()
            .ok_or(GroundTypePlanError::Unsupported)?;
        let BaseConstValue::UInt { value, .. } = env
            .const_arg(subject_name)
            .ok_or(GroundTypePlanError::Unsupported)?
        else {
            return Err(GroundTypePlanError::Unsupported);
        };
        let arm = select_arm(self.db, data, value).ok_or(GroundTypePlanError::IllFormedTypeFn)?;
        self.eval_ty(arm.rhs_ty, def.top_mod(self.db), Some(&env))
    }

    fn eval_const_arg(
        &self,
        value: ConstGenericArgValue<'db>,
        top_mod: TopLevelMod<'db>,
        env: Option<&EvalEnv<'db>>,
        expected: BaseUIntKind,
    ) -> Result<BaseConstValue<'db>, GroundTypePlanError> {
        let ConstGenericArgValue::Expr(Partial::Present(body)) = value else {
            return Err(GroundTypePlanError::Unsupported);
        };
        let inherited = env.map(|env| env.const_args.as_slice()).unwrap_or(&[]);
        Ok(eval_base_const_body(
            self.db,
            top_mod,
            body,
            Vec::new(),
            inherited,
            Some(expected),
        )?)
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
