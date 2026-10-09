use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            const_ty::lexical_const_body_owner,
            fold::{TyFoldable, TyFolder},
            subst::substitute_complete,
            trait_def::{
                ImplementorId, ResolvedMethodInstance, SelectedEvidenceError, SelectedImplEvidence,
                TraitInstId, prove_selected_impl_evidence, resolve_trait_method_instance,
                resolved_method_instance,
            },
            trait_resolution::{PredicateListId, ProofProvisions, Selection, TraitSolveCx},
            ty_check::{BodyOwner, TraitChoiceId, TypedBody, infer_body},
            ty_def::TyId,
            ty_lower::CompleteSubst,
        },
    },
    hir_def::{CallableDef, GenericParamOwner, IdentId, scope_graph::ScopeId},
    semantic::ProviderBinding,
};

#[derive(Clone, Debug)]
pub struct TypedBodyTemplate<'db> {
    pub owner: BodyOwner<'db>,
    pub body: TypedBody<'db>,
}

pub fn typed_body_template<'db>(
    db: &'db dyn HirAnalysisDb,
    owner: BodyOwner<'db>,
) -> TypedBodyTemplate<'db> {
    let typed_body = infer_body(db, owner).1.clone();

    TypedBodyTemplate {
        owner,
        body: typed_body,
    }
}

#[salsa::interned]
#[derive(Debug)]
pub struct GenericSubst<'db> {
    #[return_ref]
    pub mapping: Option<CompleteSubst<'db>>,
}

impl<'db> GenericSubst<'db> {
    pub fn none(db: &'db dyn HirAnalysisDb) -> Self {
        Self::new(db, None)
    }

    pub fn complete(db: &'db dyn HirAnalysisDb, mapping: CompleteSubst<'db>) -> Self {
        Self::new(db, Some(mapping))
    }

    pub fn for_owner(
        db: &'db dyn HirAnalysisDb,
        owner: GenericParamOwner<'db>,
        args: Vec<TyId<'db>>,
    ) -> Self {
        let mapping = CompleteSubst::for_owner(db, owner, args).unwrap_or_else(|error| {
            panic!("invalid semantic substitution for {owner:?}: {error:?}")
        });
        Self::complete(db, mapping)
    }

    /// The substitution for evaluating `owner` with `args`, one per full-schema
    /// slot of its generic context: a function's own schema, or an anonymous
    /// const body's enclosing declaration. Without arguments the body's
    /// parameters stay symbolic: a function takes its identity substitution.
    pub fn for_body_owner(
        db: &'db dyn HirAnalysisDb,
        owner: BodyOwner<'db>,
        args: Vec<TyId<'db>>,
    ) -> Self {
        let generic_owner = match owner {
            BodyOwner::Func(func) if args.is_empty() => {
                return Self::for_owner(
                    db,
                    func.into(),
                    CallableDef::Func(func).params(db).to_vec(),
                );
            }
            BodyOwner::Func(func) => Some(func.into()),
            BodyOwner::AnonConstBody { body, .. } if !args.is_empty() => {
                lexical_const_body_owner(db, body)
            }
            _ => None,
        };
        if let Some(generic_owner) = generic_owner {
            Self::for_owner(db, generic_owner, args)
        } else {
            assert!(
                args.is_empty(),
                "{owner:?} has no generic context for {args:?}"
            );
            Self::none(db)
        }
    }

    pub fn generic_args(self, db: &'db dyn HirAnalysisDb) -> &'db [TyId<'db>] {
        self.mapping(db).as_ref().map_or(&[], CompleteSubst::values)
    }
}

/// A selected application retains the context that established its premises.
/// Every consumer rechecks both that context and its requested application.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct SelectedApplication<'db> {
    goal: TraitInstId<'db>,
    candidate: ImplementorId<'db>,
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
    provisions: ProofProvisions<'db>,
}

impl<'db> SelectedApplication<'db> {
    pub fn from_evidence(evidence: &SelectedImplEvidence<'db>) -> Self {
        Self {
            goal: evidence.goal(),
            candidate: evidence.selected(),
            scope: evidence.context().scope(),
            assumptions: evidence.context().assumptions(),
            provisions: evidence.context().provisions(),
        }
    }

    pub(crate) fn candidate(self) -> ImplementorId<'db> {
        self.candidate
    }

    pub fn goal(self) -> TraitInstId<'db> {
        self.goal
    }
    pub fn scope(self) -> ScopeId<'db> {
        self.scope
    }
    pub fn assumptions(self) -> PredicateListId<'db> {
        self.assumptions
    }
    pub fn origin(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> crate::analysis::ty::trait_def::ImplementorOrigin<'db> {
        self.candidate.origin(db)
    }

    /// Matches the selection slot. Associated equalities still have to pass
    /// `revalidate`; this comparison alone is not proof of a requested goal.
    pub fn matches(self, db: &'db dyn HirAnalysisDb, goal: TraitInstId<'db>) -> bool {
        self.goal.def(db) == goal.def(db) && self.goal.args(db) == goal.args(db)
    }

    pub fn revalidate(
        self,
        db: &'db dyn HirAnalysisDb,
        destination: TraitSolveCx<'db>,
        goal: TraitInstId<'db>,
    ) -> Result<SelectedImplEvidence<'db>, SelectedEvidenceError> {
        if !self.matches(db, goal) {
            return Err(SelectedEvidenceError::NotProven);
        }
        // The origin's premises and provisions are those the choice was made
        // with, so a prerequisite another provision met there is met again.
        prove_selected_impl_evidence(
            db,
            TraitSolveCx::new(db, self.scope)
                .with_assumptions(self.assumptions)
                .with_provisions(self.provisions),
            self.candidate,
            self.goal,
        )?;
        // The choice carries how its prerequisites were met: the provisions
        // it was made under hold wherever it is used, unless the destination
        // provides the same goal itself.
        let destination =
            destination.with_provisions(self.provisions.within(db, destination.provisions()));
        prove_selected_impl_evidence(db, destination, self.candidate, goal)
    }
}

/// Resolve one trait method through the selected application in scope, or
/// through ordinary lookup when no selection covers the goal. An invalid
/// selection must never fall back to an unrelated ordinary implementation.
pub fn resolve_trait_method_with_selections<'db>(
    db: &'db dyn HirAnalysisDb,
    context: TraitSolveCx<'db>,
    goal: TraitInstId<'db>,
    method: IdentId<'db>,
    impl_env: ImplEnv<'db>,
) -> Result<Selection<ResolvedMethodInstance<'db>>, SelectedEvidenceError> {
    if let Some(selection) = impl_env.selection_for(db, goal) {
        let evidence = selection.revalidate(db, context, goal)?;
        Ok(resolved_method_instance(
            db,
            context,
            evidence.resolved_instance(),
            method,
        ))
    } else {
        let resolved = resolve_trait_method_instance(db, context, goal, method);
        // Only a witness answers this concrete goal: evidence opened by a
        // caller, whose operand evaluation passes in its place.
        let witnessed_only = match &resolved {
            Selection::NotFound => true,
            Selection::Unique(method) => matches!(
                method.resolved().selected().origin(db),
                crate::analysis::ty::trait_def::ImplementorOrigin::Assumption
            ),
            Selection::Ambiguous(_) => false,
        };
        if witnessed_only
            && !goal
                .args(db)
                .iter()
                .any(|ty| ty.has_param(db) || ty.has_var(db))
            && matches!(
                impl_env.provision_for(db, goal),
                Some(crate::semantic::Provision::Bound(_))
            )
        {
            provisional_opened_method(db, context, goal, method)
        } else {
            Ok(resolved)
        }
    }
}

/// The provisional static callee of a goal answered only by opened evidence:
/// the first visible implementation, ordinary or named, that proves the goal
/// here, as where an implementation answers it. Evaluation validates the
/// evidence's own choice and dispatches to it.
pub(crate) fn provisional_opened_method<'db>(
    db: &'db dyn HirAnalysisDb,
    context: TraitSolveCx<'db>,
    goal: TraitInstId<'db>,
    method: IdentId<'db>,
) -> Result<Selection<ResolvedMethodInstance<'db>>, SelectedEvidenceError> {
    let ingot = context.scope().ingot(db);
    crate::analysis::ty::trait_def::impls_for_trait_def(db, ingot, goal.def(db))
        .iter()
        .find_map(|&candidate| prove_selected_impl_evidence(db, context, candidate, goal).ok())
        .map(|proof| resolved_method_instance(db, context, proof.resolved_instance(), method))
        .ok_or(SelectedEvidenceError::NotProven)
}

impl<'db> crate::analysis::ty::visitor::TyVisitable<'db> for SelectedApplication<'db> {
    fn visit_with<V: crate::analysis::ty::visitor::TyVisitor<'db> + ?Sized>(
        &self,
        visitor: &mut V,
    ) {
        self.goal.visit_with(visitor);
        self.assumptions.visit_with(visitor);
        self.provisions.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for SelectedApplication<'db> {
    fn super_fold_with<F: TyFolder<'db>>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self {
        Self {
            goal: self.goal.fold_with(db, folder),
            assumptions: self.assumptions.fold_with(db, folder),
            provisions: self.provisions.fold_with(db, folder),
            ..self
        }
    }
}

#[salsa::interned(constructor = new_with_choices)]
#[derive(Debug)]
pub struct ImplEnv<'db> {
    pub normalization_scope: ScopeId<'db>,
    pub assumptions: PredicateListId<'db>,
    #[return_ref]
    pub witnesses: Vec<TraitInstId<'db>>,
    #[return_ref]
    pub selections: Vec<SelectedApplication<'db>>,
    /// Arguments to the owner's declared bound slots refer to accepted records.
    /// Slot keys survive substitution even when their goals become identical.
    #[return_ref]
    pub bound_choices: Vec<(usize, TraitChoiceId<'db>)>,
}

/// The premises and normalization scope of a position context: the part of
/// the context a premise-only query (normalization, `Copy`, provider layout)
/// reads, and so the part such a query is keyed by.
///
/// Named selections are deliberately excluded: a named choice is not proof
/// authority, and evidence opened at the position is already among the
/// assumptions. Method lookup, where a named choice shadows matching goals,
/// needs a different view and gets its own named projection.
///
/// Equality and hashing are those of the `(scope, assumptions)` pair it
/// replaces, so query keys are unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct SolverPremises<'db> {
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
}

impl<'db> SolverPremises<'db> {
    /// Transitional: premises built from a scope and assumption list that do
    /// not yet come from a position context. Each caller is a site still to be
    /// given the context of its position; `.github/check_solver_premises_budget.py`
    /// counts them and the count may only decrease.
    pub fn from_unpositioned_pair(scope: ScopeId<'db>, assumptions: PredicateListId<'db>) -> Self {
        Self { scope, assumptions }
    }

    pub fn scope(self) -> ScopeId<'db> {
        self.scope
    }

    pub fn assumptions(self) -> PredicateListId<'db> {
        self.assumptions
    }

    /// The solver view of these premises.
    pub fn solver_context(self, db: &'db dyn HirAnalysisDb) -> TraitSolveCx<'db> {
        TraitSolveCx::at_position(db, self)
    }
}

impl<'db> crate::analysis::ty::visitor::TyVisitable<'db> for SolverPremises<'db> {
    fn visit_with<V: crate::analysis::ty::visitor::TyVisitor<'db> + ?Sized>(
        &self,
        visitor: &mut V,
    ) {
        self.assumptions.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for SolverPremises<'db> {
    fn super_fold_with<F: TyFolder<'db>>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self {
        Self {
            scope: self.scope,
            assumptions: self.assumptions.fold_with(db, folder),
        }
    }
}

impl<'db> ImplEnv<'db> {
    /// The premises and normalization scope of this position context.
    pub fn premises(self, db: &'db dyn HirAnalysisDb) -> SolverPremises<'db> {
        SolverPremises {
            scope: self.normalization_scope(db),
            assumptions: self.assumptions(db),
        }
    }

    /// Read a declared field before ordinary const evaluation loses its choice.
    pub fn adt_field_type(
        self,
        db: &'db dyn HirAnalysisDb,
        adt: crate::analysis::ty::adt_def::AdtDef<'db>,
        variant: usize,
        field: usize,
        args: &[TyId<'db>],
    ) -> TyId<'db> {
        let source = crate::analysis::ty::adt_def::instantiate_adt_field_source_for_concrete_demand(
            db, adt, variant, field, args,
        );
        let source = self.contextualize_const_types(db, source);
        crate::analysis::ty::normalize::normalize_ty(db, source, self.premises(db))
    }

    /// Solver premises, normalization scope and provisions come from one
    /// position context.
    pub fn solver_context(self, db: &'db dyn HirAnalysisDb) -> TraitSolveCx<'db> {
        self.premises(db)
            .solver_context(db)
            .with_provisions(self.proof_provisions(db))
    }

    /// The provisions proof search reads at this position: for each goal, the
    /// visible provision's answer, a named choice's implementation or `None`
    /// for opened evidence and transported witnesses. Scope, premises and the
    /// choices' captured origins are not read, so they are not in the key.
    pub(crate) fn proof_provisions(self, db: &'db dyn HirAnalysisDb) -> ProofProvisions<'db> {
        use crate::semantic::Provision;
        let mut entries: Vec<(TraitInstId<'db>, Option<ImplementorId<'db>>)> = Vec::new();
        let goals = self
            .witnesses(db)
            .iter()
            .copied()
            .chain(self.selections(db).iter().map(|selection| selection.goal()));
        for goal in goals {
            let entry = match self.provision_for(db, goal) {
                Some(Provision::Named(selection)) => {
                    (selection.goal(), Some(selection.candidate()))
                }
                Some(provision) => (provision.goal(), None),
                None => continue,
            };
            if !entries.contains(&entry) {
                entries.push(entry);
            }
        }
        ProofProvisions::new(db, entries)
    }

    /// Premises for member lookup at this position. Opened evidence is a
    /// premise; a named choice shadows the matching goal so lookup sees the
    /// chosen application, without becoming proof authority for anything else.
    /// This is the projection the type checker's chain gives lookup.
    pub(crate) fn lookup_premises(self, db: &'db dyn HirAnalysisDb) -> SolverPremises<'db> {
        SolverPremises {
            scope: self.normalization_scope(db),
            assumptions: crate::semantic::lookup_assumptions(
                db,
                self.assumptions(db),
                [],
                self.selections(db).iter().copied(),
            ),
        }
    }

    /// Attach this restricted position context to uncaptured signature constants.
    /// Values and constants that already captured their choice keep that identity.
    pub(crate) fn contextualize_const_types(
        self,
        db: &'db dyn HirAnalysisDb,
        ty: TyId<'db>,
    ) -> TyId<'db> {
        if self.selections(db).is_empty() {
            return ty;
        }
        self.contextualize_const_types_with(db, ty, |goal| self.provision_for(db, goal))
    }

    pub(crate) fn contextualize_const_types_with(
        self,
        db: &'db dyn HirAnalysisDb,
        ty: TyId<'db>,
        lookup: impl Fn(TraitInstId<'db>) -> Option<crate::semantic::Provision<'db>>,
    ) -> TyId<'db> {
        if self.selections(db).is_empty() && self.witnesses(db).is_empty() {
            return ty;
        }
        use crate::analysis::ty::ty_def::TyData;
        struct SignatureConsts<'db, F> {
            context: crate::analysis::semantic::ImplEnv<'db>,
            lookup: F,
        }
        impl<'db, F: Fn(TraitInstId<'db>) -> Option<crate::semantic::Provision<'db>>> TyFolder<'db>
            for SignatureConsts<'db, F>
        {
            fn fold_assoc_const_use(
                &mut self,
                db: &'db dyn HirAnalysisDb,
                use_: crate::analysis::ty::assoc_const::AssocConstUse<'db>,
            ) -> crate::analysis::ty::assoc_const::AssocConstUse<'db> {
                let use_ = use_.super_fold_with(db, self);
                if use_.provision().is_some() {
                    return use_;
                }
                let Some(provision) = (self.lookup)(use_.inst()) else {
                    return use_;
                };
                if matches!(provision, crate::semantic::Provision::Bound(_)) {
                    return use_;
                }
                use_.with_premises(self.context.premises(db))
                    .with_provision(Some(provision))
            }

            fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
                let ty = ty.super_fold_with(db, self);
                if let TyData::ConstTy(value) = ty.data(db) {
                    return TyId::const_ty(
                        db,
                        value.with_provision_at(db, self.context, &self.lookup),
                    );
                }
                ty
            }
        }
        ty.fold_with(
            db,
            &mut SignatureConsts {
                context: self,
                lookup,
            },
        )
    }

    pub fn new_with_selections(
        db: &'db dyn HirAnalysisDb,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
        witnesses: Vec<TraitInstId<'db>>,
        selections: Vec<SelectedApplication<'db>>,
    ) -> Self {
        Self::new_with_choices(db, scope, assumptions, witnesses, selections, Vec::new())
    }

    pub fn new(
        db: &'db dyn HirAnalysisDb,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
        witnesses: Vec<TraitInstId<'db>>,
    ) -> Self {
        Self::new_with_selections(db, scope, assumptions, witnesses, Vec::new())
    }

    pub fn with_selections(
        self,
        db: &'db dyn HirAnalysisDb,
        selections: Vec<SelectedApplication<'db>>,
    ) -> Self {
        if *self.selections(db) == selections {
            return self;
        }
        // Visibility is established by the source chain. Transport preserves
        // its order; substitution must not merge accepted authored choices.
        Self::new_with_choices(
            db,
            self.normalization_scope(db),
            self.assumptions(db),
            self.witnesses(db).clone(),
            selections,
            self.bound_choices(db).clone(),
        )
    }

    /// The provisions of this context, other than `goal`'s own, that proving
    /// `goal` reaches through impl prerequisites, as a context over
    /// `premises`; `None` when there are none.
    pub(crate) fn reached_from(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
        premises: SolverPremises<'db>,
    ) -> Option<Self> {
        let requirements = PredicateListId::new(db, vec![goal]);
        let ingot = premises.scope().ingot(db);
        let reached = |provided: TraitInstId<'db>| {
            !(provided.def(db) == goal.def(db) && provided.args(db) == goal.args(db))
                && crate::core::semantic::requirements_reach_goal(db, ingot, requirements, provided)
        };
        let witnesses = self
            .witnesses(db)
            .iter()
            .copied()
            .filter(|provided| reached(*provided))
            .collect::<Vec<_>>();
        let selections = self
            .selections(db)
            .iter()
            .copied()
            .filter(|selection| reached(selection.goal()))
            .collect::<Vec<_>>();
        (!witnesses.is_empty() || !selections.is_empty()).then(|| {
            Self::new_with_selections(
                db,
                premises.scope(),
                premises.assumptions(),
                witnesses,
                selections,
            )
        })
    }

    /// This context with `goals` also answered by opened evidence; existing
    /// witnesses and choices are kept.
    pub(crate) fn with_witnesses(
        self,
        db: &'db dyn HirAnalysisDb,
        goals: Vec<TraitInstId<'db>>,
    ) -> Self {
        if goals.iter().all(|goal| self.witnesses(db).contains(goal)) {
            return self;
        }
        let mut witnesses = self.witnesses(db).clone();
        witnesses.extend(
            goals
                .into_iter()
                .filter(|goal| !self.witnesses(db).contains(goal)),
        );
        Self::new_with_choices(
            db,
            self.normalization_scope(db),
            self.assumptions(db),
            witnesses,
            self.selections(db).clone(),
            self.bound_choices(db).clone(),
        )
    }

    /// Read the transported declaration view. A bound witness retains a goal;
    /// lexical evidence operands are captured from their source record before
    /// transport, and must never be reconstructed from this goal-only view.
    pub(crate) fn provision_for(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> Option<crate::semantic::Provision<'db>> {
        use crate::semantic::Provision;
        crate::core::semantic::lookup_trait_goal(
            db,
            goal,
            self.witnesses(db)
                .iter()
                .copied()
                .map(Provision::Bound)
                .chain(self.selections(db).iter().copied().map(Provision::Named)),
            |provision| provision.goal(),
        )
    }

    pub fn selection_for(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> Option<SelectedApplication<'db>> {
        match self.provision_for(db, goal)? {
            crate::semantic::Provision::Named(selection) => Some(selection),
            _ => None,
        }
    }

    pub fn empty(db: &'db dyn HirAnalysisDb, normalization_scope: ScopeId<'db>) -> Self {
        Self::new(
            db,
            normalization_scope,
            PredicateListId::empty_list(db),
            Vec::new(),
        )
    }
}

impl<'db> crate::analysis::ty::visitor::TyVisitable<'db> for ImplEnv<'db> {
    fn visit_with<V: crate::analysis::ty::visitor::TyVisitor<'db> + ?Sized>(
        &self,
        visitor: &mut V,
    ) {
        self.assumptions(visitor.db()).visit_with(visitor);
        self.witnesses(visitor.db()).visit_with(visitor);
        self.selections(visitor.db()).visit_with(visitor);
        for (_, choice) in self.bound_choices(visitor.db()) {
            choice.visit_with(visitor);
        }
    }
}

impl<'db> TyFoldable<'db> for ImplEnv<'db> {
    fn super_fold_with<F: TyFolder<'db>>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self {
        Self::new_with_choices(
            db,
            folder.fold_scope(self.normalization_scope(db)),
            self.assumptions(db).fold_with(db, folder),
            self.witnesses(db).clone().fold_with(db, folder),
            self.selections(db).clone().fold_with(db, folder),
            self.bound_choices(db)
                .iter()
                .map(|&(slot, choice)| (slot, choice.fold_with(db, folder)))
                .collect::<Vec<_>>(),
        )
    }
}

#[salsa::interned]
#[derive(Debug)]
pub struct EffectProviderSubst<'db> {
    #[return_ref]
    pub providers: Vec<ProviderBinding<'db>>,
}

impl<'db> EffectProviderSubst<'db> {
    pub fn empty(db: &'db dyn HirAnalysisDb) -> Self {
        Self::new(db, Vec::new())
    }
}

pub fn instantiate_typed_body<'db>(
    db: &'db dyn HirAnalysisDb,
    template: TypedBodyTemplate<'db>,
    subst: GenericSubst<'db>,
) -> TypedBody<'db> {
    if let BodyOwner::Func(func) = template.owner {
        let mapping = subst
            .mapping(db)
            .as_ref()
            .expect("function body requires a semantic substitution domain");
        assert_eq!(
            mapping.domain().schema(db).owner(db),
            func.into(),
            "semantic substitution belongs to a different function",
        );
    }
    subst
        .mapping(db)
        .as_ref()
        .map_or(template.body.clone(), |mapping| {
            substitute_complete(db, template.body, mapping)
                .expect("typed body must use its semantic substitution domain")
        })
}
