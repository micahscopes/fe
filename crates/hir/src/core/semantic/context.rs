//! Owned position contexts and captured applications.
//! Raw construction and edits belong here; consumers read position views.
use crate::{
    analysis::{
        HirAnalysisDb,
        semantic::{SemanticInstance, lower::BindingRoleMode},
        ty::{
            fold::{TyFoldable, TyFolder},
            trait_def::{
                ImplementorId, ResolvedMethodInstance, SelectedEvidenceError, SelectedImplEvidence,
                TraitInstId, prove_selected_impl_evidence, resolved_method_instance,
            },
            trait_resolution::{PredicateListId, ProofProvisions, Selection, TraitSolveCx},
            ty_check::{BodyOwner, TraitChoiceId},
            ty_def::TyId,
        },
    },
    hir_def::{IdentId, scope_graph::ScopeId},
};

/// Only the semantic owner can authorize raw solver construction.
/// The private field prevents consumers from creating this zero-sized token.
pub(crate) struct ContextAuthority(());

pub(super) fn context_authority() -> ContextAuthority {
    ContextAuthority(())
}

/// A selected application retains the context that established its premises.
/// Every consumer rechecks both that context and its requested application.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, salsa::Update)]
pub struct SelectedApplication<'db> {
    goal: TraitInstId<'db>,
    candidate: ImplementorId<'db>,
    context: TraitSolveCx<'db>,
}

impl<'db> SelectedApplication<'db> {
    pub fn from_evidence(evidence: &SelectedImplEvidence<'db>) -> Self {
        Self {
            goal: evidence.goal(),
            candidate: evidence.selected(),
            context: evidence.context(),
        }
    }

    pub(crate) fn candidate(self) -> ImplementorId<'db> {
        self.candidate
    }

    pub fn goal(self) -> TraitInstId<'db> {
        self.goal
    }
    pub fn scope(self) -> ScopeId<'db> {
        self.context.scope()
    }
    pub fn assumptions(self) -> PredicateListId<'db> {
        self.context.assumptions()
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
        prove_selected_impl_evidence(db, self.context, self.candidate, self.goal)?;
        // The choice carries how its prerequisites were met: the provisions
        // it was made under hold wherever it is used, unless the destination
        // provides the same goal itself.
        let destination =
            destination.transported_destination(db, self.context, context_authority());
        prove_selected_impl_evidence(db, destination, self.candidate, goal)
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
        self.context.assumptions().visit_with(visitor);
        self.context.provisions().visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for SelectedApplication<'db> {
    fn super_fold_with<F: TyFolder<'db>>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self {
        Self {
            goal: self.goal.fold_with(db, folder),
            context: self
                .context
                .fold_at_captured_origin(db, folder, context_authority()),
            ..self
        }
    }
}

// Salsa gives its generated constructor the storage type's visibility.
// Keep that type private; the public context exposes only owned operations.
mod storage {
    use super::*;
    #[salsa::interned]
    #[derive(Debug)]
    pub(super) struct ImplEnv<'db> {
        pub normalization_scope: ScopeId<'db>,
        pub assumptions: PredicateListId<'db>,
        #[return_ref]
        pub openings: Vec<crate::semantic::Provision<'db>>,
        #[return_ref]
        pub selections: Vec<SelectedApplication<'db>>,
        /// Arguments to the owner's declared bound slots refer to accepted records.
        /// Slot keys survive substitution even when their goals become identical.
        #[return_ref]
        pub bound_choices: Vec<(usize, TraitChoiceId<'db>)>,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub struct ImplEnv<'db>(storage::ImplEnv<'db>);

impl std::fmt::Debug for ImplEnv<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<'db> ImplEnv<'db> {
    pub fn normalization_scope(self, db: &'db dyn HirAnalysisDb) -> ScopeId<'db> {
        self.0.normalization_scope(db)
    }
    pub fn assumptions(self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        self.0.assumptions(db)
    }
    pub fn witnesses(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl ExactSizeIterator<Item = TraitInstId<'db>> + DoubleEndedIterator {
        self.openings(db).iter().map(|provision| provision.goal())
    }
    /// Source evidence retains its operand; transported declaration witnesses
    /// are Bound entries. The goal view above is only a projection.
    pub(crate) fn openings(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> &'db Vec<crate::semantic::Provision<'db>> {
        self.0.openings(db)
    }
    pub fn selections(self, db: &'db dyn HirAnalysisDb) -> &'db Vec<SelectedApplication<'db>> {
        self.0.selections(db)
    }
    pub fn bound_choices(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> &'db Vec<(usize, TraitChoiceId<'db>)> {
        self.0.bound_choices(db)
    }

    pub(super) fn new_with_choices(
        db: &'db dyn HirAnalysisDb,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
        witnesses: Vec<TraitInstId<'db>>,
        selections: Vec<SelectedApplication<'db>>,
        bound_choices: Vec<(usize, TraitChoiceId<'db>)>,
    ) -> Self {
        Self::new_with_openings(
            db,
            scope,
            assumptions,
            witnesses
                .into_iter()
                .map(crate::semantic::Provision::Bound)
                .collect(),
            selections,
            bound_choices,
        )
    }

    pub(super) fn new_with_openings(
        db: &'db dyn HirAnalysisDb,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
        openings: Vec<crate::semantic::Provision<'db>>,
        selections: Vec<SelectedApplication<'db>>,
        bound_choices: Vec<(usize, TraitChoiceId<'db>)>,
    ) -> Self {
        Self(storage::ImplEnv::new(
            db,
            scope,
            assumptions,
            openings,
            selections,
            bound_choices,
        ))
    }
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

    /// Local-value layout schemas describe already lowered declaration topology.
    /// They do not inherit body bounds or lexical selections for expansion.
    pub(crate) fn for_value_layout_schema(
        db: &'db dyn HirAnalysisDb,
        body: crate::hir_def::Body<'db>,
    ) -> Self {
        Self {
            scope: body.scope(),
            assumptions: PredicateListId::empty_list(db),
        }
    }

    /// Completed callable inputs include the declaration's effect-provider
    /// premises. Earlier header/input formation must not use this projection.
    pub(crate) fn for_completed_callable_input(
        db: &'db dyn HirAnalysisDb,
        func: crate::hir_def::Func<'db>,
    ) -> Self {
        Self {
            scope: func.scope(),
            assumptions: func.assumptions(db),
        }
    }

    /// Body normalization keeps the lowering stage's premises and the
    /// actual body scope. Provisional lowering must not read final effects.
    pub(crate) fn for_body_lowering(
        db: &'db dyn HirAnalysisDb,
        instance: crate::analysis::semantic::SemanticInstance<'db>,
        mode: BindingRoleMode,
    ) -> Self {
        Self {
            scope: instance
                .key(db)
                .typed_body(db)
                .body()
                .expect("body lowering has a body")
                .scope(),
            ..Self::for_normalization(db, instance, mode)
        }
    }

    /// Read the fixed final-body normalization premises of this instance.
    pub fn for_normalized_body(
        db: &'db dyn HirAnalysisDb,
        instance: SemanticInstance<'db>,
    ) -> Self {
        Self::for_normalization(db, instance, BindingRoleMode::Final)
    }

    /// Normalized-body queries retain the declaration normalization scope
    /// and the same final or provisional premises as their admission stage.
    pub(crate) fn for_normalization(
        db: &'db dyn HirAnalysisDb,
        instance: crate::analysis::semantic::SemanticInstance<'db>,
        mode: BindingRoleMode,
    ) -> Self {
        Self {
            scope: instance.key(db).owner(db).scope(),
            assumptions: match mode {
                BindingRoleMode::Final => instance.assumptions(db),
                BindingRoleMode::Provisional => {
                    crate::analysis::semantic::semantic_instance_base_assumptions_for_key(
                        db,
                        instance.key(db),
                    )
                }
            },
        }
    }

    pub fn scope(self) -> ScopeId<'db> {
        self.scope
    }

    pub fn assumptions(self) -> PredicateListId<'db> {
        self.assumptions
    }

    /// Premise-only questions (RD108); no prover or mutation is exposed.
    pub(crate) fn queries(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> crate::analysis::ty::trait_resolution::ProofQueries<'db> {
        crate::analysis::ty::trait_resolution::ProofQueries::from_premises(db, self)
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
        TraitSolveCx::at_position(
            db,
            self.premises(db),
            self.proof_provisions(db),
            context_authority(),
        )
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
        if self.selections(db).is_empty() && self.witnesses(db).len() == 0 {
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
                // A signature constant can read a choice through its impl's
                // prerequisites even when its own goal has no provision.
                let use_ = use_.with_dependent_provisions(db, self.context);
                let use_ = if use_.dependent_context().is_some() {
                    use_.with_premises(self.context.premises(db))
                } else {
                    use_
                };
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

    pub(super) fn new_with_selections(
        db: &'db dyn HirAnalysisDb,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
        witnesses: Vec<TraitInstId<'db>>,
        selections: Vec<SelectedApplication<'db>>,
    ) -> Self {
        Self::new_with_choices(db, scope, assumptions, witnesses, selections, Vec::new())
    }

    pub(super) fn new(
        db: &'db dyn HirAnalysisDb,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
        witnesses: Vec<TraitInstId<'db>>,
    ) -> Self {
        Self::new_with_selections(db, scope, assumptions, witnesses, Vec::new())
    }

    /// Enter the named application already accepted by the enclosing checker.
    /// Its frame shadows the same goal without adding a proof premise.
    pub(crate) fn within_named_application(
        self,
        db: &'db dyn HirAnalysisDb,
        application: SelectedApplication<'db>,
    ) -> Self {
        self.within_named_frame(db, super::Frame::from_named_applications(vec![application]))
    }

    /// Enter an accepted named frame. Other visible slots remain in force;
    /// a new slot shadows the preceding frame without becoming a premise.
    pub(crate) fn within_named_frame(
        self,
        db: &'db dyn HirAnalysisDb,
        frame: super::Frame<'db>,
    ) -> Self {
        let incoming = frame.named_applications().collect::<Vec<_>>();
        let mut selections = self.selections(db).clone();
        selections.retain(|previous| {
            !incoming
                .iter()
                .any(|application| previous.matches(db, application.goal()))
        });
        selections.extend(incoming);
        self.with_selections(db, selections)
    }

    pub(super) fn with_selections(
        self,
        db: &'db dyn HirAnalysisDb,
        selections: Vec<SelectedApplication<'db>>,
    ) -> Self {
        self.with_call_choices(db, selections, self.bound_choices(db).clone())
    }

    /// Transport actual call choices without rebuilding the position's premises.
    pub(super) fn with_call_choices(
        self,
        db: &'db dyn HirAnalysisDb,
        selections: Vec<SelectedApplication<'db>>,
        bound_choices: Vec<(usize, TraitChoiceId<'db>)>,
    ) -> Self {
        if *self.selections(db) == selections && *self.bound_choices(db) == bound_choices {
            return self;
        }
        // Visibility is established by the source chain. Transport preserves
        // its order; substitution must not merge accepted authored choices.
        Self::new_with_openings(
            db,
            self.normalization_scope(db),
            self.assumptions(db),
            self.openings(db).clone(),
            selections,
            bound_choices,
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
            .openings(db)
            .iter()
            .copied()
            .filter(|provided| reached(provided.goal()))
            .collect::<Vec<_>>();
        let selections = self
            .selections(db)
            .iter()
            .copied()
            .filter(|selection| reached(selection.goal()))
            .collect::<Vec<_>>();
        (!witnesses.is_empty() || !selections.is_empty()).then(|| {
            Self::new_with_openings(
                db,
                premises.scope(),
                premises.assumptions(),
                witnesses,
                selections,
                Vec::new(),
            )
        })
    }

    /// Refresh caller-answered prerequisites while retaining the source's
    /// explicit choices and evidence operands, including coinciding slots.
    pub(crate) fn retain_captured_const_choices(
        self,
        db: &'db dyn HirAnalysisDb,
        captured: Option<Self>,
    ) -> Self {
        let Some(captured) = captured else {
            return self;
        };
        let fixed = |goal: TraitInstId<'db>| {
            captured
                .selections(db)
                .iter()
                .any(|choice| choice.matches(db, goal))
                || captured
                    .openings(db)
                    .iter()
                    .any(|entry| entry.evidence_operand().is_some() && entry.goal() == goal)
        };
        let mut openings = captured
            .openings(db)
            .iter()
            .copied()
            .filter(|entry| entry.evidence_operand().is_some())
            .collect::<Vec<_>>();
        openings.extend(
            self.openings(db)
                .iter()
                .copied()
                .filter(|entry| !fixed(entry.goal())),
        );
        let mut selections = captured.selections(db).clone();
        selections.extend(
            self.selections(db)
                .iter()
                .copied()
                .filter(|choice| !fixed(choice.goal())),
        );
        Self::new_with_openings(
            db,
            self.normalization_scope(db),
            self.assumptions(db),
            openings,
            selections,
            self.bound_choices(db).clone(),
        )
    }

    /// Read the provision held by this context. Source evidence retains its
    /// recorded operand; transported declaration witnesses retain only a goal.
    /// A missing source operand must never be reconstructed from that goal.
    pub(crate) fn provision_for(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> Option<crate::semantic::Provision<'db>> {
        use crate::semantic::Provision;
        crate::core::semantic::lookup_trait_goal(
            db,
            goal,
            self.openings(db)
                .iter()
                .copied()
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

    /// Open a checked operand in the callee's frame, replacing both its
    /// named answer and any bound argument answered by this same operand.
    pub(crate) fn open_application(
        self,
        db: &'db dyn HirAnalysisDb,
        application: SelectedApplication<'db>,
        proof: &SelectedImplEvidence<'db>,
    ) -> Self {
        let goal = application.goal();
        let supplied = SelectedApplication::from_evidence(proof);
        let mut selections = self.selections(db).clone();
        selections.retain(|inherited| !inherited.matches(db, goal));
        selections.push(application);
        let mut bounds = self.bound_choices(db).clone();
        for (_, choice) in &mut bounds {
            if choice.data(db).goal() == goal {
                *choice = TraitChoiceId::for_selected_application(db, supplied);
            }
        }
        let openings = self
            .openings(db)
            .iter()
            .copied()
            .map(|provision| {
                if application.matches(db, provision.goal()) {
                    crate::semantic::Provision::Bound(provision.goal())
                } else {
                    provision
                }
            })
            .collect();
        Self::new_with_openings(
            db,
            self.normalization_scope(db),
            self.assumptions(db),
            openings,
            selections,
            bounds,
        )
    }

    /// A constant opened through a checked operand uses that proof's
    /// premises, retaining the constant read's normalization origin.
    pub(crate) fn opened_const_caller(
        db: &'db dyn HirAnalysisDb,
        use_: crate::analysis::ty::assoc_const::AssocConstUse<'db>,
        application: SelectedApplication<'db>,
        proof: &SelectedImplEvidence<'db>,
    ) -> Self {
        Self::new_with_selections(
            db,
            use_.origin_scope(),
            proof.context().assumptions(),
            vec![proof.goal()],
            use_.dependent_context()
                .into_iter()
                .flat_map(|context| context.selections(db).iter().copied())
                .chain([application])
                .collect(),
        )
    }

    /// A method dispatched through an opened operand carries its checked
    /// premises and the callee frame's accepted named choices.
    pub(crate) fn opened_method_caller(
        self,
        db: &'db dyn HirAnalysisDb,
        method: crate::hir_def::Func<'db>,
        proof: &SelectedImplEvidence<'db>,
    ) -> Self {
        Self::new_with_selections(
            db,
            method.scope(),
            proof.context().assumptions(),
            vec![proof.goal()],
            self.selections(db).clone(),
        )
    }

    /// Caller contribution captured by an associated-constant read. Only
    /// provisions reached by that read are transported into its body.
    pub(crate) fn associated_const_caller(
        db: &'db dyn HirAnalysisDb,
        use_: crate::analysis::ty::assoc_const::AssocConstUse<'db>,
        selection: Option<SelectedApplication<'db>>,
    ) -> Self {
        Self::new_with_openings(
            db,
            use_.origin_scope(),
            use_.assumptions(),
            use_.dependent_context()
                .map(|context| context.openings(db).clone())
                .unwrap_or_default()
                .into_iter()
                .chain([crate::semantic::Provision::Bound(use_.inst())])
                .collect(),
            use_.dependent_context()
                .map(|context| context.selections(db).clone())
                .unwrap_or_default()
                .into_iter()
                .chain(selection)
                .collect(),
            Vec::new(),
        )
    }

    /// An inherent-constant read retains its authored premise stage.
    pub(crate) fn inherent_const_caller(
        db: &'db dyn HirAnalysisDb,
        use_: crate::analysis::ty::assoc_const::InherentConstUse<'db>,
    ) -> Self {
        Self::new(db, use_.origin_scope(), use_.assumptions(), Vec::new())
    }

    /// Transport the premise stage captured by type lowering into its
    /// constant destination. Reading completed destination bounds here would
    /// re-enter the header currently being lowered.
    pub(crate) fn lowered_const_caller(
        db: &'db dyn HirAnalysisDb,
        lowering: &crate::analysis::ty::const_ty::LoweringContext<'db>,
        destination: ScopeId<'db>,
    ) -> Option<Self> {
        let context = lowering.selection_context()?;
        let premises = context.assumptions(db);
        Some(Self::new_with_openings(
            db,
            destination,
            premises,
            context.openings(db).clone(),
            context.selections(db).clone(),
            context.bound_choices(db).clone(),
        ))
    }

    /// Project a checked callee transition from its source position. Context
    /// independence has already been proved against the accepted answers;
    /// witnesses and normalization origin follow the actual body transition.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn callee_caller(
        self,
        db: &'db dyn HirAnalysisDb,
        owner: BodyOwner<'db>,
        callable: &crate::analysis::ty::ty_check::Callable<'db>,
        body_function: crate::hir_def::Func<'db>,
        selected: Option<crate::analysis::ty::trait_def::ResolvedImplInstance<'db>>,
        independent: bool,
        nominal_witness_required: bool,
        selections: Vec<SelectedApplication<'db>>,
        bounds: Vec<(usize, TraitChoiceId<'db>)>,
    ) -> Self {
        use crate::{
            analysis::ty::trait_def::ImplementorOrigin,
            hir_def::{CallableDef, HirIngot},
        };
        let CallableDef::Func(nominal_function) = callable.callable_def() else {
            unreachable!("a checked function callee must have a function definition");
        };
        let selected_impl = selected.is_some_and(|resolved| {
            !matches!(
                resolved.selected().origin(db),
                ImplementorOrigin::Assumption
            )
        });
        let mut witnesses: indexmap::IndexSet<_> = if independent {
            indexmap::IndexSet::new()
        } else {
            self.witnesses(db).collect()
        };
        if let Some(witness) = callable.trait_inst().filter(|_| nominal_witness_required) {
            witnesses.insert(witness);
        }
        if let Some(resolved) = selected {
            witnesses.insert(resolved.trait_inst());
        }
        let normalization_scope = if independent {
            // Retain the caller's ingot visibility for context-free bodies.
            self.normalization_scope(db).ingot(db).root_mod(db).scope()
        } else if selected_impl {
            owner.scope()
        } else {
            self.normalization_scope(db)
        };
        Self::new_with_choices(
            db,
            normalization_scope,
            if independent {
                PredicateListId::empty_list(db)
            } else {
                self.assumptions(db)
            },
            witnesses.into_iter().collect(),
            selections,
            if body_function == nominal_function {
                bounds
            } else {
                Vec::new()
            },
        )
    }

    /// Unit controls vary context identities deliberately. This fixture
    /// constructor is absent from the compiler's production API.
    #[cfg(test)]
    pub(crate) fn test_fixture(
        db: &'db dyn HirAnalysisDb,
        premises: SolverPremises<'db>,
        witnesses: Vec<TraitInstId<'db>>,
        selections: Vec<SelectedApplication<'db>>,
        bounds: Vec<(usize, TraitChoiceId<'db>)>,
    ) -> Self {
        Self::new_with_choices(
            db,
            premises.scope(),
            premises.assumptions(),
            witnesses,
            selections,
            bounds,
        )
    }

    /// Caller contribution for an independently evaluated body. The body's
    /// own premises are added by its instance context, not supplied as callers.
    pub(crate) fn root_caller(db: &'db dyn HirAnalysisDb, owner: BodyOwner<'db>) -> Self {
        Self::empty(db, owner.scope())
    }

    pub(super) fn empty(db: &'db dyn HirAnalysisDb, normalization_scope: ScopeId<'db>) -> Self {
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
        self.openings(visitor.db()).visit_with(visitor);
        self.selections(visitor.db()).visit_with(visitor);
        for (_, choice) in self.bound_choices(visitor.db()) {
            choice.visit_with(visitor);
        }
    }
}

impl<'db> TyFoldable<'db> for ImplEnv<'db> {
    fn super_fold_with<F: TyFolder<'db>>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self {
        Self::new_with_openings(
            db,
            folder.fold_scope(self.normalization_scope(db)),
            self.assumptions(db).fold_with(db, folder),
            self.openings(db).clone().fold_with(db, folder),
            self.selections(db).clone().fold_with(db, folder),
            self.bound_choices(db)
                .iter()
                .map(|&(slot, choice)| (slot, choice.fold_with(db, folder)))
                .collect::<Vec<_>>(),
        )
    }
}

#[cfg(test)]
mod context_layout_tests {
    use super::*;

    #[allow(dead_code)]
    struct PreviousSelectedApplication<'db> {
        goal: TraitInstId<'db>,
        candidate: ImplementorId<'db>,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
        provisions: ProofProvisions<'db>,
    }

    #[test]
    fn retained_application_context_does_not_grow() {
        assert!(
            std::mem::size_of::<SelectedApplication<'static>>()
                <= std::mem::size_of::<PreviousSelectedApplication<'static>>()
        );
    }
}

#[salsa::tracked]
pub(crate) fn instance_position_context<'db>(
    db: &'db dyn HirAnalysisDb,
    instance: crate::analysis::semantic::SemanticInstance<'db>,
    origin: crate::analysis::semantic::SemOrigin<'db>,
) -> ImplEnv<'db> {
    use crate::analysis::semantic::SemOrigin;
    use crate::analysis::semantic::{
        instance::{instantiate_checked, semantic_instance_base_assumptions_for_key},
        instantiated_effect_env,
    };
    use crate::analysis::ty::ty_check::{SemanticExprLowering, infer_body};
    use indexmap::IndexSet;

    let key = instance.key(db);
    let typed = key.typed_body(db);
    let expr = match origin {
        SemOrigin::Expr(expr) => Some(expr),
        SemOrigin::Stmt(stmt) => match stmt
            .data(db, typed.body().expect("a statement position has a body"))
            .borrowed()
            .to_opt()
        {
            Some(
                crate::hir_def::Stmt::For(_, iter, _, _)
                | crate::hir_def::Stmt::Expr(iter)
                | crate::hir_def::Stmt::While(_, iter),
            ) => Some(*iter),
            Some(crate::hir_def::Stmt::Let(_, _, value) | crate::hir_def::Stmt::Return(value)) => {
                *value
            }
            _ => None,
        },
        SemOrigin::Body(_) | SemOrigin::Synthetic => None,
    };
    let caller = key.impl_env(db);
    let template = &infer_body(db, key.owner(db)).1;
    let source_call = expr.and_then(|expr| template.semantic_expr_lowering(expr));
    let mut selections = caller.selections(db).clone();
    let mut witnesses = caller.openings(db).clone();
    if let Some(expr) = expr {
        let lexical = if matches!(source_call, Some(SemanticExprLowering::Call { .. })) {
            crate::core::semantic::call_selections_at(db, template, expr)
        } else {
            template.selections_for_expr(db, expr).to_vec()
        };
        let lexical = instantiate_checked(db, key.owner(db), lexical, key.subst(db))
            .expect("position choices use their instance substitution domain");
        selections.extend(lexical);
        if matches!(source_call, Some(SemanticExprLowering::Call { .. })) {
            let opened = crate::semantic::instance_call_evidence_at(db, key, expr);
            // The checked source opening shadows an inherited named
            // choice for the same goal, before either becomes proof input.
            selections
                .retain(|selection| !opened.iter().any(|&(_, goal)| selection.matches(db, goal)));
            witnesses.extend(
                opened
                    .into_iter()
                    .map(|(_, goal)| crate::semantic::Provision::Bound(goal)),
            );
        } else {
            let position =
                crate::core::semantic::context_at(db, key.owner(db).scope(), Some((typed, expr)));
            selections.retain(|selection| {
                !position
                    .openings(db)
                    .iter()
                    .any(|opening| selection.matches(db, opening.goal()))
            });
            witnesses.extend(position.openings(db).iter().copied());
        }
    }
    let assumptions = instantiated_effect_env(db, instance).map_or_else(
        || semantic_instance_base_assumptions_for_key(db, key),
        |env| env.assumptions(db),
    );
    let witnesses = witnesses
        .into_iter()
        .collect::<IndexSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let mut predicates: IndexSet<_> = assumptions.list(db).iter().copied().collect();
    predicates.extend(witnesses.iter().map(|provision| provision.goal()));
    ImplEnv::new_with_openings(
        db,
        caller.normalization_scope(db),
        PredicateListId::new(db, predicates.into_iter().collect::<Vec<_>>()).extend_all_bounds(db),
        witnesses,
        selections,
        caller.bound_choices(db).clone(),
    )
}

/// A family declaration promises bounds pointwise under its trait parameters,
/// even if the implementation's own parameters have different names.
pub(crate) fn associated_type_bound_context<'db>(
    db: &'db dyn HirAnalysisDb,
    assoc: super::ImplementorAssocTypeView<'db>,
) -> TraitSolveCx<'db> {
    let crate::analysis::ty::trait_def::ImplementorOrigin::Hir(owner) =
        assoc.implementor.origin(db)
    else {
        unreachable!("associated implementation diagnostics require an authored impl")
    };
    let context = super::context_at(db, owner.scope(), None);
    let mut predicates = context.assumptions(db).list(db).to_vec();
    predicates.extend_from_slice(assoc.parameter_bounds(db).list(db));
    let premises = SolverPremises {
        scope: context.normalization_scope(db),
        assumptions: PredicateListId::new(db, predicates).extend_all_bounds(db),
    };
    TraitSolveCx::at_position(
        db,
        premises,
        context.proof_provisions(db),
        context_authority(),
    )
}

/// Read the context an associated-constant use captured, including only the
/// dependent provisions that its prerequisite proof reaches.
pub(crate) fn captured_assoc_const_context<'db>(
    db: &'db dyn HirAnalysisDb,
    use_: crate::analysis::ty::assoc_const::AssocConstUse<'db>,
) -> TraitSolveCx<'db> {
    let provisions = use_.dependent_context().map_or_else(
        || ProofProvisions::none(db),
        |context| context.proof_provisions(db),
    );
    TraitSolveCx::at_position(db, use_.premises(), provisions, context_authority())
}
