//! Closing declaration-owned dictionaries before constructing an instance.
//!
//! This producer may complete an explicitly absent ordinary obligation. A
//! supplied tree is transported, never replaced by a second selection. The
//! result owns every normalization fact it exposes; no caller index survives.
use salsa::Update;

use super::{EffectProviderSubst, GenericSubst, ImplEnv};
use crate::analysis::{
    HirAnalysisDb,
    ty::{
        const_ty::const_body_assumptions,
        evidence::{BoundOwner, BoundSchemaId, EvidenceList, ProvedGoal, prove},
        normalize::normalize_ty,
        subst::substitute_complete,
        trait_resolution::{PredicateListId, TraitSolveCx},
        ty_check::{BodyOwner, func_signature},
        ty_def::TyFlags,
        visitor::collect_flags,
    },
};
use crate::hir_def::{HirIngot, scope_graph::ScopeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub enum ResidualEvidence {
    /// Closure and contract checkers still own their non-function bound layouts.
    BodySchema,
    GenericOrProjectedInput,
    GenericOrProjectedGoal,
    MissingOrdinaryProof,
    UnboundSuppliedProof,
    DeferredCompletion,
}

#[salsa::interned]
#[derive(Debug)]
pub struct ClosedBodyEvidence<'db> {
    pub schema: BoundSchemaId<'db>,
    pub goals: PredicateListId<'db>,
    pub proofs: EvidenceList<'db>,
    pub dispatch: Option<ProvedGoal<'db>>,
    pub origin: ScopeId<'db>,
}

impl<'db> ClosedBodyEvidence<'db> {
    /// Facts are the proved uses, including their associated equalities and
    /// modes. Implementation heads alone cannot recover these facts.
    pub fn normalization_facts(self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        let mut goals = self.goals(db).list(db).to_vec();
        goals.extend(self.proofs(db).rows(db).iter().map(|row| row.goal));
        goals.extend(self.dispatch(db).map(|dispatch| dispatch.goal));
        PredicateListId::new(db, goals).extend_all_bounds(db)
    }

    pub fn normalization_env(self, db: &'db dyn HirAnalysisDb) -> ImplEnv<'db> {
        ImplEnv::new(
            db,
            self.origin(db),
            self.normalization_facts(db),
            Vec::new(),
        )
    }
}

fn ground(flags: TyFlags) -> bool {
    !flags.intersects(
        TyFlags::HAS_PARAM | TyFlags::HAS_VAR | TyFlags::HAS_INVALID | TyFlags::HAS_PROJECTION,
    )
}

/// Declaration-only schema lookup. It must never ask for the body or instance
/// currently being constructed. Captured layouts are supplied by their checker.
pub(crate) fn declaration_bound_schema<'db>(
    db: &'db dyn HirAnalysisDb,
    owner: BodyOwner<'db>,
) -> Option<BoundSchemaId<'db>> {
    let slots = match owner {
        BodyOwner::Func(func, mode) => func_signature(db, func, mode),
        BodyOwner::Const(_) | BodyOwner::AnonConstBody { .. } => {
            const_body_assumptions(db, owner.scope()).extend_all_bounds(db)
        }
        BodyOwner::Closure { .. }
        | BodyOwner::ContractInit { .. }
        | BodyOwner::ContractRecvArm { .. } => return None,
    };
    Some(BoundSchemaId::new(db, BoundOwner::Body(owner), slots))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub(crate) struct EvidenceCompletion<'db> {
    pub owner: BodyOwner<'db>,
    pub subst: GenericSubst<'db>,
    pub providers: EffectProviderSubst<'db>,
    pub supplied: EvidenceList<'db>,
    pub dispatch: Option<ProvedGoal<'db>>,
    /// Producer context for normalization of the use's already-selected input.
    pub context: ImplEnv<'db>,
}

#[salsa::tracked]
pub(crate) fn close_instance_evidence<'db>(
    db: &'db dyn HirAnalysisDb,
    input: EvidenceCompletion<'db>,
) -> Result<ClosedBodyEvidence<'db>, ResidualEvidence> {
    let schema = declaration_bound_schema(db, input.owner).ok_or(ResidualEvidence::BodySchema)?;
    if !ground(
        collect_flags(db, input.subst.generic_args(db))
            | collect_flags(db, input.providers.providers(db)),
    ) {
        return Err(ResidualEvidence::GenericOrProjectedInput);
    }
    let source = schema.slots(db);
    let goals = match input.subst.mapping(db) {
        Some(subst) => substitute_complete(db, source, subst)
            .map_err(|_| ResidualEvidence::GenericOrProjectedGoal)?,
        None => source,
    };
    let mut facts = input.context.assumptions(db).list(db).to_vec();
    facts.extend(input.context.witnesses(db).iter().copied());
    let context_facts = PredicateListId::new(db, facts);
    let scope = input.context.normalization_scope(db);
    let goals = normalize_ty(db, goals, scope, context_facts);
    if !ground(collect_flags(db, goals)) {
        return Err(ResidualEvidence::GenericOrProjectedGoal);
    }
    let supplied = input
        .supplied
        .complete_deferred(db)
        .ok_or(ResidualEvidence::DeferredCompletion)?;
    let supplied = normalize_ty(db, supplied, scope, context_facts);
    let ordinary = TraitSolveCx::new(db, scope);
    let entries = goals
        .list(db)
        .iter()
        .enumerate()
        .map(|(index, &goal)| {
            let proof = match supplied.get(db, index) {
                Some(proof) => proof,
                None => prove(db, ordinary, goal).ok_or(ResidualEvidence::MissingOrdinaryProof)?,
            };
            if !proof.is_closed(db) || !ground(collect_flags(db, proof)) {
                return Err(ResidualEvidence::UnboundSuppliedProof);
            }
            Ok(Some(proof))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if supplied.rows(db).iter().any(|row| {
        !ground(collect_flags(db, row.goal))
            || !row
                .proof
                .is_some_and(|proof| proof.is_closed(db) && ground(collect_flags(db, proof)))
    }) {
        return Err(ResidualEvidence::UnboundSuppliedProof);
    }
    let dispatch = input
        .dispatch
        .map(|dispatch| {
            let goal = normalize_ty(db, dispatch.goal, scope, context_facts);
            let proof = normalize_ty(db, dispatch.proof, scope, context_facts);
            if !ground(collect_flags(db, goal) | collect_flags(db, proof)) || !proof.is_closed(db) {
                return Err(ResidualEvidence::UnboundSuppliedProof);
            }
            Ok(ProvedGoal { goal, proof })
        })
        .transpose()?;
    let proofs = EvidenceList::of(db, entries).with_rows(db, supplied.rows(db).clone());
    Ok(ClosedBodyEvidence::new(
        db,
        schema,
        goals,
        proofs,
        dispatch,
        scope.ingot(db).root_mod(db).scope(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analysis::{
            semantic::{
                SemanticInstanceKey, get_or_build_semantic_instance, root_semantic_instance_key,
            },
            ty::{
                evidence::{Evidence, EvidenceId},
                ty_check::TypedBody,
            },
        },
        hir_def::{CallableDef, ItemKind},
        test_db::HirAnalysisTestDb,
    };
    use rustc_hash::FxHashSet;

    fn input<'db>(
        db: &'db HirAnalysisTestDb,
        key: SemanticInstanceKey<'db>,
    ) -> EvidenceCompletion<'db> {
        EvidenceCompletion {
            owner: key.owner(db),
            subst: key.subst(db),
            providers: key.effect_providers(db),
            supplied: key.evidence(db),
            dispatch: None,
            context: key.impl_env(db),
        }
    }

    fn compare_body_types<'db>(
        db: &'db HirAnalysisTestDb,
        body: &TypedBody<'db>,
        old: ImplEnv<'db>,
        new: ImplEnv<'db>,
    ) {
        let Some(source) = body.body() else {
            return;
        };
        let facts = |env: ImplEnv<'db>| {
            let mut facts = env.assumptions(db).list(db).to_vec();
            facts.extend(env.witnesses(db));
            facts.extend(body.assumptions().list(db));
            PredicateListId::new(db, facts).extend_all_bounds(db)
        };
        for expr in source.exprs(db).keys() {
            let ty = body.expr_ty(db, expr);
            assert_eq!(
                normalize_ty(db, ty, old.normalization_scope(db), facts(old)),
                normalize_ty(db, ty, new.normalization_scope(db), facts(new)),
                "body expression {expr:?}"
            );
        }
    }

    #[test]
    fn closed_facts_shadow_existing_closure_and_effect_instances() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "closed_facts.fe".into(),
            include_str!("../../../../../../fe/tests/fixtures/fe_test/named_closure_evidence.fe"),
        );
        let (module, _) = db.top_mod(file);
        db.assert_no_diags(module);
        let mut work = module
            .children_non_nested(&db)
            .filter_map(|item| match item {
                ItemKind::Func(func)
                    if CallableDef::Func(func).params(&db).is_empty()
                        && func.effect_requirements(&db).is_empty() =>
                {
                    root_semantic_instance_key(&db, BodyOwner::func(&db, func)).ok()
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut seen = FxHashSet::default();
        let mut closed_count = 0;
        let mut named_count = 0;
        let mut rejected_unbound = false;
        while let Some(key) = work.pop() {
            if !seen.insert(key) {
                continue;
            }
            assert!(seen.len() < 2000, "instance keys must stabilize");
            let instance = get_or_build_semantic_instance(&db, key);
            work.extend(instance.callees(&db).iter().map(|callee| callee.key));
            let Ok(closed) = close_instance_evidence(&db, input(&db, key)) else {
                continue;
            };
            closed_count += 1;
            named_count += usize::from(
                closed
                    .proofs(&db)
                    .entries(&db)
                    .iter()
                    .flatten()
                    .any(|proof| proof.has_named_impl(&db)),
            );
            let derived = closed.normalization_env(&db);
            compare_body_types(&db, key.typed_body(&db), key.impl_env(&db), derived);
            let shadow = SemanticInstanceKey::of(
                &db,
                key.owner(&db),
                key.subst(&db),
                key.effect_providers(&db),
                derived,
                closed.proofs(&db),
            );
            let shadow_instance = get_or_build_semantic_instance(&db, shadow);
            let old_calls = instance.callees(&db);
            let new_calls = shadow_instance.callees(&db);
            assert_eq!(
                old_calls.len(),
                new_calls.len(),
                "closed facts preserve call count"
            );
            for (old, new) in old_calls.iter().zip(new_calls) {
                assert_eq!(
                    old.key.owner(&db),
                    new.key.owner(&db),
                    "closed facts preserve selected body"
                );
                assert_eq!(
                    old.key.subst(&db),
                    new.key.subst(&db),
                    "closed facts preserve selected arguments and captures"
                );
                assert_eq!(
                    old.key.evidence(&db),
                    new.key.evidence(&db),
                    "closed facts preserve chosen dictionaries"
                );
            }
            if !closed.goals(&db).list(&db).is_empty() {
                let leaf = EvidenceId::new(
                    &db,
                    Evidence::Assumption(closed.schema(&db).slot(&db, 0).unwrap()),
                );
                let supplied = EvidenceList::of(&db, vec![Some(leaf)]);
                assert_eq!(
                    close_instance_evidence(
                        &db,
                        EvidenceCompletion {
                            supplied,
                            ..input(&db, key)
                        }
                    ),
                    Err(ResidualEvidence::UnboundSuppliedProof)
                );
                rejected_unbound = true;
            }
        }
        assert!(closed_count > 10, "exercise real ground instances");
        assert!(named_count >= 2, "exercise different selected dictionaries");
        assert!(
            rejected_unbound,
            "a supplied open proof must not be replaced ordinarily"
        );
    }
}
