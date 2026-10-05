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
            trait_resolution::{PredicateListId, Selection, TraitSolveCx},
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
}

impl<'db> SelectedApplication<'db> {
    pub fn from_evidence(evidence: &SelectedImplEvidence<'db>) -> Self {
        Self {
            goal: evidence.goal(),
            candidate: evidence.selected(),
            scope: evidence.context().scope(),
            assumptions: evidence.context().assumptions(),
        }
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
        prove_selected_impl_evidence(
            db,
            TraitSolveCx::new(db, self.scope).with_assumptions(self.assumptions),
            self.candidate,
            self.goal,
        )?;
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
        Ok(resolve_trait_method_instance(db, context, goal, method))
    }
}

impl<'db> crate::analysis::ty::visitor::TyVisitable<'db> for SelectedApplication<'db> {
    fn visit_with<V: crate::analysis::ty::visitor::TyVisitor<'db> + ?Sized>(
        &self,
        visitor: &mut V,
    ) {
        self.goal.visit_with(visitor);
        self.assumptions.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for SelectedApplication<'db> {
    fn super_fold_with<F: TyFolder<'db>>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self {
        Self {
            goal: self.goal.fold_with(db, folder),
            assumptions: self.assumptions.fold_with(db, folder),
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

impl<'db> ImplEnv<'db> {
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

    pub fn selection_for(
        self,
        db: &'db dyn HirAnalysisDb,
        goal: TraitInstId<'db>,
    ) -> Option<SelectedApplication<'db>> {
        crate::core::semantic::lookup_trait_goal(
            db,
            goal,
            self.selections(db).iter().copied(),
            SelectedApplication::goal,
        )
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
