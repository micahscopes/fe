//! Replay of derive-provider builder commands into real HIR.
//!
//! The executor ([`super::provider_executor`]) records what a provider
//! *wants* (requirements, method signatures, method body expressions) as
//! transient command data. This module replays those commands through the
//! same [`HirBuilder`]/[`BodyBuilder`] synthesis vocabulary used by the
//! `#[event]`/`#[error]` desugarings, producing an ordinary
//! `impl Trait for Target` item in the expansion stage's scope graph.
//!
//! No derive shape knowledge lives here: which fields are compared, how
//! variant matches nest, what the method bodies look like — all of that is
//! decided by the provider's Fe code. This module only knows how to map
//! each generated-expression node onto the corresponding HIR expression.

use num_bigint::BigUint;

use super::{
    hir_builder::{BodyBuilder, HirBuilder},
    provider::{FieldName, ReflectedVariantKind, TargetReflection},
    provider_executor::{
        BuilderCommand, FieldKey, GenExpr, GenExprId, GenPat, GenPatId, ProviderOutput,
    },
};
use crate::{
    HirDb,
    hir_def::{
        BinOp, CompBinOp, Expr, ExprId, Field, FieldIndex, FuncModifiers, GenericArg, IdentId,
        IntegerId, LogicalBinOp, MatchArm, Partial, PathId, PathKind, RecordPatField, TraitRefId,
        TypeBound, TypeId, TypeKind, Visibility, WhereClauseId, WherePredicate,
    },
    span::DeriveDesugared,
};

/// Replays `output` into an `impl Trait for Target` item. `trait_ref` is the
/// canonical trait reference from the selected provider; `self_ty` is the
/// target's name applied to its own generic params; `generics` carries the
/// target's generic parameters and inherited predicates.
pub(super) fn synthesize_provider_impl<'db>(
    builder: &mut HirBuilder<'_, 'db, DeriveDesugared>,
    target_name: IdentId<'db>,
    self_ty: TypeId<'db>,
    generics: &super::derive::DeriveGenerics<'db>,
    reflection: &TargetReflection<'db>,
    trait_ref: TraitRefId<'db>,
    output: &ProviderOutput<'db>,
) {
    let db = builder.db();
    let where_clause = requirement_where_clause(db, generics, output);

    builder.impl_trait_generic(
        trait_ref,
        self_ty,
        generics.impl_params,
        where_clause,
        |builder| {
            for command in &output.commands {
                let BuilderCommand::EmitMethod { sig, body } = command else {
                    continue;
                };
                let sig = &output.sigs[sig.0];

                let mut params = Vec::new();
                if sig.takes_self {
                    params.push(builder.param_view_self());
                }
                for (name, ty) in &sig.args {
                    params.push(builder.param_underscore_named(*name, *ty));
                }
                let params = builder.params(params);

                let body_expr = *body;
                let replay = ReplayCtxt {
                    target_name,
                    trait_ref,
                    reflection,
                    output,
                };
                builder.func_with_body_inline_always(
                    sig.name,
                    builder.empty_generic_params(),
                    params,
                    sig.ret,
                    FuncModifiers::new(Visibility::Private, false, false, false),
                    move |body| {
                        let result = replay.replay_expr(body, body_expr);
                        body.emit_return(Some(result));
                    },
                );
            }
        },
    );
}

/// The where clause of a provider-generated impl: the target's own
/// predicates, plus one `P: Trait` predicate for every generic parameter
/// `P` of the target that is *mentioned* by the type of some `require`
/// command, in parameter order. A requirement on a composite type like
/// `Pair<T, bool>` therefore becomes `T: Trait` — predicates on the
/// composite type itself would shadow the (possibly generated) blanket impl
/// and make trait resolution ambiguous. Requirements on fully concrete
/// types need no predicate — the generated method bodies discharge them at
/// their use sites like hand-written code.
fn requirement_where_clause<'db>(
    db: &'db dyn HirDb,
    generics: &super::derive::DeriveGenerics<'db>,
    output: &ProviderOutput<'db>,
) -> WhereClauseId<'db> {
    let mut requirements: Vec<(TypeId<'db>, PathId<'db>)> = Vec::new();
    for command in &output.commands {
        if let BuilderCommand::Require { ty, trait_path } = command
            && !requirements.contains(&(*ty, *trait_path))
        {
            requirements.push((*ty, *trait_path));
        }
    }

    let mut preds = generics.inherited_preds.clone();
    for param_ty in &generics.param_tys {
        let Some(param_name) = ty_as_bare_ident(db, *param_ty) else {
            continue;
        };
        let mut bound_paths: Vec<PathId<'db>> = Vec::new();
        for (ty, trait_path) in &requirements {
            if ty_mentions_param(db, *ty, &[param_name]) && !bound_paths.contains(trait_path) {
                bound_paths.push(*trait_path);
            }
        }
        for trait_path in bound_paths {
            preds.push(WherePredicate {
                ty: Partial::Present(*param_ty),
                bounds: vec![TypeBound::Trait(TraitRefId::new(
                    db,
                    Partial::Present(trait_path),
                ))],
            });
        }
    }

    WhereClauseId::new(db, preds)
}

/// The identifier of `ty` when it is a bare single-segment path type.
fn ty_as_bare_ident<'db>(db: &'db dyn HirDb, ty: TypeId<'db>) -> Option<IdentId<'db>> {
    match ty.data(db) {
        TypeKind::Path(path) => path.to_opt()?.as_ident(db),
        _ => None,
    }
}

/// Whether `ty` syntactically mentions any of the target's generic
/// parameter names.
fn ty_mentions_param<'db>(db: &'db dyn HirDb, ty: TypeId<'db>, params: &[IdentId<'db>]) -> bool {
    match ty.data(db) {
        TypeKind::Path(path) => path
            .to_opt()
            .is_some_and(|path| path_mentions_param(db, path, params)),
        TypeKind::Ptr(inner) | TypeKind::Mode(_, inner) => inner
            .to_opt()
            .is_some_and(|inner| ty_mentions_param(db, inner, params)),
        TypeKind::Array(elem, _len) => elem
            .to_opt()
            .is_some_and(|elem| ty_mentions_param(db, elem, params)),
        TypeKind::Tuple(tuple) => tuple.data(db).iter().any(|elem| {
            elem.to_opt()
                .is_some_and(|elem| ty_mentions_param(db, elem, params))
        }),
        TypeKind::Never => false,
    }
}

fn path_mentions_param<'db>(
    db: &'db dyn HirDb,
    path: PathId<'db>,
    params: &[IdentId<'db>],
) -> bool {
    // A bare single-segment occurrence of a parameter name.
    if path.parent(db).is_none()
        && let PathKind::Ident {
            ident,
            generic_args,
        } = path.kind(db)
        && generic_args.is_empty(db)
        && ident.to_opt().is_some_and(|ident| params.contains(&ident))
    {
        return true;
    }
    // A parameter mentioned in the generic arguments of any segment.
    let mut cursor = Some(path);
    while let Some(p) = cursor {
        if let PathKind::Ident { generic_args, .. } = p.kind(db) {
            for arg in generic_args.data(db) {
                if let GenericArg::Type(type_arg) = arg
                    && type_arg
                        .ty
                        .to_opt()
                        .is_some_and(|ty| ty_mentions_param(db, ty, params))
                {
                    return true;
                }
            }
        }
        cursor = p.parent(db);
    }
    false
}

/// Shared replay context for one generated method body.
#[derive(Clone, Copy)]
struct ReplayCtxt<'a, 'db> {
    target_name: IdentId<'db>,
    trait_ref: TraitRefId<'db>,
    reflection: &'a TargetReflection<'db>,
    output: &'a ProviderOutput<'db>,
}

impl<'a, 'db> ReplayCtxt<'a, 'db> {
    fn replay_expr(
        &self,
        body: &mut BodyBuilder<'_, 'db, DeriveDesugared>,
        expr: GenExprId,
    ) -> ExprId {
        let db = body.db();
        match &self.output.exprs[expr.0] {
            GenExpr::Bool(value) => body.bool_lit_expr(*value),
            GenExpr::And(lhs, rhs) => {
                let lhs = self.replay_expr(body, *lhs);
                let rhs = self.replay_expr(body, *rhs);
                body.push_expr(Expr::Bin(lhs, rhs, BinOp::Logical(LogicalBinOp::And)))
            }
            GenExpr::SelfRef => body.path_expr(PathId::from_ident(db, IdentId::make_self(db))),
            GenExpr::ArgRef(name) => body.ident_expr(*name),
            GenExpr::FieldGet(base, field) => {
                let base = self.replay_expr(body, *base);
                let index = self.field_index(db, *field);
                body.push_expr(Expr::Field(base, Partial::Present(index)))
            }
            GenExpr::EqCmp(lhs, rhs) => {
                let lhs = self.replay_expr(body, *lhs);
                let rhs = self.replay_expr(body, *rhs);
                body.push_expr(Expr::Bin(lhs, rhs, BinOp::Comp(CompBinOp::Eq)))
            }
            GenExpr::TraitCall { ty, method } => {
                let qualified = PathId::new(
                    db,
                    PathKind::QualifiedType {
                        type_: *ty,
                        trait_: self.trait_ref,
                    },
                    None,
                )
                .push_ident(db, *method);
                let callee = body.path_expr(qualified);
                body.call_expr(callee, vec![])
            }
            GenExpr::StructInit { fields } => {
                let field_inits = fields
                    .iter()
                    .map(|(field, value)| {
                        let expr = self.replay_expr(body, *value);
                        Field {
                            label: self.field_label(*field),
                            expr,
                        }
                    })
                    .collect();
                let self_path = Partial::Present(PathId::from_ident(db, IdentId::make_self_ty(db)));
                body.push_expr(Expr::RecordInit(self_path, field_inits))
            }
            GenExpr::VariantInit { variant, fields } => {
                let variant_path = self.variant_path(db, *variant);
                let kind = self
                    .reflection
                    .variant(*variant)
                    .map(|v| v.kind)
                    .unwrap_or(ReflectedVariantKind::Unit);
                match kind {
                    ReflectedVariantKind::Unit => body.path_expr(variant_path),
                    ReflectedVariantKind::Tuple => {
                        let mut ordered = fields.clone();
                        ordered.sort_by_key(|(field, _)| field.index);
                        let args = ordered
                            .iter()
                            .map(|(_, value)| self.replay_expr(body, *value))
                            .collect();
                        let callee = body.path_expr(variant_path);
                        body.call_expr(callee, args)
                    }
                    ReflectedVariantKind::Record => {
                        let field_inits = fields
                            .iter()
                            .map(|(field, value)| {
                                let expr = self.replay_expr(body, *value);
                                Field {
                                    label: self.field_label(*field),
                                    expr,
                                }
                            })
                            .collect();
                        body.push_expr(Expr::RecordInit(
                            Partial::Present(variant_path),
                            field_inits,
                        ))
                    }
                }
            }
            GenExpr::Match { scrutinee, arms } => {
                let scrutinee = self.replay_expr(body, *scrutinee);
                let arms = arms
                    .iter()
                    .map(|(pat, arm_body)| {
                        let pat = self.replay_pat(body, *pat);
                        let arm_body = self.replay_expr(body, *arm_body);
                        MatchArm {
                            pat,
                            body: arm_body,
                        }
                    })
                    .collect();
                body.match_expr(scrutinee, arms)
            }
            GenExpr::VariantBinder {
                variant,
                field,
                prefix,
            } => {
                let binder = self.binder_ident(
                    db,
                    *prefix,
                    FieldKey {
                        variant: Some(*variant),
                        index: *field,
                    },
                );
                body.ident_expr(binder)
            }
        }
    }

    fn replay_pat(
        &self,
        body: &mut BodyBuilder<'_, 'db, DeriveDesugared>,
        pat: GenPatId,
    ) -> crate::hir_def::PatId {
        let db = body.db();
        match &self.output.pats[pat.0] {
            GenPat::Wildcard => body.wildcard_pat(),
            GenPat::Variant { variant, prefix } => {
                let variant_path = self.variant_path(db, *variant);
                let Some(reflected) = self.reflection.variant(*variant) else {
                    return body.wildcard_pat();
                };
                match reflected.kind {
                    ReflectedVariantKind::Unit => body.path_pat(variant_path),
                    ReflectedVariantKind::Tuple => {
                        let elems = reflected
                            .fields
                            .iter()
                            .map(|field| {
                                let binder = self.binder_ident(
                                    db,
                                    *prefix,
                                    FieldKey {
                                        variant: Some(*variant),
                                        index: field.index,
                                    },
                                );
                                body.bind_pat(binder)
                            })
                            .collect();
                        body.path_tuple_pat(variant_path, elems)
                    }
                    ReflectedVariantKind::Record => {
                        let fields = reflected
                            .fields
                            .iter()
                            .map(|field| {
                                let binder = self.binder_ident(
                                    db,
                                    *prefix,
                                    FieldKey {
                                        variant: Some(*variant),
                                        index: field.index,
                                    },
                                );
                                let pat = body.bind_pat(binder);
                                RecordPatField {
                                    label: match field.name {
                                        FieldName::Named(name) => Partial::Present(name),
                                        FieldName::Positional(_) => Partial::Absent,
                                    },
                                    pat,
                                }
                            })
                            .collect();
                        body.record_pat(variant_path, fields)
                    }
                }
            }
        }
    }

    fn variant_path(&self, db: &'db dyn HirDb, variant: usize) -> PathId<'db> {
        let base = PathId::from_ident(db, self.target_name);
        match self.reflection.variant(variant) {
            Some(reflected) => base.push_ident(db, reflected.name),
            None => base,
        }
    }

    fn field_label(&self, field: FieldKey) -> Option<IdentId<'db>> {
        match self
            .reflection
            .field(field.variant, field.index)
            .map(|f| f.name)
        {
            Some(FieldName::Named(name)) => Some(name),
            _ => None,
        }
    }

    fn field_index(&self, db: &'db dyn HirDb, field: FieldKey) -> FieldIndex<'db> {
        match self
            .reflection
            .field(field.variant, field.index)
            .map(|f| f.name)
        {
            Some(FieldName::Named(name)) => FieldIndex::Ident(name),
            Some(FieldName::Positional(idx)) => {
                FieldIndex::Index(IntegerId::new(db, BigUint::from(idx)))
            }
            None => FieldIndex::Index(IntegerId::new(db, BigUint::from(field.index))),
        }
    }

    /// The binder name a [`GenPat::Variant`] pattern introduces for `field`:
    /// `{prefix}_{name}` for record fields, `{prefix}_{index}` for tuple
    /// fields.
    fn binder_ident(
        &self,
        db: &'db dyn HirDb,
        prefix: IdentId<'db>,
        field: FieldKey,
    ) -> IdentId<'db> {
        let suffix = match self
            .reflection
            .field(field.variant, field.index)
            .map(|f| f.name)
        {
            Some(FieldName::Named(name)) => name.data(db).to_string(),
            Some(FieldName::Positional(idx)) => idx.to_string(),
            None => field.index.to_string(),
        };
        IdentId::new(db, format!("{}_{}", prefix.data(db), suffix))
    }
}
