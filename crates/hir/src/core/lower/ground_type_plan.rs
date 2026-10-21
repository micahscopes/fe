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
    occurrences: Vec<EvalEnv<'db>>,
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
        occurrences: Vec::new(),
    }
    .finish(root)
}

/// Finite declaration-order paths to occurrences of one resolved nominal leaf.
/// Generic arguments are substituted in their occurrence environment; aliases
/// and recursive type functions use the same evaluator as ground reflection.
/// This exposes paths, not access privileges: generated accesses are checked
/// by ordinary semantic analysis at their destination.
pub(super) fn base_record_leaf_paths<'db>(
    db: &'db dyn HirDb,
    root: TypeId<'db>,
    scope: ScopeId<'db>,
    leaf: ItemKind<'db>,
) -> Result<Vec<Vec<IdentId<'db>>>, GroundTypePlanError> {
    let mut evaluator = Evaluator {
        db,
        root_top_mod: scope.top_mod(db),
        nodes: Vec::new(),
        unfolds: 0,
        occurrences: Vec::new(),
    };
    let root = evaluator.eval_ty(root, scope.top_mod(db), None)?;
    let mut paths = Vec::new();
    evaluator.collect_leaf_paths(root, leaf, &mut Vec::new(), &mut paths, 0)?;
    Ok(paths)
}

pub(super) fn normalized_record_leaf_paths<'db>(
    db: &'db dyn HirDb,
    plan: &GroundTypePlan<'db>,
    root: usize,
    leaf: ItemKind<'db>,
) -> Result<Vec<Vec<IdentId<'db>>>, GroundTypePlanError> {
    let root_top_mod = match plan.nodes[root].constructor {
        ItemKind::Struct(item) => item.top_mod(db),
        _ => return Err(GroundTypePlanError::Unsupported),
    };
    let mut evaluator = Evaluator {
        db,
        root_top_mod,
        nodes: plan.nodes.clone(),
        unfolds: 0,
        occurrences: plan.occurrences.clone(),
    };
    let mut paths = Vec::new();
    evaluator.collect_leaf_paths(root, leaf, &mut Vec::new(), &mut paths, 0)?;
    Ok(paths)
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
    // Substitution environments of nominal occurrences, indexed like nodes.
    occurrences: Vec<EvalEnv<'db>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvalEnv<'db> {
    type_args: Vec<(IdentId<'db>, TypeBinding<'db>)>,
    const_args: Vec<(IdentId<'db>, BaseConstValue<'db>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeBinding<'db> {
    ty: TypeId<'db>,
    top_mod: TopLevelMod<'db>,
    env: Option<Box<EvalEnv<'db>>>,
}

impl<'db> EvalEnv<'db> {
    fn type_arg(&self, name: IdentId<'db>) -> Option<TypeBinding<'db>> {
        self.type_args
            .iter()
            .rev()
            .find_map(|(bound, value)| (*bound == name).then(|| value.clone()))
    }

    fn const_arg(&self, name: IdentId<'db>) -> Option<BaseConstValue<'db>> {
        self.const_args
            .iter()
            .rev()
            .find_map(|(bound, value)| (*bound == name).then_some(*value))
    }
}

impl<'db> Evaluator<'db> {
    fn collect_leaf_paths(
        &mut self,
        node: usize,
        leaf: ItemKind<'db>,
        prefix: &mut Vec<IdentId<'db>>,
        paths: &mut Vec<Vec<IdentId<'db>>>,
        depth: usize,
    ) -> Result<(), GroundTypePlanError> {
        if depth >= PLAN_NODE_LIMIT || paths.len() >= PLAN_NODE_LIMIT {
            return Err(GroundTypePlanError::NodeLimit);
        }
        let constructor = self.nodes[node].constructor;
        if constructor == leaf {
            paths.push(prefix.clone());
            return Ok(());
        }
        let ItemKind::Struct(struct_) = constructor else {
            return Err(GroundTypePlanError::Unsupported);
        };
        let env = self.occurrences[node].clone();
        for field in struct_.fields(self.db).data(self.db) {
            let name = field
                .name
                .to_opt()
                .ok_or(GroundTypePlanError::Unsupported)?;
            let ty = field
                .type_ref()
                .to_opt()
                .ok_or(GroundTypePlanError::Unsupported)?;
            let child = self.eval_ty(ty, struct_.top_mod(self.db), Some(&env))?;
            prefix.push(name);
            self.collect_leaf_paths(child, leaf, prefix, paths, depth + 1)?;
            prefix.pop();
        }
        Ok(())
    }

    fn finish(mut self, root: TypeId<'db>) -> Result<GroundTypePlan<'db>, GroundTypePlanError> {
        let root = self.eval_ty(root, self.root_top_mod, None)?;
        Ok(GroundTypePlan {
            nodes: self.nodes,
            root,
            occurrences: self.occurrences,
        })
    }

    fn push(
        &mut self,
        constructor: ItemKind<'db>,
        args: Vec<GroundArg<'db>>,
        env: EvalEnv<'db>,
    ) -> Result<usize, GroundTypePlanError> {
        if self.nodes.len() == PLAN_NODE_LIMIT {
            return Err(GroundTypePlanError::NodeLimit);
        }
        let id = self.nodes.len();
        self.nodes.push(GroundTypeNode { constructor, args });
        self.occurrences.push(env);
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
            return self.eval_ty(binding.ty, binding.top_mod, binding.env.as_deref());
        }

        let item =
            resolve_base_item(self.db, top_mod, *path).ok_or(GroundTypePlanError::Unsupported)?;
        match item {
            ItemKind::TypeAlias(alias) => {
                self.unfolds += 1;
                if self.unfolds > PLAN_UNFOLD_LIMIT {
                    return Err(GroundTypePlanError::UnfoldLimit);
                }
                let params = alias.generic_params(self.db).data(self.db);
                let args = path.generic_args(self.db).data(self.db);
                if params.len() != args.len() {
                    return Err(GroundTypePlanError::Unsupported);
                }
                let mut alias_env = EvalEnv {
                    type_args: Vec::new(),
                    const_args: Vec::new(),
                };
                for (param, arg) in params.iter().zip(args) {
                    let name = param
                        .name()
                        .to_opt()
                        .ok_or(GroundTypePlanError::Unsupported)?;
                    match (param, arg) {
                        (GenericParam::Type(_), GenericArg::Type(arg)) => {
                            let ty = arg.ty.to_opt().ok_or(GroundTypePlanError::Unsupported)?;
                            alias_env.type_args.push((
                                name,
                                TypeBinding {
                                    ty,
                                    top_mod,
                                    env: env.cloned().map(Box::new),
                                },
                            ));
                        }
                        (GenericParam::Const(param), GenericArg::Const(arg)) => {
                            let kind =
                                super::base_const_eval::raw_uint_kind(self.db, param.ty.to_opt())
                                    .ok_or(GroundTypePlanError::Unsupported)?;
                            let value = self.eval_const_arg(arg.value, top_mod, env, kind)?;
                            alias_env.const_args.push((name, value));
                        }
                        _ => return Err(GroundTypePlanError::Unsupported),
                    }
                }
                let target = alias
                    .type_ref(self.db)
                    .to_opt()
                    .ok_or(GroundTypePlanError::Unsupported)?;
                self.eval_ty(target, alias.top_mod(self.db), Some(&alias_env))
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
                let mut occurrence = EvalEnv {
                    type_args: Vec::new(),
                    const_args: Vec::new(),
                };
                for (param, arg) in params.iter().zip(generic_args) {
                    let name = param
                        .name()
                        .to_opt()
                        .ok_or(GroundTypePlanError::Unsupported)?;
                    args.push(match arg {
                        GenericArg::Type(arg) => {
                            if !matches!(param, GenericParam::Type(_)) {
                                return Err(GroundTypePlanError::Unsupported);
                            }
                            occurrence.type_args.push((
                                name,
                                TypeBinding {
                                    ty: arg.ty.to_opt().ok_or(GroundTypePlanError::Unsupported)?,
                                    top_mod,
                                    env: env.cloned().map(Box::new),
                                },
                            ));
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
                            occurrence.const_args.push((
                                name,
                                BaseConstValue::UInt {
                                    value,
                                    kind: expected,
                                },
                            ));
                            GroundArg::Const(value)
                        }
                        GenericArg::AssocType(_) => return Err(GroundTypePlanError::Unsupported),
                    });
                }
                self.push(item, args, occurrence)
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
                        env: parent_env.cloned().map(Box::new),
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
