//! Const declaration requirements are discharged after inference, for every body
//! owner. Concrete discharge uses ordinary CTFE. Symbolic forwarding compares
//! resolved, typed expressions after substitution, without evaluating
//! unknown parameters or assuming the obligation being checked.
use super::*;
use crate::analysis::name_resolution::{
    method_selection::{MethodCandidate, select_method_candidate},
    resolve_path_with_minter,
};
use crate::analysis::semantic::{SelectedApplication, eval_const_instance};
use crate::analysis::ty::canonical::Canonicalized;
use crate::analysis::ty::trait_def::ImplementorOrigin;
use crate::analysis::ty::{
    const_ty::ConstBodyLowering,
    subst::substitute_complete,
    ty_lower::{CompleteSubst, SubstError, lower_hir_ty_with_resolutions, lower_type_alias},
};
use crate::hir_def::{GenericArg, GenericArgListId, ItemKind, UnOp, scope_graph::ScopeId};

#[derive(Debug, Clone, PartialEq, Eq, Update)]
pub struct PredicateKey<'db> {
    ty: TyId<'db>,
    arithmetic: crate::hir_def::attr::ArithmeticMode,
    operation: Option<Callable<'db>>,
    term: PredicateTerm<'db>,
}

#[derive(Debug, Clone, PartialEq, Eq, Update)]
pub enum PredicateTerm<'db> {
    Literal(LitKind<'db>),
    TypeConst(TyId<'db>),
    Const(Const<'db>),
    TraitConst(TraitInstId<'db>, crate::hir_def::IdentId<'db>),
    InherentConst(
        crate::hir_def::Impl<'db>,
        TyId<'db>,
        crate::hir_def::IdentId<'db>,
    ),
    Binary(BinOp, Box<PredicateKey<'db>>, Box<PredicateKey<'db>>),
    Unary(UnOp, Box<PredicateKey<'db>>),
    Cast(Box<PredicateKey<'db>>),
    Call(Vec<PredicateKey<'db>>),
}

/// A condition use, before or after its actual discharge. Checking replaces
/// the pending state in the body's record rather than retaining a second queue.
#[derive(Debug, Clone, PartialEq, Eq, Update)]
pub struct ConstConditionUse<'db> {
    declaration: WhereClauseOwner<'db>,
    predicate: Body<'db>,
    args: Vec<TyId<'db>>,
    caller: Option<GenericParamOwner<'db>>,
    context: ImplEnv<'db>,
    use_span: Option<DynLazySpan<'db>>,
    discharge: Option<RequirementDischarge<'db>>,
}

impl<'db> ConstConditionUse<'db> {
    pub fn declaration(&self) -> WhereClauseOwner<'db> {
        self.declaration
    }
    pub fn source_predicate(&self) -> Body<'db> {
        self.predicate
    }
    pub fn arguments(&self) -> &[TyId<'db>] {
        &self.args
    }
    pub fn caller(&self) -> Option<GenericParamOwner<'db>> {
        self.caller
    }
    pub fn selections(&self, db: &'db dyn HirAnalysisDb) -> &[SelectedApplication<'db>] {
        self.context.selections(db)
    }
    pub fn span(&self) -> &DynLazySpan<'db> {
        self.use_span.as_ref().expect("condition has a use site")
    }
    pub fn outcome(&self) -> Discharge<'db> {
        self.discharge
            .as_ref()
            .expect("condition has been checked")
            .outcome
    }
    pub fn predicate(&self) -> Option<&PredicateKey<'db>> {
        self.discharge.as_ref()?.predicate.as_deref()
    }

    /// Locate the actual retained proof in the caller's declared conditions.
    /// Inherited premises have no local v1 index; do not reconstruct a proof.
    pub fn premise_index(&self, db: &'db dyn HirAnalysisDb) -> Option<usize> {
        let Discharge::Holds(RequirementProof::Premise(predicate)) = self.outcome() else {
            return None;
        };
        self.caller?
            .where_clause_owner()?
            .where_clause(db)
            .const_predicates(db)
            .iter()
            .position(|candidate| *candidate == predicate)
    }
}

impl<'db> PredicateKey<'db> {
    pub fn ty(&self) -> TyId<'db> {
        self.ty
    }
    pub fn term(&self) -> &PredicateTerm<'db> {
        &self.term
    }
    pub fn arithmetic_mode(&self) -> crate::hir_def::attr::ArithmeticMode {
        self.arithmetic
    }
    pub fn operation(&self) -> Option<&Callable<'db>> {
        self.operation.as_ref()
    }
}

/// A condition application in its inference template. One HIR site may
/// instantiate a declaration's predicate several ways (for example two call
/// bounds); its original arguments and lexical context distinguish those uses.
/// Folding substitutes the record, leaving this authored identity unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Update)]
pub struct ConditionSite<'db> {
    span: DynLazySpan<'db>,
    declaration: WhereClauseOwner<'db>,
    predicate: Body<'db>,
    args: Vec<TyId<'db>>,
    caller: Option<GenericParamOwner<'db>>,
    context: ImplEnv<'db>,
}

pub(super) type ConditionDecisions<'db> =
    common::indexmap::IndexMap<ConditionSite<'db>, ConstConditionUse<'db>>;

pub(super) fn selection_context<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    selections: Vec<SelectedApplication<'db>>,
) -> ImplEnv<'db> {
    // This is the existing interned context authority. Preserve the ordered
    // lexical inputs here; evaluator normalization still happens at its boundary.
    ImplEnv::new_with_selections(
        db,
        scope,
        PredicateListId::empty_list(db),
        Vec::new(),
        selections,
    )
}

impl<'db> ConstConditionUse<'db> {
    fn site(&self) -> ConditionSite<'db> {
        ConditionSite {
            span: self.span().clone(),
            declaration: self.declaration,
            predicate: self.predicate,
            args: self.args.clone(),
            caller: self.caller,
            context: self.context,
        }
    }
}

pub(super) fn record_condition<'db>(
    conditions: &mut ConditionDecisions<'db>,
    use_: ConstConditionUse<'db>,
) {
    assert!(
        use_.discharge.is_some(),
        "unchecked conditions are absent from Decisions"
    );
    let site = use_.site();
    if let Some(previous) = conditions.get(&site) {
        assert_eq!(
            previous, &use_,
            "a condition application cannot acquire a different outcome"
        );
    } else {
        conditions.insert(site, use_);
    }
}

struct ConditionCapture<'a, 'db> {
    site: DynLazySpan<'db>,
    conditions: &'a mut ConditionDecisions<'db>,
}

impl<'db> ConditionCapture<'_, 'db> {
    fn decide(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        mut use_: ConstConditionUse<'db>,
    ) -> Option<RequirementReport<'db>> {
        crate::analysis::ty::trait_resolution::assert_outside_proof_search();
        use_.use_span = Some(self.site.clone());
        let site = use_.site();
        if !self.conditions.contains_key(&site) {
            use_.discharge = Some(discharge_selected_requirement(
                db,
                use_.declaration,
                use_.predicate,
                use_.args.clone(),
                use_.caller,
                use_.context,
            ));
            record_condition(self.conditions, use_);
        }
        matches!(self.conditions[&site].outcome(), Discharge::Fails(_))
            .then_some(RequirementReport::Condition(site, None))
    }
}

impl<'db> ConstConditionUse<'db> {
    pub(super) fn fold_with<F>(mut self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: crate::analysis::ty::fold::TyFolder<'db>,
    {
        self.args = self.args.fold_with(db, folder);
        self.context = self.context.fold_with(db, folder);
        if let Some(result) = &mut self.discharge {
            result.predicate = result
                .predicate
                .take()
                .map(|key| Arc::new(Arc::unwrap_or_clone(key).fold_with(db, folder)));
        }
        self
    }

    pub(super) fn visit_with<V>(&self, visitor: &mut V)
    where
        V: crate::analysis::ty::visitor::TyVisitor<'db> + ?Sized,
    {
        use crate::analysis::ty::visitor::TyVisitable;
        self.args.visit_with(visitor);
        self.context.visit_with(visitor);
        if let Some(result) = &self.discharge
            && let Some(predicate) = &result.predicate
        {
            predicate.visit_with(visitor);
        }
    }
}

pub(super) fn pending_owner_const_obligations<'db>(
    db: &'db dyn HirAnalysisDb,
    declaration: WhereClauseOwner<'db>,
    args: &[TyId<'db>],
    use_scope: ScopeId<'db>,
    use_span: crate::span::DynLazySpan<'db>,
) -> Vec<ConstConditionUse<'db>> {
    owner_const_obligations(db, declaration, args, use_scope)
        .into_iter()
        .map(|mut use_| {
            use_.use_span = Some(use_span.clone());
            use_
        })
        .collect()
}

fn owner_const_obligations<'db>(
    db: &'db dyn HirAnalysisDb,
    declaration: WhereClauseOwner<'db>,
    args: &[TyId<'db>],
    use_scope: ScopeId<'db>,
) -> Vec<ConstConditionUse<'db>> {
    let caller =
        premise_owner_in_scope(db, use_scope).filter(|_| args.iter().any(|ty| ty.has_param(db)));
    declaration
        .where_clause(db)
        .const_predicates(db)
        .iter()
        .map(|&predicate| ConstConditionUse {
            declaration,
            predicate,
            args: args.to_vec(),
            caller,
            context: selection_context(db, use_scope, Vec::new()),
            use_span: None,
            discharge: None,
        })
        .collect()
}

fn selected_impl_condition_obligations<'db>(
    db: &'db dyn HirAnalysisDb,
    conditions: crate::analysis::ty::trait_resolution::ImplConditionsId<'db>,
    use_scope: ScopeId<'db>,
) -> Vec<ConstConditionUse<'db>> {
    let mut obligations = Vec::new();
    for &goal in conditions.trait_goals(db) {
        for obligation in owner_const_obligations(
            db,
            WhereClauseOwner::Trait(goal.def(db)),
            goal.args(db),
            use_scope,
        ) {
            if !obligations.contains(&obligation) {
                obligations.push(obligation);
            }
        }
    }
    for &instance in conditions.instances(db) {
        let ImplementorOrigin::Hir(implementation) = instance.origin(db) else {
            continue;
        };
        for obligation in owner_const_obligations(
            db,
            WhereClauseOwner::ImplTrait(implementation),
            instance.params(db),
            use_scope,
        ) {
            if !obligations.contains(&obligation) {
                obligations.push(obligation);
            }
        }
    }
    obligations
}

pub(super) fn selected_impl_condition_uses<'db>(
    db: &'db dyn HirAnalysisDb,
    conditions: crate::analysis::ty::trait_resolution::ImplConditionsId<'db>,
    use_scope: ScopeId<'db>,
    use_span: crate::span::DynLazySpan<'db>,
    selections: &[SelectedApplication<'db>],
) -> Vec<ConstConditionUse<'db>> {
    selected_impl_condition_obligations(db, conditions, use_scope)
        .into_iter()
        .map(|mut use_| {
            use_.context = selection_context(db, use_scope, selections.to_vec());
            use_.use_span = Some(use_span.clone());
            use_
        })
        .collect()
}

pub(crate) fn selected_impl_requirements_hold<'db>(
    db: &'db dyn HirAnalysisDb,
    conditions: crate::analysis::ty::trait_resolution::ImplConditionsId<'db>,
    scope: ScopeId<'db>,
) -> bool {
    crate::analysis::ty::trait_resolution::assert_outside_proof_search();
    selected_impl_condition_obligations(db, conditions, scope)
        .iter()
        .all(|use_| {
            matches!(
                discharge_selected_requirement(
                    db,
                    use_.declaration,
                    use_.predicate,
                    use_.args.clone(),
                    use_.caller,
                    use_.context
                )
                .outcome,
                Discharge::Holds(_)
            )
        })
}

fn check_recorded_condition_uses<'db>(
    db: &'db dyn HirAnalysisDb,
    uses: &[ConstConditionUse<'db>],
) -> RequirementCheck<'db> {
    crate::analysis::ty::trait_resolution::assert_outside_proof_search();
    let mut check = RequirementCheck::default();
    for use_ in uses {
        if check.conditions.contains_key(&use_.site()) {
            continue;
        }
        if let Some(report) = (ConditionCapture {
            site: use_.span().clone(),
            conditions: &mut check.conditions,
        })
        .decide(db, use_.clone())
        {
            check.reports.push(report);
        }
    }
    check
}

pub(crate) fn check_impl_trait_header_const_requirements<'db>(
    db: &'db dyn HirAnalysisDb,
    implementation: crate::hir_def::ImplTrait<'db>,
) -> Vec<FuncBodyDiag<'db>> {
    let Some(implementor) = super::super::trait_lower::lower_impl_trait(db, implementation) else {
        return Vec::new();
    };
    let goal = implementor.trait_(db);
    let uses = pending_owner_const_obligations(
        db,
        WhereClauseOwner::Trait(goal.def(db)),
        goal.args(db),
        implementation.scope(),
        implementation.span().trait_ref().into(),
    );
    check_recorded_condition_uses(db, &uses).diagnostics()
}

impl<'db> crate::analysis::ty::visitor::TyVisitable<'db> for PredicateKey<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: crate::analysis::ty::visitor::TyVisitor<'db> + ?Sized,
    {
        self.ty.visit_with(visitor);
        if let Some(operation) = &self.operation {
            operation.visit_with(visitor);
        }
        match &self.term {
            PredicateTerm::TypeConst(ty) | PredicateTerm::InherentConst(_, ty, _) => {
                ty.visit_with(visitor)
            }
            PredicateTerm::TraitConst(goal, _) => goal.visit_with(visitor),
            PredicateTerm::Binary(_, lhs, rhs) => {
                lhs.visit_with(visitor);
                rhs.visit_with(visitor);
            }
            PredicateTerm::Unary(_, value) | PredicateTerm::Cast(value) => {
                value.visit_with(visitor)
            }
            PredicateTerm::Call(args) => args.visit_with(visitor),
            PredicateTerm::Literal(_) | PredicateTerm::Const(_) => {}
        }
    }
}

impl<'db> crate::analysis::ty::fold::TyFoldable<'db> for PredicateKey<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: crate::analysis::ty::fold::TyFolder<'db>,
    {
        let term = match self.term {
            PredicateTerm::TypeConst(ty) => PredicateTerm::TypeConst(ty.fold_with(db, folder)),
            PredicateTerm::TraitConst(goal, name) => {
                PredicateTerm::TraitConst(goal.fold_with(db, folder), name)
            }
            PredicateTerm::InherentConst(owner, ty, name) => {
                PredicateTerm::InherentConst(owner, ty.fold_with(db, folder), name)
            }
            PredicateTerm::Binary(op, lhs, rhs) => PredicateTerm::Binary(
                op,
                Box::new((*lhs).fold_with(db, folder)),
                Box::new((*rhs).fold_with(db, folder)),
            ),
            PredicateTerm::Unary(op, arg) => {
                PredicateTerm::Unary(op, Box::new((*arg).fold_with(db, folder)))
            }
            PredicateTerm::Cast(arg) => PredicateTerm::Cast(Box::new((*arg).fold_with(db, folder))),
            PredicateTerm::Call(args) => PredicateTerm::Call(args.fold_with(db, folder)),
            term => term,
        };
        Self {
            ty: self.ty.fold_with(db, folder),
            arithmetic: self.arithmetic,
            operation: self
                .operation
                .map(|callable| callable.fold_with(db, folder)),
            term,
        }
    }
}

pub(crate) fn comparable_const_predicate_key<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    scope: ScopeId<'db>,
    args: &[TyId<'db>],
) -> Option<PredicateKey<'db>> {
    let (diags, typed) = infer_body(
        db,
        BodyOwner::AnonConstBody {
            body,
            expected: TyId::bool(db),
        },
    );
    if !diags_allow_evaluation(db, diags) {
        return None;
    }
    let owner = GenericParamOwner::from_item_opt(scope.item())?;
    let subst = CompleteSubst::for_owner(db, owner, args.to_vec()).ok()?;
    predicate_key(db, body, typed, &subst).ok().flatten()
}

/// Canonicalize operands without evaluating the predicate itself: a restated
/// condition must retain its operators, casts and selected applications.
pub(crate) fn normalize_comparable_const_predicate_key<'db>(
    db: &'db dyn HirAnalysisDb,
    key: PredicateKey<'db>,
    scope: ScopeId<'db>,
) -> PredicateKey<'db> {
    use super::expr::TraitOps;
    use crate::analysis::ty::const_ty::{ConstCanonEnv, ConstCanonMode, canonicalize_ty_for_mode};
    use crate::analysis::ty::ty_def::{PrimTy, TyBase};

    struct CanonicalOperands<'db> {
        env: ConstCanonEnv<'db>,
    }
    impl<'db> TyFolder<'db> for CanonicalOperands<'db> {
        fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
            let ty = canonicalize_ty_for_mode(db, ty, self.env, ConstCanonMode::Identity);
            // Integer value identity uses the shared runtime width/sign shape.
            // usize/isize have the same shape as u256/i256 in Fe.
            match ty.data(db) {
                TyData::TyBase(TyBase::Prim(PrimTy::Usize)) => TyId::u256(db),
                TyData::TyBase(TyBase::Prim(PrimTy::Isize)) => {
                    TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::I256)))
                }
                TyData::ConstTy(_) => ty.super_fold_with(db, self),
                _ => ty,
            }
        }
    }
    fn primitive_operand<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> bool {
        ty == TyId::bool(db) || crate::analysis::semantic::consts::int_ty_shape(db, ty).is_some()
    }
    fn normalize_primitive_operation<'db>(
        db: &'db dyn HirAnalysisDb,
        operation: &mut Option<Callable<'db>>,
        op: &impl TraitOps,
        operands: &[TyId<'db>],
        scope: ScopeId<'db>,
    ) {
        // Keep evidence and user-defined operator authorities. Only closed
        // core operations on primitive values share the syntax representation.
        if let Some(callable) = operation
            && callable.effect_providers().is_empty()
            && let Some(inst) = callable.trait_inst()
            && crate::analysis::ty::corelib::resolve_core_trait(
                db,
                scope,
                &op.trait_path_segments(),
            ) == Some(inst.def(db))
            && let CallableDef::Func(func) = callable.callable_def()
            && func.name(db).to_opt() == Some(op.trait_method(db))
            && inst.args(db).iter().all(|ty| primitive_operand(db, *ty))
            && operands.iter().all(|ty| primitive_operand(db, *ty))
        {
            *operation = None;
        }
    }
    fn normalize<'db>(
        db: &'db dyn HirAnalysisDb,
        mut key: PredicateKey<'db>,
        scope: ScopeId<'db>,
    ) -> PredicateKey<'db> {
        key.term = match key.term {
            PredicateTerm::Literal(LitKind::Int(value)) => {
                if key.ty.is_integral_var(db) {
                    key.ty = TyId::u256(db);
                }
                PredicateTerm::TypeConst(TyId::const_ty(
                    db,
                    ConstTyId::integer(db, key.ty, value.data(db).clone().into()),
                ))
            }
            PredicateTerm::Literal(LitKind::Bool(value)) => {
                PredicateTerm::TypeConst(TyId::const_ty(
                    db,
                    crate::analysis::ty::const_ty::const_ty_from_sem_const(
                        db,
                        crate::analysis::semantic::consts::bool_const(db, value),
                    ),
                ))
            }
            PredicateTerm::Binary(op, lhs, rhs) => {
                let lhs = normalize(db, *lhs, scope);
                let rhs = normalize(db, *rhs, scope);
                normalize_primitive_operation(
                    db,
                    &mut key.operation,
                    &op,
                    &[lhs.ty, rhs.ty],
                    scope,
                );
                PredicateTerm::Binary(op, Box::new(lhs), Box::new(rhs))
            }
            PredicateTerm::Unary(op, arg) => {
                let arg = normalize(db, *arg, scope);
                normalize_primitive_operation(db, &mut key.operation, &op, &[arg.ty], scope);
                PredicateTerm::Unary(op, Box::new(arg))
            }
            PredicateTerm::Cast(arg) => PredicateTerm::Cast(Box::new(normalize(db, *arg, scope))),
            PredicateTerm::Call(args) => PredicateTerm::Call(
                args.into_iter()
                    .map(|arg| normalize(db, arg, scope))
                    .collect(),
            ),
            term => term,
        };
        key
    }
    let env = ConstCanonEnv::new(scope, PredicateListId::empty_list(db), None);
    normalize(db, key.fold_with(db, &mut CanonicalOperands { env }), scope)
}

// A declaration's full parameter schema also resolves the parameters that
// lexical lookup keeps owned by an enclosing impl, so one simultaneous
// substitution instantiates a method's requirement with its callable arguments.
fn requirement_subst<'db>(
    db: &'db dyn HirAnalysisDb,
    declaration: WhereClauseOwner<'db>,
    args: &[TyId<'db>],
) -> Result<CompleteSubst<'db>, SubstError<'db>> {
    CompleteSubst::for_owner(db, declaration.into(), args.to_vec())
}

// Arguments inferred in a method body can mention its impl's parameters as the
// impl owns them. Rebase them onto the method's own formals, which is how the
// caller's premises are instantiated, so both sides compare in one basis. A
// rebase that fails is an error, as an instantiation that fails is
// (`requirement_subst`): comparing in two bases could match the wrong premise
// or miss the right one.
fn caller_args<'db>(
    db: &'db dyn HirAnalysisDb,
    caller: GenericParamOwner<'db>,
    args: Vec<TyId<'db>>,
) -> Result<Vec<TyId<'db>>, SubstError<'db>> {
    let identity = collect_generic_params(db, caller).params(db).to_vec();
    let subst = CompleteSubst::for_owner(db, caller, identity)?;
    substitute_complete(db, args, &subst)
}

/// The forwardable form of a predicate's expression, instantiated with
/// `subst`, or `None` for an expression that cannot be forwarded. A
/// substitution that fails is an error, not an expression that cannot be
/// forwarded: it would report the wrong failure, or keep a premise from
/// matching.
fn predicate_key<'db>(
    db: &'db dyn HirAnalysisDb,
    predicate: Body<'db>,
    typed: &TypedBody<'db>,
    subst: &CompleteSubst<'db>,
) -> Result<Option<PredicateKey<'db>>, SubstError<'db>> {
    PredicateKeyBuilder {
        db,
        body: predicate,
        typed,
        subst,
        arithmetic: BodyOwner::const_predicate(db, predicate).arithmetic_mode(db),
    }
    .key(predicate.expr(db))
}

/// What building a predicate's key needs at every node of its expression.
struct PredicateKeyBuilder<'a, 'db> {
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    typed: &'a TypedBody<'db>,
    subst: &'a CompleteSubst<'db>,
    /// The predicate's arithmetic mode, which each node's key records.
    arithmetic: crate::hir_def::attr::ArithmeticMode,
}

impl<'db> PredicateKeyBuilder<'_, 'db> {
    fn instantiate<T>(&self, value: T) -> Result<T, SubstError<'db>>
    where
        T: TyFoldable<'db>,
    {
        substitute_complete(self.db, value, self.subst)
    }

    fn key(&self, expr: ExprId) -> Result<Option<PredicateKey<'db>>, SubstError<'db>> {
        let (db, typed) = (self.db, self.typed);
        let Some(data) = expr.data(db, self.body).borrowed().to_opt() else {
            return Ok(None);
        };
        let callable = typed.callable_expr(expr);
        // Supported keys have no opening/block syntax and use canonical inference.
        debug_assert!(crate::semantic::call_evidence_at(db, typed, expr).is_empty());
        let term = match data {
            Expr::Lit(lit) => PredicateTerm::Literal(*lit),
            Expr::Path(_) => {
                if let Some(ValuePathRef::TypeConst(ty)) = typed.value_path_ref(expr) {
                    PredicateTerm::TypeConst(self.instantiate(ty)?)
                } else {
                    // A reference's lookup scope is provenance, not its identity.
                    // Keep the resolved declaration and substituted receiver/goal.
                    match typed.expr_const_ref(expr) {
                        None => return Ok(None),
                        Some(ConstRef::Const(constant)) => PredicateTerm::Const(constant),
                        Some(ConstRef::TraitConst(reference)) => PredicateTerm::TraitConst(
                            self.instantiate(reference.inst())?,
                            reference.name(),
                        ),
                        Some(ConstRef::InherentConst(reference)) => PredicateTerm::InherentConst(
                            reference.impl_(),
                            self.instantiate(reference.receiver_ty())?,
                            reference.name(),
                        ),
                    }
                }
            }
            Expr::Bin(lhs, rhs, op) => {
                let (Some(lhs), Some(rhs)) = (self.key(*lhs)?, self.key(*rhs)?) else {
                    return Ok(None);
                };
                PredicateTerm::Binary(*op, Box::new(lhs), Box::new(rhs))
            }
            Expr::Un(value, op) => {
                let Some(value) = self.key(*value)? else {
                    return Ok(None);
                };
                PredicateTerm::Unary(*op, Box::new(value))
            }
            Expr::Cast(value, _) => {
                let Some(value) = self.key(*value)? else {
                    return Ok(None);
                };
                PredicateTerm::Cast(Box::new(value))
            }
            Expr::Call(_, call_args) => {
                if callable.is_none() {
                    return Ok(None);
                }
                let args = call_args
                    .iter()
                    .map(|arg| self.key(arg.expr))
                    .collect::<Result<Option<_>, _>>()?;
                let Some(args) = args else {
                    return Ok(None);
                };
                PredicateTerm::Call(args)
            }
            _ => return Ok(None),
        };
        Ok(Some(PredicateKey {
            ty: self.instantiate(typed.expr_ty(db, expr))?,
            arithmetic: self.arithmetic,
            operation: callable
                .map(|callable| self.instantiate(callable.clone()))
                .transpose()?,
            term,
        }))
    }
}

/// The generic parameters that a predicate's expression mentions, each
/// once, as types. The ambient trait assumptions it is checked under are not
/// dependencies of the expression, so their parameters are left out.
#[salsa::tracked(return_ref)]
fn predicate_params<'db>(db: &'db dyn HirAnalysisDb, predicate: Body<'db>) -> Vec<TyId<'db>> {
    use crate::analysis::ty::visitor::walk_ty;
    struct Params<'db> {
        db: &'db dyn HirAnalysisDb,
        found: Vec<TyId<'db>>,
    }
    impl<'db> TyVisitor<'db> for Params<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }

        fn visit_ty(&mut self, ty: TyId<'db>) {
            let is_param = match ty.data(self.db) {
                TyData::TyParam(_) => true,
                TyData::ConstTy(const_ty) => {
                    matches!(const_ty.data(self.db), ConstTyData::TyParam(..))
                }
                _ => false,
            };
            if is_param && !self.found.contains(&ty) {
                self.found.push(ty);
            }
            walk_ty(self, ty);
        }
    }
    let mut params = Params {
        db,
        found: Vec::new(),
    };
    infer_body(db, BodyOwner::const_predicate(db, predicate))
        .1
        .visit_body_types(&mut params);
    params.found
}

/// Why a const requirement does not hold at a use. Discharge decides it, and
/// the use's diagnostic renders it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub enum RequirementFailure {
    /// The condition evaluated to `false`.
    False,
    /// The requirement depends on itself.
    Recursive,
    /// Compile-time evaluation of the condition stopped.
    Evaluation(EvaluationStop),
    /// The condition is ill-formed, or its evaluation gave no `bool`.
    NotEstablished,
    /// The condition could not be instantiated with the use's arguments.
    NotInstantiable,
    /// A generic use states no identical condition.
    NoMatchingPremise,
    /// A generic use cannot forward the condition's expression.
    NotForwardable,
    /// A record with conditions that is not fully applied.
    PartiallyAppliedRecord,
    /// An enum with conditions that is not fully applied.
    PartiallyAppliedEnum,
}

/// Why compile-time evaluation of a condition stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub enum EvaluationStop {
    DivisionByZero,
    Overflow,
    StepLimit,
    RecursionLimit,
    RecursiveConst,
}

impl RequirementFailure {
    fn from_evaluation(cause: &InvalidCause<'_>) -> Self {
        Self::Evaluation(match cause {
            InvalidCause::ConstEvalDivisionByZero { .. } => EvaluationStop::DivisionByZero,
            InvalidCause::ConstEvalArithmeticOverflow { .. } => EvaluationStop::Overflow,
            InvalidCause::ConstEvalStepLimitExceeded { .. } => EvaluationStop::StepLimit,
            InvalidCause::ConstEvalRecursionLimitExceeded { .. } => EvaluationStop::RecursionLimit,
            InvalidCause::ConstEvalRecursiveConst { .. } => EvaluationStop::RecursiveConst,
            _ => return Self::NotEstablished,
        })
    }
}

/// Whether a use's const requirement holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub enum Discharge<'db> {
    Holds(RequirementProof<'db>),
    Fails(RequirementFailure),
}

/// The actual reason the shared checker established a condition.
/// Keep the matching predicate identity rather than reconstructing a premise
/// index from the caller's flattened inherited conditions during export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Update)]
pub enum RequirementProof<'db> {
    Evaluated,
    Premise(Body<'db>),
}

/// The outcome and substituted predicate retained by the same discharge query.
#[derive(Debug, Clone, PartialEq, Eq, Update)]
pub(super) struct RequirementDischarge<'db> {
    outcome: Discharge<'db>,
    predicate: Option<Arc<PredicateKey<'db>>>,
}

impl RequirementDischarge<'_> {
    fn failure(failure: RequirementFailure) -> Self {
        Self {
            outcome: Discharge::Fails(failure),
            predicate: None,
        }
    }
}

/// Whether a predicate can be discharged.
#[derive(Debug, Clone, PartialEq, Eq, Update)]
pub(super) enum FormationStatus<'db> {
    /// It type checks as a `bool` const body and does not depend on itself.
    /// Discharge reads its typed body.
    WellFormed(TypedBody<'db>),
    IllFormed,
    Recursive,
}

#[derive(Debug, Clone, PartialEq, Eq, Update)]
pub(super) struct PredicateFormation<'db> {
    /// The predicate's own diagnostics, reported at its declaration.
    pub(super) diags: Vec<FuncBodyDiag<'db>>,
    pub(super) status: FormationStatus<'db>,
}

impl<'db> PredicateFormation<'db> {
    /// The typed body of a well-formed predicate.
    pub(super) fn well_formed(&self) -> Option<&TypedBody<'db>> {
        match &self.status {
            FormationStatus::WellFormed(typed) => Some(typed),
            FormationStatus::IllFormed | FormationStatus::Recursive => None,
        }
    }
}

/// Reporting order and display metadata refer to the retained outcome;
/// formation diagnostics have no discharged application to refer to.
#[derive(Debug, Clone, PartialEq, Eq, Update)]
enum RequirementReport<'db> {
    Condition(ConditionSite<'db>, Option<TyId<'db>>),
    Formation(FuncBodyDiag<'db>),
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Update)]
pub(super) struct RequirementCheck<'db> {
    reports: Vec<RequirementReport<'db>>,
    pub(super) conditions: ConditionDecisions<'db>,
}

impl<'db> RequirementCheck<'db> {
    pub(super) fn recursive(&self) -> bool {
        self.reports.iter().any(|report| matches!(report,
            RequirementReport::Condition(site, _) if self.conditions[site].outcome() == Discharge::Fails(RequirementFailure::Recursive)
        ))
    }

    pub(super) fn diagnostics(&self) -> Vec<FuncBodyDiag<'db>> {
        crate::analysis::ty::trait_resolution::assert_outside_proof_search();
        self.reports
            .iter()
            .filter_map(|report| match report {
                RequirementReport::Condition(site, inferred) => {
                    let use_ = &self.conditions[site];
                    match use_.outcome() {
                        Discharge::Holds(_) => None,
                        Discharge::Fails(failure) => Some(unmet_requirement_diag(
                            use_.span().clone(),
                            use_.predicate,
                            failure,
                            *inferred,
                        )),
                    }
                }
                RequirementReport::Formation(diag) => Some(diag.clone()),
            })
            .collect()
    }

    fn report_type(&mut self, mut unmet: TypeRequirementFailure<'db>, inferred: bool) {
        if inferred {
            match &mut unmet.report {
                RequirementReport::Condition(_, displayed) => *displayed = Some(unmet.ty),
                RequirementReport::Formation(FuncBodyDiag::Body(
                    BodyDiag::ConstRequirementNotSatisfied { inferred, .. },
                )) => *inferred = Some(unmet.ty),
                _ => unreachable!("type requirement failures have a type display slot"),
            }
        }
        self.reports.push(unmet.report);
    }

    fn extend(&mut self, other: Self) {
        self.reports.extend(other.reports);
        for (_, use_) in other.conditions {
            record_condition(&mut self.conditions, use_);
        }
    }
}

fn unmet_requirement_diag<'db>(
    primary: DynLazySpan<'db>,
    predicate: Body<'db>,
    failure: RequirementFailure,
    inferred: Option<TyId<'db>>,
) -> FuncBodyDiag<'db> {
    BodyDiag::ConstRequirementNotSatisfied {
        primary,
        predicate: predicate.span().into(),
        reason: failure,
        inferred,
    }
    .into()
}

#[salsa::tracked(return_ref, cycle_initial=formation_cycle_initial, cycle_fn=formation_cycle_recover)]
pub(super) fn check_predicate_formation<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
) -> PredicateFormation<'db> {
    let owner = BodyOwner::const_predicate(db, body);
    // A condition the parser could not read has no expression to check or
    // evaluate. The parser reported it, at its position.
    if matches!(body.expr(db).data(db, body), Partial::Absent) {
        return PredicateFormation {
            diags: infer_body(db, owner).0.clone(),
            status: FormationStatus::IllFormed,
        };
    }
    // The condition is checked as every body is (`checked_body`).
    let CheckedBody {
        mut diags,
        typed,
        recursive,
    } = checked_body(db, owner);
    // A recursive failure is absorbing, and is reported once, at this predicate.
    let status = if recursive {
        diags = vec![BodyDiag::RecursiveConstRequirement(body.span().into()).into()];
        FormationStatus::Recursive
    } else if diags_allow_evaluation(db, &diags) {
        FormationStatus::WellFormed(typed)
    } else {
        FormationStatus::IllFormed
    };
    PredicateFormation { diags, status }
}

/// Whether uses of a declaration check `predicate`. A ground predicate, one
/// that mentions no generic parameter, is checked once where it is declared
/// (`check_where_const_predicates`), whether or not anything uses the item,
/// so a use does not report it again.
fn checked_at_uses<'db>(db: &'db dyn HirAnalysisDb, predicate: Body<'db>) -> bool {
    predicate_may_depend_on_params(db, predicate)
}

pub(super) fn predicate_may_depend_on_params<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
) -> bool {
    !predicate_params(db, body).is_empty()
}

// Requirements scope over function signatures/bodies and ADT fields, but
// their formation must be checked without those assumptions. In particular,
// nested anonymous constants inside a predicate are part of its formation.
fn requirement_premise_owner<'db>(
    db: &'db dyn HirAnalysisDb,
    owner: BodyOwner<'db>,
) -> Option<GenericParamOwner<'db>> {
    if !matches!(owner, BodyOwner::Func(_) | BodyOwner::AnonConstBody { .. }) {
        return None;
    }
    premise_owner_in_scope(db, owner.scope())
}

/// The declaration whose generic parameters the premises at `origin` are
/// stated over (see `caller_premises`).
fn premise_owner_in_scope<'db>(
    db: &'db dyn HirAnalysisDb,
    origin: ScopeId<'db>,
) -> Option<GenericParamOwner<'db>> {
    let mut current = Some(origin);
    while let Some(scope) = current {
        match scope.item() {
            item @ (ItemKind::Func(_)
            | ItemKind::Struct(_)
            | ItemKind::Enum(_)
            | ItemKind::Trait(_)
            | ItemKind::Impl(_)
            | ItemKind::ImplTrait(_)) => {
                let candidate = WhereClauseOwner::from_item_opt(item)?;
                if candidate
                    .where_clause(db)
                    .const_predicates(db)
                    .iter()
                    .any(|predicate| origin.is_transitive_child_of(db, predicate.scope()))
                {
                    return None;
                }
                return GenericParamOwner::from_item_opt(item);
            }
            item @ ItemKind::TypeAlias(_) => {
                return GenericParamOwner::from_item_opt(item);
            }
            ItemKind::Body(_) => current = scope.parent(db),
            // A nested declaration does not inherit function premises.
            _ => return None,
        }
    }
    None
}

/// The conditions that hold where premises are stated over `caller`'s
/// generic parameters, each with the substitution that states it there:
/// - the caller's own conditions, when its uses check them;
/// - inside an impl, the conditions of the records and enums in its self
///   type, since every use of the impl instantiates the self type with a
///   checked type (`check_entered_header`, and the receiver and argument
///   types);
/// - in a type alias, the conditions of the aliased type, since every
///   application of the alias is checked after expansion.
///
/// Trait and impl conditions are checked at their uses and are premises inside
/// their declarations. A method also inherits its immediate trait or impl's
/// premises, stated in the method's full parameter basis. Other enclosing
/// declarations do not supply premises to a nested declaration.
fn caller_premises<'db>(
    db: &'db dyn HirAnalysisDb,
    caller: GenericParamOwner<'db>,
) -> Result<Vec<(Body<'db>, CompleteSubst<'db>)>, SubstError<'db>> {
    let item = ItemKind::from(caller);
    let mut premises = Vec::new();
    let mut declarations = Vec::new();
    if let Some(owner) = WhereClauseOwner::from_item_opt(item) {
        declarations.push(owner);
    }
    if let GenericParamOwner::Func(func) = caller
        && let Some(parent @ (ItemKind::Impl(_) | ItemKind::ImplTrait(_) | ItemKind::Trait(_))) =
            func.scope().parent_item(db)
        && let Some(owner) = WhereClauseOwner::from_item_opt(parent)
    {
        declarations.push(owner);
    }
    for owner in declarations {
        let identity = collect_generic_params(db, owner.into()).params(db).to_vec();
        let args = caller_args(db, caller, identity)?;
        let subst = requirement_subst(db, owner, &args)?;
        premises.extend(
            owner
                .where_clause(db)
                .const_predicates(db)
                .iter()
                .map(|&predicate| (predicate, subst.clone())),
        );
    }
    let implied = match caller {
        GenericParamOwner::Func(func) => func
            .scope()
            .parent_item(db)
            .and_then(|parent| impl_self_ty(db, parent)),
        GenericParamOwner::Impl(_) | GenericParamOwner::ImplTrait(_) => impl_self_ty(db, item),
        GenericParamOwner::TypeAlias(alias) => {
            Some(lower_type_alias(db, alias).alias_to.instantiate_identity())
        }
        _ => None,
    };
    for ty in caller_args(db, caller, implied.into_iter().collect())? {
        type_conditions(db, ty, &mut premises)?;
    }
    Ok(premises)
}

/// The record or enum declaration of `ty`'s base, which states the
/// conditions of its applications.
fn adt_declaration<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
) -> Option<WhereClauseOwner<'db>> {
    use crate::analysis::ty::adt_def::AdtRef;
    let TyData::TyBase(TyBase::Adt(adt)) = ty.base_ty(db).data(db) else {
        return None;
    };
    Some(match adt.adt_ref(db) {
        AdtRef::Struct(record) => WhereClauseOwner::Struct(record),
        AdtRef::Enum(enum_) => WhereClauseOwner::Enum(enum_),
    })
}

/// The conditions of each fully applied record or enum in `ty`, instantiated
/// with its arguments.
fn type_conditions<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
    conditions: &mut Vec<(Body<'db>, CompleteSubst<'db>)>,
) -> Result<(), SubstError<'db>> {
    let (_, args) = ty.decompose_ty_app(db);
    for &arg in args {
        type_conditions(db, arg, conditions)?;
    }
    let Some(declaration) = adt_declaration(db, ty) else {
        return Ok(());
    };
    let predicates = declaration.where_clause(db).const_predicates(db);
    if predicates.is_empty() {
        return Ok(());
    }
    let subst = requirement_subst(db, declaration, args)?;
    conditions.extend(
        predicates
            .iter()
            .map(|&predicate| (predicate, subst.clone())),
    );
    Ok(())
}

/// A condition of a record or enum in `ty`, if any.
fn first_condition<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> Option<Body<'db>> {
    ty.decompose_ty_app(db)
        .1
        .iter()
        .find_map(|&arg| first_condition(db, arg))
        .or_else(|| {
            adt_declaration(db, ty)?
                .where_clause(db)
                .const_predicates(db)
                .first()
                .copied()
        })
}

/// Checks the requirements of the types and callees that enter a typed body.
///
/// Requirements are checked where a type enters the body: a written type
/// (`written_type_check`), a path that passes through an application, or an
/// instantiating expression. A path does not report an application that a
/// written type carries, and an expression does not report one that either
/// carries, since those report it. A pattern, a binding use, or a block or
/// branch only carries a type that entered elsewhere, so checking it again
/// repeats that report. A function-typed expression is not checked for its
/// type arguments here: they are written types, checked where they are
/// written, or inferred, and the check of inferred types at the end covers
/// them. An item reached through a path checks the header types it
/// instantiates that no checked value carries (`check_entered_header`).
pub(super) fn check_body_requirements<'db>(
    db: &'db dyn HirAnalysisDb,
    owner: BodyOwner<'db>,
    typed: &TypedBody<'db>,
    condition_inputs: &[ConstConditionUse<'db>],
) -> RequirementCheck<'db> {
    let Some(body) = typed.body() else {
        return RequirementCheck::default();
    };
    let mut checker = BodyRequirements::new(db, owner, typed, body);
    checker.check_path_applications();
    let mut entered = Vec::new();
    for (expr, data) in body.exprs(db).iter() {
        if !checker.carries(expr) {
            entered.push(checker.check_expr(expr, data));
        }
    }
    let mut check = checker.check_inferred_types(&entered);
    check.extend(check_recorded_condition_uses(db, condition_inputs));
    for (nested, expected) in expression_const_bodies(db, body, typed) {
        check.extend(anon_const_position_check(db, nested, expected, &[]));
    }
    check
}

fn is_function_ty<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> bool {
    matches!(ty.base_ty(db).data(db), TyData::TyBase(TyBase::Func(_)))
}

/// An expression that `check_body_requirements` checked, with its type.
struct CheckedExpr<'db> {
    expr: ExprId,
    ty: TyId<'db>,
    /// Whether a requirement of `ty` failed.
    failed: bool,
}

/// The state of `check_body_requirements` for one body.
struct BodyRequirements<'a, 'db> {
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    typed: &'a TypedBody<'db>,
    body: Body<'db>,
    /// The generic owner whose conditions discharge a use's requirements.
    premise_owner: Option<GenericParamOwner<'db>>,
    /// The callee expressions of the body's calls.
    direct_callees: FxHashSet<ExprId>,
    /// The applications that a written type or a path carries, which an
    /// expression does not report again.
    written: FxHashSet<TyId<'db>>,
    /// The failing applications reported so far.
    entered: FxHashSet<TyId<'db>>,
    check: RequirementCheck<'db>,
}

impl<'a, 'db> BodyRequirements<'a, 'db> {
    fn new(
        db: &'db dyn HirAnalysisDb,
        owner: BodyOwner<'db>,
        typed: &'a TypedBody<'db>,
        body: Body<'db>,
    ) -> Self {
        let direct_callees = body
            .exprs(db)
            .values()
            .filter_map(|expr| match expr.borrowed().to_opt()? {
                Expr::Call(callee, _) => Some(*callee),
                _ => None,
            })
            .collect();
        Self {
            db,
            scope: owner.scope(),
            typed,
            body,
            premise_owner: requirement_premise_owner(db, owner),
            direct_callees,
            written: written_applications(db, owner),
            entered: FxHashSet::default(),
            check: RequirementCheck::default(),
        }
    }

    /// Whether `expr` only carries a type that entered elsewhere.
    fn carries(&self, expr: ExprId) -> bool {
        self.typed.expr_binding(expr).is_some()
            || matches!(
                expr.data(self.db, self.body).borrowed().to_opt(),
                Some(Expr::Block(..) | Expr::If(..) | Expr::Match(..) | Expr::With(..))
            )
    }

    /// Whether `expr` is a path that names a function, as opposed to a
    /// binding use or another expression that holds a function value.
    fn names_function_value(&self, expr: ExprId) -> bool {
        self.typed.expr_binding(expr).is_none()
            && matches!(
                expr.data(self.db, self.body).borrowed().to_opt(),
                Some(Expr::Path(..))
            )
    }

    fn report_entered_type(&mut self, unmet: TypeRequirementFailure<'db>) {
        self.entered.insert(unmet.ty);
        self.check.report_type(unmet, false);
    }

    /// Checks the applications that the body's paths pass through, once per
    /// site, and adds them to the written applications.
    fn check_path_applications(&mut self) {
        // A written type suppresses only the path in its own expression.
        // The body-wide set remains useful for inferred types, but another
        // expression spelling the same type still needs its own decision.
        let written_sites: Vec<_> = written_types(self.db, ItemKind::Body(self.body))
            .iter()
            .filter_map(|entry| {
                let WrittenEntry::Type(written) = entry else {
                    return None;
                };
                let expr = written.expr?;
                let span = expr.span(self.body);
                let region: DynLazySpan<'db> =
                    match expr.data(self.db, self.body).borrowed().to_opt()? {
                        Expr::Path(..) => span.into_path_expr().into(),
                        Expr::RecordInit(..) => span.into_record_init_expr().into(),
                        _ => span.into(),
                    };
                Some((region, written))
            })
            .collect();
        let expression_sites: FxHashMap<_, _> = self
            .body
            .exprs(self.db)
            .iter()
            .filter_map(|(expr, data)| {
                let span = expr.span(self.body);
                let site: DynLazySpan<'db> = match data.borrowed().to_opt()? {
                    Expr::Path(..) => span.into_path_expr().into(),
                    Expr::RecordInit(..) => span.into_record_init_expr().into(),
                    _ => return None,
                };
                Some((site, expr))
            })
            .collect();
        let mut reported_paths = FxHashSet::default();
        for (site, application) in self.typed.path_applications() {
            if !reported_paths.insert((site, *application)) {
                continue;
            }
            let reported = written_sites
                .iter()
                .filter(|(region, written)| region == site || &written.span == site)
                .flat_map(|(_, written)| {
                    written.lowered.iter().chain(&written.applications).copied()
                })
                .collect();
            if let Some(unmet) = check_path_application(
                self.db,
                *application,
                self.scope,
                &reported,
                expression_sites
                    .get(site)
                    .map(|&expr| {
                        crate::semantic::context_at(self.db, self.scope, Some((self.typed, expr)))
                            .selections(self.db)
                            .as_slice()
                    })
                    .unwrap_or_default(),
                &mut ConditionCapture {
                    site: site.clone(),
                    conditions: &mut self.check.conditions,
                },
            ) {
                self.entered.insert(unmet.ty);
                // Keep the position's decision, while an authored type owns
                // the diagnostic for an application it already reports.
                if !self.written.contains(&unmet.ty) {
                    self.check.report_type(unmet, false);
                }
            }
        }
        self.written
            .extend(self.typed.path_applications().iter().map(|&(_, ty)| ty));
    }

    /// Checks the type of an expression that does not only carry one, the
    /// header types a constant reference enters, and a callee's
    /// requirements.
    fn check_expr(&mut self, expr: ExprId, data: &Partial<Expr<'db>>) -> CheckedExpr<'db> {
        let ty = self.typed.expr_ty(self.db, expr);
        let mut failed = false;
        if !is_function_ty(self.db, ty)
            && let Some(unmet) = check_type_requirements(
                self.db,
                ty,
                self.scope,
                &self.written,
                &mut ConditionCapture {
                    site: expr.span(self.body).into(),
                    conditions: &mut self.check.conditions,
                },
            )
        {
            failed = true;
            self.report_entered_type(unmet);
        }
        if let Some(headers) = const_ref_headers(self.db, self.typed, expr)
            && let Some(unmet) = check_entered_header(
                self.db,
                self.typed,
                expr,
                headers,
                self.scope,
                &self.written,
                &mut self.check.conditions,
            )
        {
            self.report_entered_type(unmet);
        }
        self.check_callee(expr, data, ty);
        CheckedExpr { expr, ty, failed }
    }

    /// Checks the requirements of the function or constructor that `expr`
    /// calls or names as a value.
    fn check_callee(&mut self, expr: ExprId, data: &Partial<Expr<'db>>, ty: TyId<'db>) {
        let callable = self.typed.callable_expr(expr);
        if callable.is_none() && self.direct_callees.contains(&expr) {
            return;
        }
        // A function value enters the body where it is written, at a path
        // that names the function. A call through anything else, such as a
        // binding, block, branch or field, calls such a value, whose
        // requirements were checked there.
        if let Some(Expr::Call(callee, _)) = data.borrowed().to_opt()
            && !self.names_function_value(*callee)
        {
            return;
        }
        let (definition, args) = match callable {
            Some(callable) => (callable.callable_def(), callable.generic_args()),
            // Any other expression with a function type, such as a field
            // read, carries a value written elsewhere.
            None if !self.names_function_value(expr) => return,
            None => {
                let (base, args) = ty.decompose_ty_app(self.db);
                let TyData::TyBase(TyBase::Func(definition)) = base.data(self.db) else {
                    return;
                };
                (*definition, args)
            }
        };
        match definition {
            CallableDef::Func(func) => {
                self.check_callee_headers(expr, func, args);
                self.discharge_callee_conditions(expr, func, args);
            }
            CallableDef::VariantCtor(_) => {
                self.check_constructor_value(expr, data, definition, args);
            }
        }
    }

    /// A constructor used as a value enters its enum type here, since no call
    /// expression instantiates it. A called constructor's enum type enters at
    /// the call, whose type is checked in `check_expr`.
    fn check_constructor_value(
        &mut self,
        expr: ExprId,
        data: &Partial<Expr<'db>>,
        definition: CallableDef<'db>,
        args: &[TyId<'db>],
    ) {
        if matches!(data.borrowed().to_opt(), Some(Expr::Path(..)))
            && !self.direct_callees.contains(&expr)
            && let Some(unmet) = check_type_requirements(
                self.db,
                definition.ret_ty(self.db).instantiate(self.db, args),
                self.scope,
                &self.written,
                &mut ConditionCapture {
                    site: expr.span(self.body).into(),
                    conditions: &mut self.check.conditions,
                },
            )
        {
            self.report_entered_type(unmet);
        }
    }

    /// Checks the header types that a use of `func` instantiates.
    fn check_callee_headers(&mut self, expr: ExprId, func: Func<'db>, args: &[TyId<'db>]) {
        match callee_headers(self.db, func, args) {
            Ok(headers) => {
                if let Some(unmet) = check_entered_header(
                    self.db,
                    self.typed,
                    expr,
                    headers,
                    self.scope,
                    &self.written,
                    &mut self.check.conditions,
                ) {
                    self.report_entered_type(unmet);
                }
            }
            // The instantiated headers are unknown, so a condition of a record
            // or enum that they could carry, from the declared header or from
            // the use's arguments, fails as a condition that discharge cannot
            // instantiate does.
            Err(_) => {
                if let Some(predicate) = declared_callee_headers(self.db, func)
                    .into_iter()
                    .chain(args.iter().copied())
                    .find_map(|ty| first_condition(self.db, ty))
                {
                    self.check
                        .reports
                        .push(RequirementReport::Formation(unmet_requirement_diag(
                            expr.span(self.body).into(),
                            predicate,
                            RequirementFailure::NotInstantiable,
                            None,
                        )));
                }
            }
        }
    }

    /// Discharges the conditions of `func` for a use with `args`.
    ///
    /// Method conditions are checked with the same instantiated callable
    /// arguments as free-function conditions. Calls keep ordinary resolution:
    /// requirements constrain the resolved call and do not take part in
    /// candidate selection.
    fn discharge_callee_conditions(&mut self, expr: ExprId, func: Func<'db>, args: &[TyId<'db>]) {
        let db = self.db;
        let predicates = WhereClauseOwner::Func(func)
            .where_clause(db)
            .const_predicates(db);
        if predicates.is_empty() {
            return;
        }
        // Only a use whose arguments mention the owner's parameters can be
        // discharged by the owner's own conditions.
        let premise_owner = self
            .premise_owner
            .filter(|_| args.iter().any(|ty| ty.has_param(db)));
        // Conditions must see the same lexical implementation choices as
        // the resolved call, including choices introduced by `with`.
        let selections = crate::semantic::call_selections_at(db, self.typed, expr);
        for &predicate in predicates
            .iter()
            .filter(|&&predicate| checked_at_uses(db, predicate))
        {
            let use_ = ConstConditionUse {
                declaration: WhereClauseOwner::Func(func),
                predicate,
                args: args.to_vec(),
                caller: premise_owner,
                context: selection_context(self.db, self.scope, selections.to_vec()),
                use_span: Some(expr.span(self.body).into()),
                discharge: None,
            };
            self.check
                .extend(check_recorded_condition_uses(db, &[use_]));
        }
    }

    /// Checks every type that inference gave an expression, whatever the
    /// position: the expression's own type, including a function value's
    /// generic arguments, and the generic arguments inferred for each call and
    /// method call.
    ///
    /// The checks in `check_expr` find the positions where a type enters;
    /// this one does not depend on listing them. It catches, for example,
    /// `U = Bounded<0>` inferred by trait solving for `call(Holder<0> {})`
    /// with `fn call<U, T: Tr<U>>(_ x: T)`, which no written type, path or
    /// expression type holds. An application reported before is not reported
    /// again, and any other is reported once, at the first expression that
    /// carries it. Patterns, binding uses, blocks and branches are left out:
    /// their types come from an expression here or from the signature, and a
    /// signature type reaches the body with its layout holes instantiated, so
    /// it would not match the written type that reported it.
    ///
    /// An expression type that `check_expr` found no failure in has none here
    /// either, since `reported` only adds to `written`, so only a function
    /// value's type and the types that failed there are looked at again. A
    /// failure in a function value's type or a call's generic arguments is in
    /// a type that inference supplied, so its error names the type. A further
    /// failure in an expression type that already failed is not: the
    /// expression's type gives it, as it gave the first.
    fn check_inferred_types(self, exprs: &[CheckedExpr<'db>]) -> RequirementCheck<'db> {
        let Self {
            db,
            scope,
            typed,
            body,
            written,
            entered,
            mut check,
            ..
        } = self;
        let mut reported = written;
        reported.extend(entered);
        for &CheckedExpr { expr, ty, failed } in exprs {
            let own = if is_function_ty(db, ty) {
                Some((ty, true))
            } else {
                failed.then_some((ty, false))
            };
            let callable_args = typed
                .callable_expr(expr)
                .map(|callable| callable.generic_args())
                .unwrap_or_default();
            let types = own
                .into_iter()
                .chain(callable_args.iter().map(|&ty| (ty, true)));
            for (ty, inferred) in types {
                while let Some(unmet) = check_type_requirements(
                    db,
                    ty,
                    scope,
                    &reported,
                    &mut ConditionCapture {
                        site: expr.span(body).into(),
                        conditions: &mut check.conditions,
                    },
                ) {
                    reported.insert(unmet.ty);
                    check.report_type(unmet, inferred);
                }
            }
        }
        check
    }
}

/// What an anonymous constant checked against `expected` reports at its
/// position: its unmet requirements, or the inference failures that its
/// type's error cannot show. Type lowering evaluates such a constant but does
/// not check its requirements, since checking them evaluates code, which can
/// lower the type the constant is part of. So the constant's position owns
/// them, and its owner calls this: `check_body_requirements` for expression
/// positions and `check_declared_type_requirements` for types.
fn anon_const_position_check<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    expected: TyId<'db>,
    selections: &[SelectedApplication<'db>],
) -> RequirementCheck<'db> {
    use crate::analysis::ty::const_ty::{ConstBodyFailure, const_body_failure};
    let owner = BodyOwner::AnonConstBody { body, expected };
    let inferred = inferred_body_with_selections(db, owner, selections);
    let (diags, typed) = &inferred.result;
    if diags_allow_evaluation(db, diags) {
        return check_body_requirements(db, owner, typed, &inferred.conditions);
    }
    // A lone path to a constant is read as that constant, not checked as a
    // body, so its failures are reported where the constant is.
    let position = body.scope().parent(db).unwrap_or(body.scope());
    let mut check = RequirementCheck::default();
    if !crate::analysis::ty::ty_lower::const_body_names_a_constant(
        db,
        body,
        position,
        typed.assumptions(),
    ) && let ConstBodyFailure::AtPosition(_) = const_body_failure(db, body, diags, typed)
    {
        check
            .reports
            .extend(diags.iter().cloned().map(RequirementReport::Formation));
    }
    check
}

/// The anonymous constants written directly in `path`'s segments, such as
/// `{ n + 1 }` in `Bounded<{ n + 1 }>::helper`.
fn path_const_bodies<'db>(db: &'db dyn HirAnalysisDb, path: PathId<'db>) -> Vec<Body<'db>> {
    let mut bodies = Vec::new();
    let mut segment = Some(path);
    while let Some(current) = segment {
        bodies.extend(generic_arg_const_bodies(db, current.generic_args(db)));
        segment = current.parent(db);
    }
    bodies
}

fn generic_arg_const_bodies<'db>(
    db: &'db dyn HirAnalysisDb,
    args: GenericArgListId<'db>,
) -> impl Iterator<Item = Body<'db>> + 'db {
    args.data(db).iter().filter_map(|arg| match arg {
        GenericArg::Const(arg) => match arg.value {
            ConstGenericArgValue::Expr(Partial::Present(body)) => Some(body),
            _ => None,
        },
        _ => None,
    })
}

/// Pairs each of `bodies` with the type lowering checks it against, read
/// from `lowered`, a lowering that deferred its anonymous constants.
fn positioned_const_bodies<'db>(
    db: &'db dyn HirAnalysisDb,
    lowered: &[TyId<'db>],
    bodies: &[Body<'db>],
) -> Vec<(Body<'db>, TyId<'db>)> {
    unevaluated_const_bodies(db, lowered)
        .into_iter()
        .filter(|(body, _)| bodies.contains(body))
        .collect()
}

/// The anonymous constants passed to `path`'s segments, each with the type of
/// the parameter it is passed to, read from that segment's own resolution in
/// `resolutions` (a lowering that deferred its anonymous constants). An
/// argument is checked there even when its value never reaches what the path
/// names: an alias can drop it, and a qualifier such as `Holder<{ n }>` in
/// `Holder<{ n }>::Out` may only select an impl. A segment that did not
/// resolve is reported by resolution. A type, trait or callable segment's
/// resolution holds its arguments, and an argument it has no parameter for
/// is reported by lowering as an arity error. An enum variant or module
/// segment has no generic parameters and its resolution holds no arguments,
/// so arguments written on it, such as `{ n }` in `Choice::Empty<{ n }>`,
/// are not found here. Resolution does not reject them either, and nothing
/// evaluates them. The explicit arguments of a callable are applied with the
/// call, so its uses check them (`expression_const_bodies`).
fn segment_const_args<'db>(
    db: &'db dyn HirAnalysisDb,
    path: PathId<'db>,
    resolutions: &[(PathId<'db>, PathRes<'db>)],
) -> Vec<(Body<'db>, TyId<'db>)> {
    let mut found = Vec::new();
    let mut segment = Some(path);
    while let Some(current) = segment {
        segment = current.parent(db);
        let args = current.generic_args(db);
        let bodies: Vec<_> = generic_arg_const_bodies(db, args).collect();
        if bodies.is_empty() {
            continue;
        }
        let Some(res) = resolutions
            .iter()
            .rev()
            .find_map(|(resolved, res)| (*resolved == current).then_some(res))
        else {
            continue;
        };
        match res {
            // An alias's arguments are its own parameters', whether or not the
            // aliased type uses them.
            PathRes::TyAlias(alias, _) => {
                for (arg, param) in args.data(db).iter().zip(alias.params(db)) {
                    if let GenericArg::Const(arg) = arg
                        && let ConstGenericArgValue::Expr(Partial::Present(body)) = arg.value
                        && let Some(expected) = param.const_ty_ty(db)
                    {
                        found.push((body, expected));
                    }
                }
            }
            res => found.extend(positioned_const_bodies(
                db,
                &path_res_tys(db, res.clone()),
                &bodies,
            )),
        }
    }
    found
}

/// The type of an array's length, which a length is checked against.
fn array_length_ty<'db>(db: &'db dyn HirAnalysisDb) -> Option<TyId<'db>> {
    TyId::array(db, TyId::unit(db)).applicable_ty(db)?.const_ty
}

/// The types a path resolution carries, including trait arguments.
fn path_res_tys<'db>(db: &'db dyn HirAnalysisDb, res: PathRes<'db>) -> Vec<TyId<'db>> {
    let mut tys = Vec::new();
    match res {
        PathRes::Trait(inst) | PathRes::TraitMethod(inst, _) => {
            tys.extend(inst.args(db).iter().copied());
        }
        PathRes::TraitConst(receiver, inst, _) => {
            tys.push(receiver);
            tys.extend(inst.args(db).iter().copied());
        }
        res => {
            res.map_over_ty(|ty| {
                tys.push(ty);
                ty
            });
        }
    }
    tys
}

/// The applications of records and enums with conditions that a path
/// resolution carries, at any depth: `Bounded<0>` for the segment `Bounded<0>`
/// of `Bounded<0>::Out`, or for the receiver of `Bounded<0>::L`. Path
/// resolution reports every segment it resolves, so the lowering of a written
/// type and the inference of a body record these for every prefix of every
/// path they resolve, and the requirement checks read them from there. This
/// reads only declarations, so inference can call it while it resolves.
pub(super) fn constrained_applications<'db>(
    db: &'db dyn HirAnalysisDb,
    res: &PathRes<'db>,
) -> Vec<TyId<'db>> {
    fn collect<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>, found: &mut Vec<TyId<'db>>) {
        for &arg in ty.decompose_ty_app(db).1 {
            collect(db, arg, found);
        }
        if adt_declaration(db, ty).is_some_and(|declaration| {
            !declaration.where_clause(db).const_predicates(db).is_empty()
        }) && !found.contains(&ty)
        {
            found.push(ty);
        }
    }
    let mut found = Vec::new();
    for ty in path_res_tys(db, res.clone()) {
        collect(db, ty, &mut found);
    }
    found
}

/// The first unmet requirement of an application that a path passed through.
/// A partial application, such as `Bounded` in `Bounded::helper()`, is not an
/// instantiation: inference supplies its arguments, and the header it
/// instantiates is checked where the item is used (`check_entered_header`).
/// A partial application written as a type is checked as that type.
fn check_path_application<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
    scope: ScopeId<'db>,
    reported: &FxHashSet<TyId<'db>>,
    selections: &[SelectedApplication<'db>],
    capture: &mut ConditionCapture<'_, 'db>,
) -> Option<TypeRequirementFailure<'db>> {
    let declaration = adt_declaration(db, ty)?;
    let arity = collect_generic_params(db, declaration.into())
        .params(db)
        .len();
    if ty.decompose_ty_app(db).1.len() != arity {
        return None;
    }
    check_selected_type_requirements(db, ty, scope, reported, selections, capture)
}

/// The anonymous constants a body writes in expression and pattern
/// positions, with the type each position checks it against: array repeat
/// lengths, explicit const arguments of calls, method calls and function
/// values, and const arguments of the other path segments. Types written in
/// the body are covered by `check_declared_type_requirements`.
fn expression_const_bodies<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    typed: &TypedBody<'db>,
) -> Vec<(Body<'db>, TyId<'db>)> {
    let checked_const_ty = |ty: TyId<'db>| match ty.data(db) {
        TyData::ConstTy(const_ty) if !ty.has_invalid(db) => Some(const_ty.ty(db)),
        _ => None,
    };
    // The callable an expression calls or names, with the generic arguments
    // inference gave it. A call whose explicit arguments failed has no
    // checked callable, so its definition is found again from its path or
    // receiver, without arguments.
    let callee = |expr: ExprId| -> Option<(CallableDef<'db>, Vec<TyId<'db>>)> {
        if let Some(callable) = typed.callable_expr(expr) {
            return Some((callable.callable_def(), callable.generic_args().to_vec()));
        }
        let (base, generic_args) = typed.expr_ty(db, expr).decompose_ty_app(db);
        if let TyData::TyBase(TyBase::Func(definition)) = base.data(db) {
            return Some((*definition, generic_args.to_vec()));
        }
        let candidate_def = |candidate: MethodCandidate<'db>| match candidate {
            MethodCandidate::InherentMethod(method) => method.def,
            MethodCandidate::TraitMethod(method) | MethodCandidate::NeedsConfirmation(method) => {
                CallableDef::Func(method.method)
            }
        };
        let definition = match expr.data(db, body).borrowed().to_opt()? {
            Expr::Path(Partial::Present(path)) => {
                let minter = LoweringContext::deferred(HoleAnchor::TemplatePath {
                    path: *path,
                    scope: body.scope(),
                    assumptions: typed.assumptions(),
                });
                match resolve_path_with_minter(
                    db,
                    *path,
                    body.scope(),
                    typed.assumptions(),
                    true,
                    &minter,
                )
                .ok()?
                {
                    PathRes::Func(ty) => match ty.base_ty(db).data(db) {
                        TyData::TyBase(TyBase::Func(definition)) => *definition,
                        _ => return None,
                    },
                    PathRes::Method(_, candidate) => candidate_def(candidate),
                    PathRes::TraitMethod(_, method) => CallableDef::Func(method),
                    _ => return None,
                }
            }
            Expr::MethodCall(receiver, Partial::Present(name), _, _) => {
                let receiver_ty = typed.expr_ty(db, *receiver);
                if receiver_ty.has_invalid(db) {
                    return None;
                }
                candidate_def(
                    select_method_candidate(
                        db,
                        &Canonicalized::new(db, receiver_ty),
                        *name,
                        body.scope(),
                        typed.assumptions(),
                        None,
                    )
                    .ok()?,
                )
            }
            _ => return None,
        };
        Some((definition, Vec::new()))
    };
    // Explicit arguments of a callable's own parameters, which the type
    // checker applies after the path resolves. Each is checked against its
    // parameter's type: the one its checked argument carries, or, for an
    // argument that failed, the parameter's declared type.
    let explicit_args = |expr: ExprId, args: GenericArgListId<'db>| {
        let Some((definition, generic_args)) = callee(expr) else {
            return Vec::new();
        };
        let offset = definition.offset_to_explicit_params_position(db);
        let expected = |index: usize| {
            generic_args
                .get(index)
                .and_then(|&arg| checked_const_ty(arg))
                .or_else(|| {
                    let CallableDef::Func(func) = definition else {
                        return None;
                    };
                    collect_generic_params(db, func.into())
                        .params(db)
                        .get(index)?
                        .const_ty_ty(db)
                })
        };
        args.data(db)
            .iter()
            .enumerate()
            .filter_map(|(idx, arg)| {
                let GenericArg::Const(arg) = arg else {
                    return None;
                };
                let ConstGenericArgValue::Expr(Partial::Present(body)) = arg.value else {
                    return None;
                };
                Some((body, expected(offset + idx)?))
            })
            .collect::<Vec<_>>()
    };
    // Arguments of the path's segments, which path resolution applies, each
    // checked against its segment's own resolution (`segment_const_args`).
    let segment_args = |path: PathId<'db>, value: bool| {
        if path_const_bodies(db, path).is_empty() {
            return Vec::new();
        }
        let minter = LoweringContext::deferred(HoleAnchor::TemplatePath {
            path,
            scope: body.scope(),
            assumptions: typed.assumptions(),
        })
        .recording_resolutions();
        let resolve = |value| {
            resolve_path_with_minter(db, path, body.scope(), typed.assumptions(), value, &minter)
        };
        // A tail that is not a value can still be a type, as a record's is.
        if resolve(value).is_err() {
            let _ = resolve(!value);
        }
        segment_const_args(db, path, &minter.into_resolutions())
    };

    let mut found: Vec<(Body<'db>, TyId<'db>)> = Vec::new();
    for (expr, data) in body.exprs(db).iter() {
        let entries = match data.borrowed().to_opt() {
            Some(Expr::ArrayRep(_, len)) => {
                len.to_opt().zip(array_length_ty(db)).into_iter().collect()
            }
            Some(Expr::Path(Partial::Present(path))) => {
                let mut entries = segment_args(*path, true);
                entries.extend(explicit_args(expr, path.generic_args(db)));
                entries
            }
            Some(Expr::RecordInit(Partial::Present(path), _)) => segment_args(*path, false),
            Some(Expr::MethodCall(_, _, args, _)) => explicit_args(expr, *args),
            _ => Vec::new(),
        };
        found.extend(entries);
    }
    for pat in body.pats(db).values() {
        if let Some(
            Pat::Path(Partial::Present(path), _)
            | Pat::PathTuple(Partial::Present(path), _)
            | Pat::Record(Partial::Present(path), _),
        ) = pat.borrowed().to_opt()
        {
            found.extend(segment_args(*path, true));
        }
    }
    // A body appears in one position, but two lookups can both find it.
    let mut seen = FxHashSet::default();
    found.retain(|(body, _)| seen.insert(*body));
    found
}

/// The types an impl's header instantiates: its self type and, for a trait
/// impl, the trait's arguments, in the impl's declaration coordinates.
fn header_types<'db>(db: &'db dyn HirAnalysisDb, item: ItemKind<'db>) -> Vec<TyId<'db>> {
    let mut tys: Vec<_> = impl_self_ty(db, item).into_iter().collect();
    if let ItemKind::ImplTrait(impl_trait) = item
        && let Some(inst) = impl_trait.trait_inst(db)
    {
        tys.extend(inst.args(db).iter().copied());
    }
    tys
}

/// The self type of an impl or trait impl, in its declaration coordinates.
fn impl_self_ty<'db>(db: &'db dyn HirAnalysisDb, item: ItemKind<'db>) -> Option<TyId<'db>> {
    match item {
        ItemKind::Impl(impl_) => Some(impl_.ty(db)),
        ItemKind::ImplTrait(impl_trait) => Some(impl_trait.ty(db)),
        _ => None,
    }
}

/// The header types of `func`'s parent in its declaration coordinates: an
/// impl's header, or a trait's `Self` and parameters.
fn declared_callee_headers<'db>(db: &'db dyn HirAnalysisDb, func: Func<'db>) -> Vec<TyId<'db>> {
    match func.scope().parent_item(db) {
        Some(ItemKind::Trait(trait_)) => trait_.params(db).to_vec(),
        Some(item) => header_types(db, item),
        None => Vec::new(),
    }
}

/// The header types a use of `func` with `args` instantiates: its impl's
/// header, or a trait method's `Self` and trait arguments. A header that
/// cannot be instantiated with `args` is an error, since dropping it would
/// skip the conditions it carries.
fn callee_headers<'db>(
    db: &'db dyn HirAnalysisDb,
    func: Func<'db>,
    args: &[TyId<'db>],
) -> Result<Vec<TyId<'db>>, SubstError<'db>> {
    let headers = declared_callee_headers(db, func);
    if headers.is_empty() {
        return Ok(headers);
    }
    let subst = CompleteSubst::for_owner(db, func.into(), args.to_vec())?;
    substitute_complete(db, headers, &subst)
}

/// The header types an associated constant path instantiates.
fn const_ref_headers<'db>(
    db: &'db dyn HirAnalysisDb,
    typed: &TypedBody<'db>,
    expr: ExprId,
) -> Option<Vec<TyId<'db>>> {
    match typed.expr_const_ref(expr)? {
        ConstRef::Const(_) => None,
        ConstRef::TraitConst(reference) => Some(reference.inst().args(db).to_vec()),
        ConstRef::InherentConst(reference) => Some(vec![reference.receiver_ty()]),
    }
}

/// A use of an associated item enters the header types it instantiates, such
/// as `Bounded<0>` in `Bounded<0>::helper()`, which no written type or checked
/// expression may carry. A header type that a checked receiver, argument,
/// result, or called function value already carries was checked where that
/// value entered; any other is checked here.
fn check_entered_header<'db>(
    db: &'db dyn HirAnalysisDb,
    typed: &TypedBody<'db>,
    expr: ExprId,
    headers: Vec<TyId<'db>>,
    scope: ScopeId<'db>,
    written: &FxHashSet<TyId<'db>>,
    conditions: &mut ConditionDecisions<'db>,
) -> Option<TypeRequirementFailure<'db>> {
    let body = typed.body()?;
    let mut carried = vec![expr];
    match expr.data(db, body).borrowed().to_opt() {
        Some(Expr::Call(callee, call_args)) => {
            if typed.expr_binding(*callee).is_some() {
                carried.push(*callee);
            }
            carried.extend(call_args.iter().map(|arg| arg.expr));
        }
        Some(Expr::MethodCall(receiver, _, _, call_args)) => {
            carried.push(*receiver);
            carried.extend(call_args.iter().map(|arg| arg.expr));
        }
        _ => {}
    }
    // A function value's own type is not checked (see `check_body_requirements`),
    // so it carries its header types only once bound and used again.
    let carried: Vec<_> = carried
        .into_iter()
        .map(|carrier| (carrier, typed.expr_ty(db, carrier)))
        .filter(|&(carrier, ty)| carrier != expr || !is_function_ty(db, ty))
        .map(|(_, ty)| ty)
        .collect();
    headers
        .into_iter()
        .filter(|&header| !carried.iter().any(|&ty| ty_mentions(db, ty, header)))
        .find_map(|header| {
            check_type_requirements(
                db,
                header,
                scope,
                written,
                &mut ConditionCapture {
                    site: expr.span(body).into(),
                    conditions,
                },
            )
        })
}

/// Whether `needle` occurs in `ty`.
fn ty_mentions<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>, needle: TyId<'db>) -> bool {
    ty == needle
        || ty
            .decompose_ty_app(db)
            .1
            .iter()
            .any(|&arg| ty_mentions(db, arg, needle))
}

/// An unmet requirement of a type application.
pub(super) struct TypeRequirementFailure<'db> {
    /// The failing application.
    ty: TyId<'db>,
    report: RequirementReport<'db>,
}

/// Returns the first unmet requirement in `ty`. Failures of the
/// applications in `reported` were already reported where those types
/// entered, so the search continues past them.
fn check_type_requirements<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
    scope: ScopeId<'db>,
    reported: &FxHashSet<TyId<'db>>,
    capture: &mut ConditionCapture<'_, 'db>,
) -> Option<TypeRequirementFailure<'db>> {
    check_selected_type_requirements(db, ty, scope, reported, &[], capture)
}

fn check_selected_type_requirements<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
    scope: ScopeId<'db>,
    reported: &FxHashSet<TyId<'db>>,
    selections: &[SelectedApplication<'db>],
    capture: &mut ConditionCapture<'_, 'db>,
) -> Option<TypeRequirementFailure<'db>> {
    let (base, args) = ty.decompose_ty_app(db);
    for &arg in args {
        if let Some(failure) =
            check_selected_type_requirements(db, arg, scope, reported, selections, capture)
        {
            return Some(failure);
        }
    }
    if reported.contains(&ty) {
        return None;
    }
    let TyData::TyBase(TyBase::Adt(adt)) = base.data(db) else {
        return None;
    };
    use crate::analysis::ty::adt_def::AdtRef;
    let (declaration, generic_owner, partially_applied) = match adt.adt_ref(db) {
        AdtRef::Struct(record) => (
            WhereClauseOwner::Struct(record),
            GenericParamOwner::Struct(record),
            RequirementFailure::PartiallyAppliedRecord,
        ),
        AdtRef::Enum(enum_) => (
            WhereClauseOwner::Enum(enum_),
            GenericParamOwner::Enum(enum_),
            RequirementFailure::PartiallyAppliedEnum,
        ),
    };
    let predicates = declaration.where_clause(db).const_predicates(db);
    if predicates.is_empty() {
        return None;
    }
    if args.len() != collect_generic_params(db, generic_owner).params(db).len() {
        return Some(TypeRequirementFailure {
            ty,
            report: RequirementReport::Formation(unmet_requirement_diag(
                capture.site.clone(),
                predicates[0],
                partially_applied,
                None,
            )),
        });
    }
    if args.iter().any(|arg| arg.has_var(db)) {
        return None;
    }
    let caller = premise_owner_in_scope(db, scope);
    predicates
        .iter()
        .filter(|&&predicate| checked_at_uses(db, predicate))
        .find_map(|&predicate| {
            let obligation = ConstConditionUse {
                declaration,
                predicate,
                args: args.to_vec(),
                caller,
                context: selection_context(db, scope, selections.to_vec()),
                use_span: None,
                discharge: None,
            };
            capture
                .decide(db, obligation)
                .map(|report| TypeRequirementFailure { ty, report })
        })
}

// This boundary must run even when Salsa reuses a cached discharge result.
fn discharge_selected_requirement<'db>(
    db: &'db dyn HirAnalysisDb,
    declaration: WhereClauseOwner<'db>,
    predicate: Body<'db>,
    args: Vec<TyId<'db>>,
    caller: Option<GenericParamOwner<'db>>,
    context: ImplEnv<'db>,
) -> RequirementDischarge<'db> {
    crate::analysis::ty::trait_resolution::assert_outside_proof_search();
    discharge_selected_requirement_query(db, declaration, predicate, args, caller, context)
}

#[salsa::tracked(cycle_initial=requirement_cycle_initial, cycle_fn=requirement_cycle_recover)]
fn discharge_selected_requirement_query<'db>(
    db: &'db dyn HirAnalysisDb,
    declaration: WhereClauseOwner<'db>,
    predicate: Body<'db>,
    args: Vec<TyId<'db>>,
    caller: Option<GenericParamOwner<'db>>,
    context: ImplEnv<'db>,
) -> RequirementDischarge<'db> {
    let selections = context.selections(db);
    let typed = match &check_predicate_formation(db, predicate).status {
        FormationStatus::WellFormed(typed) => typed,
        FormationStatus::IllFormed => {
            return RequirementDischarge::failure(RequirementFailure::NotEstablished);
        }
        FormationStatus::Recursive => {
            return RequirementDischarge::failure(RequirementFailure::Recursive);
        }
    };
    let not_instantiable = RequirementDischarge::failure(RequirementFailure::NotInstantiable);
    let args = match caller {
        Some(caller) => match caller_args(db, caller, args) {
            Ok(args) => args,
            Err(_) => return not_instantiable,
        },
        None => args,
    };
    let Ok(subst) = requirement_subst(db, declaration, &args) else {
        return not_instantiable;
    };
    // The use leaves the predicate generic when an argument for one of the
    // parameters it mentions still mentions a parameter.
    let mut symbolic = false;
    for &param in predicate_params(db, predicate) {
        let Ok(arg) = substitute_complete(db, param, &subst) else {
            return not_instantiable;
        };
        symbolic |= arg.has_param(db);
    }
    if symbolic {
        let Ok(key) = predicate_key(db, predicate, typed, &subst) else {
            return not_instantiable;
        };
        let key = key.map(Arc::new);
        if let (Some(key), Some(caller)) = (&key, caller) {
            let Ok(premises) = caller_premises(db, caller) else {
                return not_instantiable;
            };
            for (premise, premise_subst) in premises {
                let Some(premise_typed) = check_predicate_formation(db, premise).well_formed()
                else {
                    continue;
                };
                let Ok(premise_key) = predicate_key(db, premise, premise_typed, &premise_subst)
                else {
                    return not_instantiable;
                };
                if premise_key.as_ref() == Some(key.as_ref()) {
                    return RequirementDischarge {
                        outcome: Discharge::Holds(RequirementProof::Premise(premise)),
                        predicate: Some(key.clone()),
                    };
                }
            }
        }
        return RequirementDischarge {
            outcome: Discharge::Fails(if key.is_some() {
                RequirementFailure::NoMatchingPremise
            } else {
                RequirementFailure::NotForwardable
            }),
            predicate: key,
        };
    }
    // This is a view of the checked predicate before CTFE, not its truth value.
    // Unsupported views do not change concrete discharge behavior.
    let captured_predicate = predicate_key(db, predicate, typed, &subst)
        .ok()
        .flatten()
        .map(Arc::new);
    let owner = BodyOwner::const_predicate(db, predicate);
    let subst = GenericSubst::for_body_owner(db, owner, args);
    let outcome = if selections.is_empty() {
        condition_outcome(db, owner, subst)
    } else {
        let key = SemanticInstanceKey::new(
            db,
            owner,
            subst,
            EffectProviderSubst::empty(db),
            ImplEnv::new(
                db,
                owner.scope(),
                crate::semantic::body_assumptions_at(db, owner.scope()),
                Vec::new(),
            )
            .with_selections(db, selections.clone()),
        );
        condition_outcome_from_evaluation(
            db,
            owner,
            eval_const_instance(db, get_or_build_semantic_instance(db, key)),
        )
    };
    let outcome = match outcome {
        ConditionOutcome::True => Discharge::Holds(RequirementProof::Evaluated),
        ConditionOutcome::False => Discharge::Fails(RequirementFailure::False),
        ConditionOutcome::NotBool | ConditionOutcome::Blocked(_) => {
            Discharge::Fails(RequirementFailure::NotEstablished)
        }
        ConditionOutcome::Failed(cause) => {
            Discharge::Fails(RequirementFailure::from_evaluation(&cause))
        }
    };
    RequirementDischarge {
        outcome,
        predicate: captured_predicate,
    }
}

// A requirement reached again while it is being discharged depends on itself.
fn requirement_cycle_initial<'db>(
    _db: &'db dyn HirAnalysisDb,
    _declaration: WhereClauseOwner<'db>,
    _predicate: Body<'db>,
    _args: Vec<TyId<'db>>,
    _caller: Option<GenericParamOwner<'db>>,
    _context: ImplEnv<'db>,
) -> RequirementDischarge<'db> {
    RequirementDischarge::failure(RequirementFailure::Recursive)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Salsa recovery adds value and iteration count to the query arguments"
)]
fn requirement_cycle_recover<'db>(
    _db: &'db dyn HirAnalysisDb,
    _value: &RequirementDischarge<'db>,
    _count: u32,
    _declaration: WhereClauseOwner<'db>,
    _predicate: Body<'db>,
    _args: Vec<TyId<'db>>,
    _caller: Option<GenericParamOwner<'db>>,
    _context: ImplEnv<'db>,
) -> salsa::CycleRecoveryAction<RequirementDischarge<'db>> {
    salsa::CycleRecoveryAction::Iterate
}

// A predicate reached again while its formation is checked depends on
// itself.
fn formation_cycle_initial<'db>(
    _db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
) -> PredicateFormation<'db> {
    PredicateFormation {
        diags: vec![BodyDiag::RecursiveConstRequirement(body.span().into()).into()],
        status: FormationStatus::Recursive,
    }
}
fn formation_cycle_recover<'db>(
    _db: &'db dyn HirAnalysisDb,
    _value: &PredicateFormation<'db>,
    _count: u32,
    _body: Body<'db>,
) -> salsa::CycleRecoveryAction<PredicateFormation<'db>> {
    salsa::CycleRecoveryAction::Iterate
}

/// The applications that the types written in `owner`'s signature and body
/// carry, as `written_types` lists them for `written_type_check`. That check
/// reports a written type's unmet requirement where the type is written, so a
/// path or expression that carries the same application does not report it
/// again.
fn written_applications<'db>(
    db: &'db dyn HirAnalysisDb,
    owner: BodyOwner<'db>,
) -> FxHashSet<TyId<'db>> {
    fn examined<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>, found: &mut FxHashSet<TyId<'db>>) {
        if found.insert(ty) {
            for &arg in ty.decompose_ty_app(db).1 {
                examined(db, arg, found);
            }
        }
    }
    let signature = match owner {
        BodyOwner::Func(func) => Some(ItemKind::Func(func)),
        BodyOwner::Const(const_) => Some(ItemKind::Const(const_)),
        _ => None,
    };
    let mut found = FxHashSet::default();
    for item in signature
        .into_iter()
        .chain(owner.body(db).map(ItemKind::Body))
    {
        for entry in written_types(db, item) {
            if let WrittenEntry::Type(written) = entry {
                for &ty in written.lowered.iter().chain(&written.applications) {
                    examined(db, ty, &mut found);
                }
            }
        }
    }
    found
}

/// Check every authored type position, including unused defaults and aliases,
/// and the anonymous constants written directly in types and trait
/// references. Each item and body of the module is checked on its own
/// (`written_type_check_query`). Inferred expression types are checked separately
/// after body inference.
pub(crate) fn check_declared_type_requirements<'db>(
    db: &'db dyn HirAnalysisDb,
    top_mod: crate::hir_def::TopLevelMod<'db>,
) -> Vec<FuncBodyDiag<'db>> {
    top_mod
        .all_items(db)
        .iter()
        .flat_map(|&item| {
            let mut diags =
                written_type_check_query(db, WrittenTypeRegion::new(db, item)).diagnostics();
            if let ItemKind::ImplTrait(implementation) = item {
                // Written types check applications in the header. The trait's
                // own predicates are a distinct use of the same Discharge.
                diags.extend(check_impl_trait_header_const_requirements(
                    db,
                    implementation,
                ));
            }
            diags
        })
        .collect()
}

/// A position in an item or body that `written_type_check` checks, in the
/// order it checks them.
#[derive(Debug, Clone, PartialEq, Eq, Update)]
enum WrittenEntry<'db> {
    /// Anonymous constants written in a type or trait reference, each with
    /// the type its position checks it against.
    ConstBodies {
        bodies: Vec<(Body<'db>, TyId<'db>)>,
        expr: Option<ExprId>,
    },
    /// A written type, listed after the types nested in it.
    Type(WrittenType<'db>),
}

#[derive(Debug, Clone, PartialEq, Eq, Update)]
struct WrittenType<'db> {
    span: DynLazySpan<'db>,
    scope: ScopeId<'db>,
    /// The enclosing expression, whose inferred lexical selections apply here.
    expr: Option<ExprId>,
    /// Its lowering, unless that is invalid.
    lowered: Option<TyId<'db>>,
    /// The constrained applications its paths passed through.
    applications: Vec<TyId<'db>>,
    /// The index of the first entry nested in this type.
    nested_from: usize,
}

#[salsa::interned]
struct WrittenTypeRegion<'db> {
    item: ItemKind<'db>,
}

/// The types written in `item` itself, and the anonymous constants written
/// in them. An item or body nested in it is a region of its own, since
/// `TopLevelMod::all_items` lists every item and body of a module, including
/// the default of a const parameter. Listing them only lowers types, so a
/// body's requirement check can read the list while a requirement is being
/// discharged; `written_type_check` discharges.
fn written_types<'db>(db: &'db dyn HirAnalysisDb, item: ItemKind<'db>) -> &'db [WrittenEntry<'db>] {
    written_types_query(db, WrittenTypeRegion::new(db, item))
}

#[salsa::tracked(return_ref)]
fn written_types_query<'db>(
    db: &'db dyn HirAnalysisDb,
    region: WrittenTypeRegion<'db>,
) -> Vec<WrittenEntry<'db>> {
    use crate::hir_def::{TraitRefId, TypeKind};
    use crate::span::types::LazyTySpan;
    use crate::visitor::{
        Visitor, VisitorCtxt,
        prelude::{LazyBodySpan, LazyExprSpan, LazyItemSpan, LazyTraitRefSpan},
        walk_body, walk_expr, walk_item, walk_path, walk_trait_ref, walk_type,
    };
    struct Collector<'db> {
        db: &'db dyn HirAnalysisDb,
        root: ItemKind<'db>,
        entries: Vec<WrittenEntry<'db>>,
        /// Type parameter defaults, and how deep the walk is inside one.
        /// `check_generic_default_bodies` owns the anonymous constants there,
        /// and finds them with the same rule (`written_type_const_args`).
        defaults: FxHashSet<crate::hir_def::TypeId<'db>>,
        default_depth: usize,
        expr: Option<ExprId>,
    }
    impl<'db> Visitor<'db> for Collector<'db> {
        // Nested items and bodies are their own regions.
        fn visit_item(
            &mut self,
            ctxt: &mut VisitorCtxt<'db, LazyItemSpan<'db>>,
            item: ItemKind<'db>,
        ) {
            if item == self.root {
                walk_item(self, ctxt, item);
            }
        }

        fn visit_body(&mut self, ctxt: &mut VisitorCtxt<'db, LazyBodySpan<'db>>, body: Body<'db>) {
            if ItemKind::Body(body) == self.root {
                walk_body(self, ctxt, body);
            }
        }

        fn visit_generic_param(
            &mut self,
            ctxt: &mut VisitorCtxt<'db, crate::visitor::prelude::LazyGenericParamSpan<'db>>,
            param: &crate::hir_def::GenericParam<'db>,
        ) {
            if let crate::hir_def::GenericParam::Type(param) = param
                && let Some(default) = param.default_ty
            {
                self.defaults.insert(default);
            }
            crate::visitor::walk_generic_param(self, ctxt, param);
        }

        fn visit_trait_ref(
            &mut self,
            ctxt: &mut VisitorCtxt<'db, LazyTraitRefSpan<'db>>,
            trait_ref: TraitRefId<'db>,
        ) {
            if let Some(path) = trait_ref.path(self.db).to_opt()
                && self.default_depth == 0
            {
                let scope = ctxt.scope();
                let bodies = trait_ref_const_args(
                    self.db,
                    path,
                    scope,
                    crate::semantic::context_at(self.db, scope, None).assumptions(self.db),
                );
                if !bodies.is_empty() {
                    self.entries.push(WrittenEntry::ConstBodies {
                        bodies,
                        expr: self.expr,
                    });
                }
            }
            walk_trait_ref(self, ctxt, trait_ref);
        }

        fn visit_ty(
            &mut self,
            ctxt: &mut VisitorCtxt<'db, LazyTySpan<'db>>,
            hir_ty: crate::hir_def::TypeId<'db>,
        ) {
            let scope = ctxt.scope();
            let span = ctxt.span().map(Into::into);
            self.written_type(hir_ty, scope, span, |this| walk_type(this, ctxt, hir_ty));
        }

        // A `with` key is a type written in the body, whose paths are
        // resolved like any other written type's.
        fn visit_expr(
            &mut self,
            ctxt: &mut VisitorCtxt<'db, LazyExprSpan<'db>>,
            expr: ExprId,
            expr_data: &Expr<'db>,
        ) {
            let saved_expr = self.expr.replace(expr);
            if let Expr::With(bindings, _) = expr_data {
                for (idx, binding) in bindings.iter().enumerate() {
                    let Some(Partial::Present(key)) = binding.key_path else {
                        continue;
                    };
                    let hir_ty =
                        crate::hir_def::TypeId::new(self.db, TypeKind::Path(Partial::Present(key)));
                    let span = LazyExprSpan::new(ctxt.body(), expr)
                        .into_with_expr()
                        .params()
                        .param(idx)
                        .path();
                    let mut key_ctxt = VisitorCtxt::new(self.db, ctxt.scope(), span.clone());
                    self.written_type(hir_ty, ctxt.scope(), Some(span.into()), |this| {
                        walk_path(this, &mut key_ctxt, key)
                    });
                }
            }
            walk_expr(self, ctxt, expr);
            self.expr = saved_expr;
        }
    }
    impl<'db> Collector<'db> {
        /// Lists a written type at `span`: the anonymous constants written in
        /// it, then the types nested in it, which `walk_nested` visits, then
        /// the type itself.
        fn written_type(
            &mut self,
            hir_ty: crate::hir_def::TypeId<'db>,
            scope: ScopeId<'db>,
            span: Option<DynLazySpan<'db>>,
            walk_nested: impl FnOnce(&mut Self),
        ) {
            let db = self.db;
            let assumptions = crate::semantic::context_at(db, scope, None).assumptions(db);
            let (ty, applications) = match hir_ty.data(db) {
                // Only a path of two or more segments passes through types
                // that its lowering does not hold. A one-segment path lowers
                // to what it resolves to, its arguments are written types or
                // anonymous constants of their own, and an alias's arguments
                // are checked after expansion or as constants
                // (`written_const_args`).
                TypeKind::Path(Partial::Present(path)) if path.len(db) > 1 => {
                    let (ty, resolutions) = lower_hir_ty_with_resolutions(
                        db,
                        hir_ty,
                        scope,
                        assumptions,
                        ConstBodyLowering::Eager,
                    );
                    let applications = resolutions
                        .iter()
                        .flat_map(|(_, res)| constrained_applications(db, res))
                        .collect();
                    (ty, applications)
                }
                _ => (lower_hir_ty(db, hir_ty, scope, assumptions), Vec::new()),
            };
            let in_default = self.defaults.contains(&hir_ty);
            self.default_depth += usize::from(in_default);
            if self.default_depth == 0 {
                let bodies = written_const_args(db, hir_ty, scope, assumptions);
                if !bodies.is_empty() {
                    self.entries.push(WrittenEntry::ConstBodies {
                        bodies,
                        expr: self.expr,
                    });
                }
            }
            let nested_from = self.entries.len();
            walk_nested(self);
            if let Some(span) = span {
                self.entries.push(WrittenEntry::Type(WrittenType {
                    span,
                    scope,
                    expr: self.expr,
                    lowered: (!ty.has_invalid(db)).then_some(ty),
                    applications,
                    nested_from,
                }));
            }
            self.default_depth -= usize::from(in_default);
        }
    }
    let root = region.item(db);
    let mut collector = Collector {
        db,
        root,
        entries: Vec::new(),
        defaults: FxHashSet::default(),
        default_depth: 0,
        expr: None,
    };
    collector.visit_item(&mut VisitorCtxt::with_item(db, root), root);
    collector.entries
}

/// Read the actual results retained by authored-type checking.
/// The boundary applies even when the tracked query is already cached.
pub fn written_condition_uses<'db>(
    db: &'db dyn HirAnalysisDb,
    item: ItemKind<'db>,
) -> impl ExactSizeIterator<Item = &'db ConstConditionUse<'db>> {
    crate::analysis::ty::trait_resolution::assert_outside_proof_search();
    written_type_check_query(db, WrittenTypeRegion::new(db, item))
        .conditions
        .values()
}

#[salsa::tracked(return_ref)]
fn written_type_check_query<'db>(
    db: &'db dyn HirAnalysisDb,
    region: WrittenTypeRegion<'db>,
) -> RequirementCheck<'db> {
    let entries = written_types(db, region.item(db));
    // Listing written types remains independent of inference: the body's
    // requirement check reads that list itself. Discharge alone reads the
    // completed inference template for lexical implementation selections.
    let typed = match region.item(db) {
        ItemKind::Body(body)
            if body
                .exprs(db)
                .iter()
                .any(|(_, expr)| matches!(expr.borrowed().to_opt(), Some(Expr::With(..)))) =>
        {
            let owner = match body.scope().parent(db) {
                Some(ScopeId::Item(ItemKind::Func(func))) if func.body(db) == Some(body) => {
                    Some(BodyOwner::Func(func))
                }
                Some(ScopeId::Item(ItemKind::Const(constant)))
                    if constant.body(db).to_opt() == Some(body) =>
                {
                    Some(BodyOwner::Const(constant))
                }
                _ => None,
            };
            owner.map(|owner| &infer_body(db, owner).1)
        }
        _ => None,
    };
    let mut check = RequirementCheck::default();
    // Applications completed by each nested entry, including successful checks.
    let mut reported: Vec<Vec<TyId<'db>>> = Vec::with_capacity(entries.len());
    for entry in entries {
        let report = match entry {
            WrittenEntry::ConstBodies { bodies, expr } => {
                let selections = expr
                    .and_then(|expr| {
                        typed.map(|typed| {
                            crate::semantic::context_at(
                                db,
                                region.item(db).scope(),
                                Some((typed, expr)),
                            )
                            .selections(db)
                            .as_slice()
                        })
                    })
                    .unwrap_or(&[]);
                for &(body, expected) in bodies {
                    check.extend(anon_const_position_check(db, body, expected, selections));
                }
                Vec::new()
            }
            WrittenEntry::Type(written) => {
                let context =
                    crate::semantic::context_at(db, written.scope, typed.zip(written.expr));
                let selections = context.selections(db);
                let solve_cx = TraitSolveCx::new(db, context.normalization_scope(db))
                    .with_assumptions(context.assumptions(db));
                let mut uses = Vec::new();
                for ty in written.lowered.iter().chain(&written.applications) {
                    if let Some(conditions) =
                        crate::analysis::ty::trait_resolution::check_ty_wf(db, solve_cx, *ty)
                            .conditions()
                    {
                        uses.extend(selected_impl_condition_uses(
                            db,
                            conditions,
                            written.scope,
                            written.span.clone(),
                            selections,
                        ));
                    }
                }
                check.extend(check_recorded_condition_uses(db, &uses));
                let nested: FxHashSet<_> = reported[written.nested_from..]
                    .iter()
                    .flatten()
                    .copied()
                    .collect();
                let unmet = written
                    .lowered
                    .and_then(|ty| {
                        check_selected_type_requirements(
                            db,
                            ty,
                            written.scope,
                            &nested,
                            selections,
                            &mut ConditionCapture {
                                site: written.span.clone(),
                                conditions: &mut check.conditions,
                            },
                        )
                    })
                    .or_else(|| {
                        written.applications.iter().find_map(|&application| {
                            check_path_application(
                                db,
                                application,
                                written.scope,
                                &nested,
                                selections,
                                &mut ConditionCapture {
                                    site: written.span.clone(),
                                    conditions: &mut check.conditions,
                                },
                            )
                        })
                    });
                if let Some(unmet) = unmet {
                    let ty = unmet.ty;
                    check.report_type(unmet, false);
                    vec![ty]
                } else {
                    written
                        .lowered
                        .iter()
                        .chain(&written.applications)
                        .copied()
                        .collect()
                }
            }
        };
        reported.push(report);
    }
    check
}

/// The anonymous constants written in `hir_ty` and in every type and trait
/// reference nested in it, each with the type its position checks it
/// against. This is the rule `written_types` applies to each type and trait
/// reference it lists (`written_const_args`, `trait_ref_const_args`), for a
/// type parameter's default, whose declaration check owns its constants
/// (`check_generic_default_bodies`). A default's lowering does not hold every
/// constant written in it: an alias can drop one, and a qualifier such as
/// `Holder<{ n }>` in `Holder<{ n }>::Out` may only select an impl.
pub(super) fn written_type_const_args<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: crate::hir_def::TypeId<'db>,
    span: crate::span::types::LazyTySpan<'db>,
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
) -> Vec<(Body<'db>, TyId<'db>)> {
    use crate::hir_def::TraitRefId;
    use crate::span::types::LazyTySpan;
    use crate::visitor::{
        Visitor, VisitorCtxt,
        prelude::{LazyBodySpan, LazyTraitRefSpan},
        walk_trait_ref, walk_type,
    };
    struct Finder<'db> {
        db: &'db dyn HirAnalysisDb,
        assumptions: PredicateListId<'db>,
        found: Vec<(Body<'db>, TyId<'db>)>,
    }
    impl<'db> Visitor<'db> for Finder<'db> {
        fn visit_ty(
            &mut self,
            ctxt: &mut VisitorCtxt<'db, LazyTySpan<'db>>,
            hir_ty: crate::hir_def::TypeId<'db>,
        ) {
            self.found.extend(written_const_args(
                self.db,
                hir_ty,
                ctxt.scope(),
                self.assumptions,
            ));
            walk_type(self, ctxt, hir_ty);
        }

        fn visit_trait_ref(
            &mut self,
            ctxt: &mut VisitorCtxt<'db, LazyTraitRefSpan<'db>>,
            trait_ref: TraitRefId<'db>,
        ) {
            if let Some(path) = trait_ref.path(self.db).to_opt() {
                self.found.extend(trait_ref_const_args(
                    self.db,
                    path,
                    ctxt.scope(),
                    self.assumptions,
                ));
            }
            walk_trait_ref(self, ctxt, trait_ref);
        }

        // An anonymous constant's own body is checked as that constant.
        fn visit_body(&mut self, _: &mut VisitorCtxt<'db, LazyBodySpan<'db>>, _: Body<'db>) {}
    }
    let mut finder = Finder {
        db,
        assumptions,
        found: Vec::new(),
    };
    finder.visit_ty(&mut VisitorCtxt::new(db, scope, span), hir_ty);
    finder.found
}

/// The anonymous constants passed to the segments of a trait reference's
/// path, such as `{ n }` in `Tr<{ n }>`, each with the type of the parameter
/// it is passed to (`segment_const_args`).
fn trait_ref_const_args<'db>(
    db: &'db dyn HirAnalysisDb,
    path: PathId<'db>,
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
) -> Vec<(Body<'db>, TyId<'db>)> {
    if path_const_bodies(db, path).is_empty() {
        return Vec::new();
    }
    let minter = LoweringContext::deferred(HoleAnchor::TemplatePath {
        path,
        scope,
        assumptions,
    })
    .recording_resolutions();
    let _ = resolve_path_with_minter(db, path, scope, assumptions, false, &minter);
    segment_const_args(db, path, &minter.into_resolutions())
}

/// The anonymous constants written directly in `hir_ty`, each with the type
/// its position checks it against: an array's length, or the arguments of its
/// path's segments (`segment_const_args`).
fn written_const_args<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: crate::hir_def::TypeId<'db>,
    scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
) -> Vec<(Body<'db>, TyId<'db>)> {
    use crate::hir_def::TypeKind;
    match hir_ty.data(db) {
        TypeKind::Array(_, Partial::Present(len)) => array_length_ty(db)
            .map(|expected| (*len, expected))
            .into_iter()
            .collect(),
        TypeKind::Path(Partial::Present(path)) if !path_const_bodies(db, *path).is_empty() => {
            let (_, resolutions) = lower_hir_ty_with_resolutions(
                db,
                hir_ty,
                scope,
                assumptions,
                ConstBodyLowering::Deferred,
            );
            segment_const_args(db, *path, &resolutions)
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod recording_tests {
    use crate::test_db::HirAnalysisTestDb;

    #[test]
    fn context_view_reads_the_instantiated_body_root_premises() {
        use crate::analysis::semantic::{
            EffectProviderSubst, GenericSubst, ImplEnv, SemanticInstanceKey,
        };
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "instantiated_context_root.fe".into(),
            "trait Mark {}\nimpl Mark for u8 {}\nfn caller<T: Mark>() {}",
        );
        let (top, _) = db.top_mod(file);
        let caller = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| {
                func.name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "caller")
            })
            .expect("caller function");
        let key = SemanticInstanceKey::new(
            &db,
            super::BodyOwner::Func(caller),
            GenericSubst::for_owner(&db, caller.into(), vec![super::TyId::u8(&db)]),
            EffectProviderSubst::empty(&db),
            ImplEnv::empty(&db, caller.scope()),
        );
        let typed = key.typed_body(&db);
        let body = typed.body().expect("instantiated body");
        let actual = crate::semantic::context_at(&db, body.scope(), Some((typed, body.expr(&db))))
            .assumptions(&db);
        assert_eq!(typed.assumptions().list(&db).len(), 1);
        assert_eq!(
            typed.assumptions().list(&db)[0].self_ty(&db),
            super::TyId::u8(&db)
        );
        assert_eq!(
            actual,
            typed.assumptions(),
            "a position must read its instantiated root"
        );
    }

    #[test]
    fn authored_types_and_param_env_share_trait_method_self_premise() {
        use crate::analysis::ty::trait_resolution::{
            GoalSatisfiability, TraitSolveCx, is_goal_satisfiable,
        };
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "trait_method_context.fe".into(),
            "trait HasSize { const N: usize
fn inspect() { let _: [u8; Self::N] } }",
        );
        let (top, _) = db.top_mod(file);
        let method = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|f| {
                f.name(&db)
                    .to_opt()
                    .is_some_and(|n| n.data(&db) == "inspect")
            })
            .expect("trait method");
        let Some(super::ItemKind::Trait(trait_)) = method.scope().parent_item(&db) else {
            panic!("expected enclosing trait");
        };
        let goal = crate::semantic::trait_self_predicate(&db, trait_);
        let scope = method.body(&db).unwrap().scope();
        let written = crate::semantic::context_at(&db, scope, None).assumptions(&db);
        let parameter = crate::semantic::param_env(&db, method.into());
        let solve = |assumptions| {
            is_goal_satisfiable(
                &db,
                TraitSolveCx::new(&db, scope).with_assumptions(assumptions),
                goal,
            )
        };
        let const_body = crate::semantic::body_assumptions_at(&db, scope);
        let declared = crate::analysis::ty::trait_resolution::constraint::collect_constraints(
            &db,
            method.into(),
        )
        .instantiate_identity();
        assert!(matches!(solve(parameter), GoalSatisfiability::Satisfied(_)));
        assert!(matches!(
            solve(const_body),
            GoalSatisfiability::Satisfied(_)
        ));
        // Declaration bounds alone do not establish the implicit body premise.
        assert!(matches!(solve(declared), GoalSatisfiability::UnSat(_)));
        assert!(
            matches!(solve(written), GoalSatisfiability::Satisfied(_)),
            "authored type context lost the trait method's Self premise: {:?}",
            solve(written)
        );
    }

    #[test]
    fn context_view_reads_nested_choices_and_restores_the_outer_position() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "context_positions.fe".into(),
            "trait Compute {}
struct Subject {}
impl Compute for Subject {}
impl Compute for Subject as First {}
impl Compute for Subject as Second {}
fn outer<T: Compute>() {}
fn inner<T: Compute>() {}
fn restored<T: Compute>() {}
fn outside<T: Compute>() {}
fn caller() {
    with (First) {
        outer<Subject>()
        with (Second) { inner<Subject>() }
        restored<Subject>()
    }
    outside<Subject>()
}",
        );
        let (top, _) = db.top_mod(file);
        let caller = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| {
                func.name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "caller")
            })
            .unwrap();
        let owner = super::super::BodyOwner::Func(caller);
        let inferred = super::super::inferred_body_with_selections(&db, owner, &[]);
        assert!(inferred.result.0.is_empty(), "{:?}", inferred.result.0);
        let typed = &inferred.result.1;
        let openings = typed
            .with_choices()
            .map(|choice| match choice.outcome().unwrap() {
                crate::core::semantic::Provision::Named(selection) => selection,
                _ => panic!("expected named opening"),
            })
            .collect::<Vec<_>>();
        assert_eq!(openings.len(), 2);
        let body = caller.body(&db).unwrap();
        let mut checked = 0;
        for expr in body.exprs(&db).keys() {
            let Some(callable) = typed.callable_expr(expr) else {
                continue;
            };
            let crate::hir_def::CallableDef::Func(func) = callable.callable_def() else {
                continue;
            };
            let name = func.name(&db).to_opt().unwrap();
            let context = crate::semantic::context_at(&db, body.scope(), Some((typed, expr)));
            let expected = match name.data(&db).as_str() {
                "outer" | "restored" => &openings[..1],
                "inner" => &openings[1..],
                "outside" => &[],
                _ => continue,
            };
            assert_eq!(context.selections(&db), expected, "{}", name.data(&db));
            let reconstructed = crate::semantic::recorded_chain_at(&db, typed, expr);
            let selections = reconstructed
                .visible(&db)
                .filter_map(|provision| match provision {
                    crate::core::semantic::Provision::Named(selection) => Some(selection),
                    crate::core::semantic::Provision::Evidence(..) => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(selections, expected, "reconstructed {}", name.data(&db));
            assert_eq!(
                context.assumptions(&db),
                crate::semantic::param_env(&db, caller.into())
            );
            checked += 1;
        }
        assert_eq!(checked, 4);
    }

    #[test]
    fn context_view_reads_evidence_frames_and_restores_outer_premises() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "evidence_context_positions.fe".into(),
            "use core::obligations::Evidence
trait Compute {}
trait Inspect {}
const fn outer<T>() {}
const fn inner<T>() {}
const fn restored<T>() {}
const fn outside<T>() {}
const fn caller<T>(_ first: Evidence<Compute<T>>, _ second: Evidence<Inspect<T>>) {
    with (first) {
        outer<T>()
        with (second) { inner<T>() }
        restored<T>()
    }
    outside<T>()
}",
        );
        let (top, _) = db.top_mod(file);
        let caller = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| {
                func.name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "caller")
            })
            .unwrap();
        let owner = super::super::BodyOwner::Func(caller);
        let inferred = super::super::inferred_body_with_selections(&db, owner, &[]);
        assert!(inferred.result.0.is_empty(), "{:?}", inferred.result.0);
        let typed = &inferred.result.1;
        let goals = typed
            .with_choices()
            .map(|choice| match choice.outcome().unwrap() {
                crate::core::semantic::Provision::Evidence(_, goal) => goal,
                _ => panic!("expected evidence opening"),
            })
            .collect::<Vec<_>>();
        assert_eq!(goals.len(), 2);
        let body = caller.body(&db).unwrap();
        let canonical = crate::semantic::param_env(&db, caller.into());
        assert!(goals.iter().all(|goal| !canonical.list(&db).contains(goal)));
        // An opening does not establish its own premise while its value is evaluated.
        // The inner opening still sees evidence introduced by the outer frame.
        for choice in typed.with_choices() {
            let crate::core::semantic::Provision::Evidence(_, goal) = choice.outcome().unwrap()
            else {
                unreachable!();
            };
            let expected = if goal == goals[0] {
                &[][..]
            } else {
                &goals[..1]
            };
            let mut premises = canonical.list(&db).to_vec();
            premises.extend_from_slice(expected);
            let expected =
                crate::analysis::ty::PredicateListId::new(&db, premises).extend_all_bounds(&db);
            let context =
                crate::semantic::context_at(&db, body.scope(), Some((typed, choice.expression())));
            assert_eq!(context.assumptions(&db), expected, "opening prefix");
        }
        let mut checked = 0;
        for expr in body.exprs(&db).keys() {
            let Some(callable) = typed.callable_expr(expr) else {
                continue;
            };
            let crate::hir_def::CallableDef::Func(func) = callable.callable_def() else {
                continue;
            };
            let name = func.name(&db).to_opt().unwrap();
            let expected = match name.data(&db).as_str() {
                "outer" | "restored" => &goals[..1],
                "inner" => goals.as_slice(),
                "outside" => &[],
                _ => continue,
            };
            let context = crate::semantic::context_at(&db, body.scope(), Some((typed, expr)));
            let mut premises = canonical.list(&db).to_vec();
            premises.extend_from_slice(expected);
            let expected =
                crate::analysis::ty::PredicateListId::new(&db, premises).extend_all_bounds(&db);
            assert_eq!(context.assumptions(&db), expected, "{}", name.data(&db));
            assert!(context.selections(&db).is_empty());
            checked += 1;
        }
        assert_eq!(checked, 4);
    }

    #[test]
    fn recorded_context_view_preserves_inherited_selection_without_local_openings() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "inherited_context_positions.fe".into(),
            "trait Compute {}
struct Subject {}
impl Compute for Subject {}
impl Compute for Subject as First {}
const fn target<T: Compute>() {}
const fn source() { with (First) { target<Subject>() } }
const fn inherited() { target<Subject>() }",
        );
        let (top, _) = db.top_mod(file);
        let find = |name: &str| {
            top.all_funcs(&db)
                .iter()
                .copied()
                .find(|func| {
                    func.name(&db)
                        .to_opt()
                        .is_some_and(|ident| ident.data(&db) == name)
                })
                .unwrap()
        };
        let source = super::super::inferred_body_with_selections(
            &db,
            super::super::BodyOwner::Func(find("source")),
            &[],
        );
        assert!(source.result.0.is_empty(), "{:?}", source.result.0);
        let selection = match source
            .result
            .1
            .with_choices()
            .next()
            .unwrap()
            .outcome()
            .unwrap()
        {
            crate::core::semantic::Provision::Named(selection) => selection,
            _ => panic!("expected named opening"),
        };
        let inherited = find("inherited");
        let result = super::super::inferred_body_with_selections(
            &db,
            super::super::BodyOwner::Func(inherited),
            &[selection],
        );
        assert!(result.result.0.is_empty(), "{:?}", result.result.0);
        let typed = &result.result.1;
        assert_eq!(typed.with_choices().len(), 0);
        assert!(typed.trait_choices(&db).any(|choice| {
            matches!(choice.via(), super::super::TraitChoiceVia::Selected(selected)
                if selected == selection)
        }));
        let body = inherited.body(&db).unwrap();
        let mut checked = 0;
        for expr in body.exprs(&db).keys() {
            let Some(callable) = typed.callable_expr(expr) else {
                continue;
            };
            let crate::hir_def::CallableDef::Func(func) = callable.callable_def() else {
                continue;
            };
            if func.name(&db).to_opt().unwrap().data(&db) != "target" {
                continue;
            }
            let existing = crate::semantic::context_at(&db, body.scope(), Some((typed, expr)));
            assert_eq!(existing.selections(&db), &[selection]);
            let chain = crate::semantic::recorded_chain_at(&db, typed, expr);
            let reconstructed = chain
                .visible(&db)
                .filter_map(|provision| match provision {
                    crate::core::semantic::Provision::Named(selection) => Some(selection),
                    crate::core::semantic::Provision::Evidence(..) => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                reconstructed,
                existing.selections(&db).as_slice(),
                "inherited root selection"
            );
            checked += 1;
        }
        assert_eq!(checked, 1);
    }

    #[test]
    fn const_opened_dispatch_reads_record_after_callable_trait_link_is_cleared() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "const_opened_record.fe".into(),
            "use core::obligations::Evidence
trait Read { const fn read(_ x: u256) -> u256 }
const fn caller<T>(_ evidence: Evidence<Read<T>>) -> u256 {
    with (evidence) { <T as Read>::read(1) }
}",
        );
        let (top, _) = db.top_mod(file);
        let caller = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|func| {
                func.name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "caller")
            })
            .unwrap();
        let result = super::super::inferred_body_with_selections(
            &db,
            super::super::BodyOwner::Func(caller),
            &[],
        );
        assert_eq!(
            result.result.0.len(),
            1,
            "the unsupported opened call must reject"
        );
        let mut typed = result.result.1.clone();
        let before = crate::analysis::ty::const_check::check_const_fn_body(&db, caller, &typed);
        assert!(
            !before.is_empty(),
            "opened method with an argument must reject"
        );
        let mut cleared = 0;
        for lowering in std::sync::Arc::make_mut(&mut typed.tables)
            .semantic_expr_lowering
            .values_mut()
        {
            if let Some(super::super::SemanticExprLowering::Call { callable }) = lowering
                && callable.trait_inst.is_some()
            {
                callable.trait_inst = None;
                cleared += 1;
            }
        }
        assert_eq!(cleared, 1);
        let after = crate::analysis::ty::const_check::check_const_fn_body(&db, caller, &typed);
        assert_eq!(
            after.len(),
            before.len(),
            "the accepted opening record must remain authoritative"
        );
    }

    #[test]
    fn requirement_reports_read_the_retained_outcome() {
        let source = "fn read<const N: usize>() -> u256 where N > 0 { 7 }\nfn caller() -> u256 { read<0>() }\n";
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("requirement_report.fe".into(), source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "caller")
            })
            .expect("caller function");
        let owner = super::super::BodyOwner::Func(function);
        let inferred = super::super::inferred_body_with_selections(&db, owner, &[]);
        assert!(inferred.result.0.is_empty());
        let mut check =
            super::check_body_requirements(&db, owner, &inferred.result.1, &inferred.conditions);
        let use_ = check
            .conditions
            .values_mut()
            .find(|use_| {
                use_.outcome() == super::Discharge::Fails(super::RequirementFailure::False)
            })
            .expect("actual failed callee condition");
        // An independent mutation control: consumers must read the one result,
        // rather than a diagnostic and flag copied when it was first computed.
        use_.discharge.as_mut().unwrap().outcome =
            super::Discharge::Fails(super::RequirementFailure::Recursive);
        assert!(
            check.recursive(),
            "recursion must project the retained outcome"
        );
        assert!(
            check.diagnostics().iter().any(|diag| matches!(
                diag,
                super::FuncBodyDiag::Body(super::BodyDiag::ConstRequirementNotSatisfied {
                    reason: super::RequirementFailure::Recursive,
                    ..
                })
            )),
            "diagnostics must project the retained outcome"
        );
    }

    #[test]
    fn unchecked_body_has_no_condition_decisions() {
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/impl_const_requirements/recording_owners.fe"
        ));
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("unchecked_decisions.fe".into(), source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "one_call")
            })
            .expect("fixture function");
        let (_, inferred) = super::super::infer_body(&db, super::super::BodyOwner::Func(function));
        assert!(
            inferred.decisions.condition_uses.is_empty(),
            "pending inference inputs are not completed decisions"
        );
        let (diagnostics, checked) = super::super::check_func_body(&db, function);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(
            checked.checked_conditions().len(),
            1,
            "discharge still consumes the inference inputs"
        );
        assert!(
            inferred.decisions.condition_uses.is_empty(),
            "checking does not mutate the inference template"
        );
    }

    #[test]
    fn implementation_conditions_have_one_record_per_use() {
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/impl_const_requirements/recording_owners.fe"
        ));
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("recording_owners.fe".into(), source);
        let (top, _) = db.top_mod(file);
        for (name, expected) in [("one_call", 1), ("two_calls", 2), ("two_values", 2)] {
            let function = top
                .all_funcs(&db)
                .iter()
                .copied()
                .find(|function| {
                    function
                        .name(&db)
                        .to_opt()
                        .is_some_and(|id| id.data(&db) == name)
                })
                .expect("fixture function");
            let (diagnostics, typed) = super::super::check_func_body(&db, function);
            assert!(diagnostics.is_empty(), "{name}: {diagnostics:?}");
            assert_eq!(
                typed.checked_conditions().len(),
                expected,
                "{name}: each application must record its implementation condition once"
            );
            let choices: Vec<_> = typed
                .trait_choices(&db)
                .filter(|choice| {
                    choice
                        .goal()
                        .def(&db)
                        .name(&db)
                        .to_opt()
                        .is_some_and(|name| name.data(&db) == "Read")
                })
                .collect();
            assert_eq!(
                choices.len(),
                expected,
                "{name}: one accepted choice per use"
            );
            assert!(choices.iter().all(|choice| matches!(
                choice.via(),
                super::super::env::TraitChoiceVia::Implementation(_)
            )));
            assert_eq!(typed.checked_conditions().len(), expected);
            assert!(typed.checked_conditions().all(|checked| matches!(
                checked.outcome(),
                super::Discharge::Holds(super::RequirementProof::Evaluated)
            )));
        }
    }

    #[test]
    fn checked_body_retains_successful_and_failed_callee_conditions() {
        let cases = [
            ("concrete_true", "fn use_it() -> u256 { read<7>() }", None),
            (
                "concrete_false",
                "fn use_it() -> u256 { read<0>() }",
                Some(super::RequirementFailure::False),
            ),
            (
                "forward_premise",
                "fn use_it<const N: usize>() -> u256 where N > 0 { read<N>() }",
                None,
            ),
            (
                "missing_premise",
                "fn use_it<const N: usize>() -> u256 { read<N>() }",
                Some(super::RequirementFailure::NoMatchingPremise),
            ),
        ];
        for (name, caller, failure) in cases {
            let source =
                format!("fn read<const N: usize>() -> u256 where N > 0 {{ 7 }}\n{caller}\n");
            let mut db = HirAnalysisTestDb::default();
            let file = db.new_stand_alone(format!("{name}.fe").into(), &source);
            let (top, _) = db.top_mod(file);
            let function = top
                .all_funcs(&db)
                .iter()
                .copied()
                .find(|function| {
                    function
                        .name(&db)
                        .to_opt()
                        .is_some_and(|id| id.data(&db) == "use_it")
                })
                .expect("fixture function");
            let (diagnostics, typed) = super::super::check_func_body(&db, function);
            assert_eq!(
                diagnostics.is_empty(),
                failure.is_none(),
                "{name}: {diagnostics:?}"
            );
            assert_eq!(typed.checked_conditions().len(), 1, "{name}");
            let checked = typed.checked_conditions().next().unwrap();
            let predicate = checked
                .predicate()
                .expect("substituted predicate before evaluation");
            let super::PredicateTerm::Binary(
                super::BinOp::Comp(crate::hir_def::CompBinOp::Gt),
                lhs,
                _,
            ) = &predicate.term
            else {
                panic!("{name}: predicate was replaced by its truth value");
            };
            if name.starts_with("concrete") {
                let super::PredicateTerm::TypeConst(ty) = lhs.term else {
                    panic!("{name}: missing substituted operand");
                };
                assert!(!ty.has_param(&db), "{name}: argument was not substituted");
            }
            match failure {
                Some(failure) => {
                    assert_eq!(
                        checked.outcome(),
                        super::Discharge::Fails(failure),
                        "{name}"
                    )
                }
                None if name == "forward_premise" => {
                    let premise = super::WhereClauseOwner::Func(function)
                        .where_clause(&db)
                        .const_predicates(&db)[0];
                    assert_eq!(
                        checked.outcome(),
                        super::Discharge::Holds(super::RequirementProof::Premise(premise))
                    );
                }
                None => assert_eq!(
                    checked.outcome(),
                    super::Discharge::Holds(super::RequirementProof::Evaluated)
                ),
            }
        }
    }

    #[test]
    fn folded_body_preserves_checked_condition_records() {
        use crate::analysis::ty::fold::{TyFoldable, TyFolder};
        struct Identity;
        impl<'db> TyFolder<'db> for Identity {
            fn fold_ty(
                &mut self,
                _: &'db dyn crate::analysis::HirAnalysisDb,
                ty: crate::analysis::ty::ty_def::TyId<'db>,
            ) -> crate::analysis::ty::ty_def::TyId<'db> {
                ty
            }
        }
        let source = "fn read<const N: usize>() -> u256 where N > 0 { 7 }
fn use_it() -> u256 { read<7>() }";
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("folded_conditions.fe".into(), source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|f| {
                f.name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "use_it")
            })
            .unwrap();
        let (_, inferred) = super::super::infer_body(&db, super::super::BodyOwner::Func(function));
        assert!(inferred.checked_conditions().len() == 0);
        let (diags, typed) = super::super::check_func_body(&db, function);
        assert!(std::sync::Arc::ptr_eq(&typed.tables, &inferred.tables));
        assert!(inferred.checked_conditions().len() == 0);
        assert!(diags.is_empty(), "{diags:?}");
        assert_eq!(typed.checked_conditions().len(), 1);
        let folded = typed.clone().fold_with(&db, &mut Identity);
        assert_eq!(
            folded.checked_conditions().collect::<Vec<_>>(),
            typed.checked_conditions().collect::<Vec<_>>()
        );
    }

    #[test]
    fn written_type_query_retains_success_and_failure_at_the_authored_span() {
        use crate::span::LazySpan;
        for (argument, expected) in [
            (
                7,
                super::Discharge::Holds(super::RequirementProof::Evaluated),
            ),
            (0, super::Discharge::Fails(super::RequirementFailure::False)),
        ] {
            let source = format!(
                "struct Bounded<const N: usize> where N > 0 {{}}\nfn use_it(_ value: Bounded<{argument}>) {{}}\n"
            );
            let mut db = HirAnalysisTestDb::default();
            let file = db.new_stand_alone("written_capture.fe".into(), &source);
            let (top, _) = db.top_mod(file);
            let function = top
                .all_funcs(&db)
                .iter()
                .copied()
                .find(|function| {
                    function
                        .name(&db)
                        .to_opt()
                        .is_some_and(|id| id.data(&db) == "use_it")
                })
                .expect("fixture function");
            let check = super::written_type_check_query(
                &db,
                super::WrittenTypeRegion::new(&db, super::ItemKind::Func(function)),
            );
            assert_eq!(check.conditions.len(), 1, "argument {argument}");
            assert_eq!(check.conditions.get_index(0).unwrap().1.outcome(), expected);
            assert_eq!(check.diagnostics().is_empty(), argument > 0);
            let span = check
                .conditions
                .get_index(0)
                .unwrap()
                .1
                .span()
                .resolve(&db)
                .expect("authored type span");
            let start = u32::from(span.range.start()) as usize;
            assert!(span.file.text(&db)[start..].starts_with("Bounded<"));
        }
    }

    #[test]
    fn selected_and_bound_goals_retain_the_actual_answer() {
        let source = "trait Read { fn read() -> u256 }\nstruct S {}\nimpl Read for S { fn read() -> u256 { 1 } }\nimpl Read for S as Alternative { fn read() -> u256 { 2 } }\nfn selected() -> u256 { with (Alternative) { <S as Read>::read() } }\nfn bound<T: Read>() -> u256 { <T as Read>::read() }\n";
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("actual_choices.fe".into(), source);
        let (top, _) = db.top_mod(file);
        for name in ["selected", "bound"] {
            let function = top
                .all_funcs(&db)
                .iter()
                .copied()
                .find(|function| {
                    function
                        .name(&db)
                        .to_opt()
                        .is_some_and(|id| id.data(&db) == name)
                })
                .expect("fixture function");
            let (diagnostics, typed) = super::super::check_func_body(&db, function);
            assert!(diagnostics.is_empty(), "{name}: {diagnostics:?}");
            let choices: Vec<_> = typed
                .trait_choices(&db)
                .filter(|choice| {
                    choice
                        .goal()
                        .def(&db)
                        .name(&db)
                        .to_opt()
                        .is_some_and(|id| id.data(&db) == "Read")
                })
                .collect();
            assert_eq!(choices.len(), 1, "{name}");
            assert_eq!(typed.with_choices().len(), usize::from(name == "selected"));
            if name == "selected" {
                let with_selection = typed
                    .with_choices()
                    .next()
                    .expect("with choice")
                    .outcome()
                    .expect("accepted binding");
                let crate::core::semantic::Provision::Named(with_selection) = with_selection else {
                    panic!("expected named provision");
                };
                assert_eq!(
                    choices[0].via(),
                    super::super::env::TraitChoiceVia::Selected(with_selection)
                );
            }
            match (name, choices[0].via()) {
                ("selected", super::super::env::TraitChoiceVia::Selected(_)) => {}
                ("bound", super::super::env::TraitChoiceVia::Implementation(implementor)) => {
                    assert_eq!(
                        implementor.origin(&db),
                        super::ImplementorOrigin::Assumption
                    )
                }
                _ => panic!("{name}: wrong actual proof"),
            }
        }
    }

    #[test]
    fn discarded_probe_does_not_retain_a_with_choice() {
        let source = "trait Read { fn read() -> u256 }\nstruct S {}\nimpl Read for S as Alternative { fn read() -> u256 { 2 } }\nfn use_it() -> u256 { with (Alternative) { 0 } }\n";
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("discarded_with_capture.fe".into(), source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|id| id.data(&db) == "use_it")
            })
            .expect("fixture function");
        let mut checker =
            super::super::TyChecker::new(&db, super::BodyOwner::Func(function)).expect("checker");
        let snapshot = checker.snapshot_state();
        let root = checker.body().expr(&db);
        checker.check_expr_unknown(root);
        assert_eq!(checker.env.with_choices().len(), 1);
        assert!(
            checker
                .env
                .with_choices()
                .next()
                .expect("with choice")
                .outcome()
                .is_ok()
        );
        checker.rollback_state(snapshot);
        assert!(checker.env.with_choices().len() == 0);
    }

    #[test]
    fn rejected_with_binding_retains_its_actual_error() {
        let source = "trait Read { fn read() -> u256 }\nstruct S {}\nimpl Read for S as Alternative { fn read() -> u256 { 2 } }\nfn use_it() -> u256 { with (Missing) { 0 } }\n";
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("rejected_with_capture.fe".into(), source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|id| id.data(&db) == "use_it")
            })
            .expect("fixture function");
        let (diagnostics, typed) = super::super::check_func_body(&db, function);
        assert!(!diagnostics.is_empty());
        assert_eq!(typed.with_choices().len(), 1);
        let choice = typed.with_choices().next().expect("with choice");
        assert_eq!(choice.error_code(), Some(99));
        assert!(matches!(
            choice.outcome(),
            Err(crate::analysis::ty::diagnostics::BodyDiag::UnknownNamedSelection { .. })
        ));
    }

    #[test]
    fn discarded_probe_does_not_retain_an_impl_choice() {
        let source = "trait Read { fn read() -> u256 }\nstruct S {}\nimpl Read for S { fn read() -> u256 { 1 } }\nfn use_it() -> u256 { 1 }\n";
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("discarded_choice.fe".into(), source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|id| id.data(&db) == "use_it")
            })
            .expect("fixture function");
        let implementation = top
            .all_items(&db)
            .iter()
            .find_map(|item| match item {
                super::ItemKind::ImplTrait(item) => Some(*item),
                _ => None,
            })
            .expect("fixture implementation");
        let goal = crate::analysis::ty::trait_lower::lower_impl_trait(&db, implementation)
            .expect("lowered impl")
            .trait_(&db);
        let mut checker =
            super::super::TyChecker::new(&db, super::BodyOwner::Func(function)).expect("checker");
        let snapshot = checker.snapshot_state();
        let obligation = super::super::env::TraitObligation {
            goal,
            assumptions: checker.env.assumptions(),
            context: checker.env.selection_context(),
            origin: super::super::env::TraitObligationOrigin::GenericConfirmation,
            span: function.span().into(),
        };
        let outcome = checker.process_trait_obligation(obligation, true);
        assert!(matches!(
            outcome,
            super::super::TraitObligationOutcome::Discharged
                | super::super::TraitObligationOutcome::Progressed
        ));
        assert_eq!(checker.env.trait_choices_len(), 1);
        checker.rollback_state(snapshot);
        assert!(checker.env.trait_choices_len() == 0);
    }

    #[test]
    fn impl_choice_keys_distinguish_sites_and_preserve_prior_answers() {
        let source = "trait Read { fn read() -> u256 }\nstruct S {}\nimpl Read for S { fn read() -> u256 { 1 } }\nfn use_it() -> u256 { 1 }\n";
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("discarded_choice.fe".into(), source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|id| id.data(&db) == "use_it")
            })
            .expect("fixture function");
        let implementation = top
            .all_items(&db)
            .iter()
            .find_map(|item| match item {
                super::ItemKind::ImplTrait(item) => Some(*item),
                _ => None,
            })
            .expect("fixture implementation");
        let goal = crate::analysis::ty::trait_lower::lower_impl_trait(&db, implementation)
            .expect("lowered impl")
            .trait_(&db);
        let mut checker =
            super::super::TyChecker::new(&db, super::BodyOwner::Func(function)).expect("checker");
        let snapshot = checker.snapshot_state();
        let obligation = super::super::env::TraitObligation {
            goal,
            assumptions: checker.env.assumptions(),
            context: checker.env.selection_context(),
            origin: super::super::env::TraitObligationOrigin::GenericConfirmation,
            span: function.span().into(),
        };
        let outcome = checker.process_trait_obligation(obligation.clone(), true);
        assert!(matches!(
            outcome,
            super::super::TraitObligationOutcome::Discharged
                | super::super::TraitObligationOutcome::Progressed
        ));
        assert_eq!(checker.env.trait_choices_len(), 1);
        let after_first = checker.snapshot_state();
        checker.process_trait_obligation(obligation.clone(), true);
        assert_eq!(checker.env.trait_choices_len(), 1, "one answer per key");
        let mut other_site = obligation;
        other_site.span = checker.body().expr(&db).span(checker.body()).into();
        checker.process_trait_obligation(other_site, true);
        assert_eq!(checker.env.trait_choices_len(), 2, "distinct use sites");
        checker.rollback_state(after_first);
        assert_eq!(checker.env.trait_choices_len(), 1, "prior answer survives");
        checker.rollback_state(snapshot);
        assert!(checker.env.trait_choices_len() == 0);
    }

    #[test]
    fn folded_condition_decisions_preserve_each_authored_application() {
        use crate::analysis::HirAnalysisDb;
        use crate::analysis::ty::{
            TyId,
            fold::{TyFoldable, TyFolder},
            ty_def::TyData,
        };
        struct Replace<'db> {
            from: TyId<'db>,
            to: TyId<'db>,
        }
        impl<'db> TyFolder<'db> for Replace<'db> {
            fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
                if ty == self.from {
                    self.to
                } else {
                    ty.super_fold_with(db, self)
                }
            }
        }
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/impl_const_requirements/pass/choice_slots_collapse.fe"
        ))
        .replace(
            "impl<T> Read<T> for S {",
            "impl<T> Read<T> for S where 1 > 0 {",
        )
        .replace("with (Alternative) { consume<S, T>() }", "consume<S, T>()");
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("condition_applications.fe".into(), &source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|name| name.data(&db) == "run")
            })
            .expect("run function");
        let (diagnostics, typed) = super::super::check_func_body(&db, function);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let entries: Vec<_> = typed.decisions().condition_entries().collect();
        assert_eq!(entries.len(), 2, "two impl applications at one call");
        assert_eq!(entries[0].1.span(), entries[1].1.span());
        assert_eq!(
            entries[0].1.source_predicate(),
            entries[1].1.source_predicate()
        );
        assert_ne!(
            entries[0].0, entries[1].0,
            "different authored instantiations"
        );
        assert_ne!(entries[0].1.arguments(), entries[1].1.arguments());
        let from = entries
            .iter()
            .flat_map(|(_, use_)| use_.arguments())
            .copied()
            .find(|ty| matches!(ty.data(&db), TyData::TyParam(_)))
            .expect("generic argument");
        let folded = typed.clone().fold_with(
            &db,
            &mut Replace {
                from,
                to: TyId::u8(&db),
            },
        );
        let folded_uses: Vec<_> = folded.checked_conditions().collect();
        assert_eq!(
            folded_uses.len(),
            2,
            "folding never merges distinct applications"
        );
        assert_eq!(folded_uses[0].arguments(), folded_uses[1].arguments());
        for (site, original) in entries {
            let actual = folded
                .decisions()
                .condition(site)
                .expect("same authored key after folding");
            assert_eq!(
                actual.outcome(),
                original.outcome(),
                "folding does not discharge again"
            );
        }
        assert_eq!(
            typed.decisions().condition_uses.keys().collect::<Vec<_>>(),
            folded.decisions().condition_uses.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    fn folded_choices_preserve_distinct_declared_constraint_slots() {
        use crate::analysis::HirAnalysisDb;
        use crate::analysis::ty::{
            TyId,
            fold::{TyFoldable, TyFolder},
            ty_def::TyData,
        };
        struct Replace<'db> {
            from: TyId<'db>,
            to: TyId<'db>,
        }
        impl<'db> TyFolder<'db> for Replace<'db> {
            fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
                if ty == self.from {
                    self.to
                } else {
                    ty.super_fold_with(db, self)
                }
            }
        }
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/impl_const_requirements/pass/choice_slots_collapse.fe"
        ));
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("choice_slots_collapse.fe".into(), source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|id| id.data(&db) == "run")
            })
            .expect("run function");
        let (diagnostics, typed) = super::super::check_func_body(&db, function);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let choices: Vec<_> = typed
            .trait_choices(&db)
            .filter(|choice| choice.call_constraint().is_some())
            .collect();
        assert_eq!(choices.len(), 2);
        assert_eq!(
            choices[0].span(),
            choices[1].span(),
            "one rendered call site"
        );
        assert_ne!(choices[0].call_constraint(), choices[1].call_constraint());
        let from = choices
            .iter()
            .flat_map(|choice| choice.goal().args(&db))
            .copied()
            .find(|ty| matches!(ty.data(&db), TyData::TyParam(_)))
            .expect("generic argument");
        let sites: Vec<_> = choices.iter().map(|choice| choice.site()).collect();
        assert_ne!(sites[0], sites[1], "distinct semantic use sites");
        let folded = typed.clone().fold_with(
            &db,
            &mut Replace {
                from,
                to: TyId::u8(&db),
            },
        );
        let choices: Vec<_> = folded
            .trait_choices(&db)
            .filter(|choice| choice.call_constraint().is_some())
            .collect();
        assert_eq!(choices.len(), 2, "both declared obligations survive");
        assert_eq!(
            choices[0].goal(),
            choices[1].goal(),
            "substitution merges the goals"
        );
        assert_ne!(
            choices[0].via(),
            choices[1].via(),
            "preserve each actual supplied answer"
        );
        for site in sites {
            let actual = folded
                .decisions()
                .choice(&db, &site)
                .expect("keyed slot answer");
            assert_eq!(actual.site(), site);
        }
    }

    #[test]
    fn folded_body_substitutes_retained_impl_choices() {
        use crate::analysis::HirAnalysisDb;
        use crate::analysis::ty::TyId;
        use crate::analysis::ty::fold::{TyFoldable, TyFolder};
        struct Replace<'db> {
            from: TyId<'db>,
            to: TyId<'db>,
        }
        impl<'db> TyFolder<'db> for Replace<'db> {
            fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
                if ty == self.from {
                    self.to
                } else {
                    ty.super_fold_with(db, self)
                }
            }
        }
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("folded_choice.fe".into(), "trait Read { fn read() -> u256 }\nfn bound<T: Read>() -> u256 { <T as Read>::read() }\n");
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|id| id.data(&db) == "bound")
            })
            .expect("fixture function");
        let (diagnostics, typed) = super::super::check_func_body(&db, function);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let original = typed
            .trait_choices(&db)
            .find(|choice| {
                choice
                    .goal()
                    .def(&db)
                    .name(&db)
                    .to_opt()
                    .is_some_and(|id| id.data(&db) == "Read")
            })
            .expect("Read decision");
        assert_eq!(
            typed.decisions().choice(&db, &original.site()),
            Some(original)
        );
        let from = original.goal().self_ty(&db);
        let to = TyId::u8(&db);
        assert_ne!(from, to);
        let span = original.span().clone();
        let site = original.site();
        let folded = typed.clone().fold_with(&db, &mut Replace { from, to });
        let folded_choice = folded
            .trait_choices(&db)
            .find(|choice| choice.span() == &span)
            .expect("folded decision");
        assert_eq!(
            folded_choice.goal().self_ty(&db),
            to,
            "retained decisions must follow substitution"
        );
        assert_eq!(folded.decisions().choice(&db, &site), Some(folded_choice));
        assert_eq!(
            typed.decisions().choices.keys().collect::<Vec<_>>(),
            folded.decisions().choices.keys().collect::<Vec<_>>(),
            "substitution preserves authored decision sites"
        );
        assert_eq!(
            typed
                .trait_choices(&db)
                .find(|choice| choice.span() == &span)
                .unwrap()
                .goal()
                .self_ty(&db),
            from,
            "shared inference record stays unchanged"
        );
    }

    #[test]
    fn symbolic_discharge_retains_the_actual_matching_premise() {
        let source = "trait Read { fn read() -> u256 }
\
                      struct S<const N: usize> {}
\
                      impl<const N: usize> Read for S<N> where N > 0 {
\
                          fn read() -> u256 { 7 }
\
                      }
\
                      fn use_it<const N: usize>() -> u256 where N > 0 {
\
                          <S<N> as Read>::read()
\
                      }";
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("matching_premise.fe".into(), source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|id| id.data(&db) == "use_it")
            })
            .expect("fixture function");
        let (diagnostics, typed) = super::super::check_func_body(&db, function);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert_eq!(typed.checked_conditions().len(), 1);
        let obligation = typed.checked_conditions().next().unwrap();
        let result = super::discharge_selected_requirement(
            &db,
            obligation.declaration,
            obligation.predicate,
            obligation.args.clone(),
            obligation.caller,
            obligation.context,
        );
        let premise = super::WhereClauseOwner::Func(function)
            .where_clause(&db)
            .const_predicates(&db)[0];
        assert_eq!(
            result.outcome,
            super::Discharge::Holds(super::RequirementProof::Premise(premise))
        );
    }

    #[test]
    fn cached_condition_discharge_cannot_run_during_proof_search() {
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/impl_const_requirements/recording_owners.fe"
        ));
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone("proof_boundary.fe".into(), source);
        let (top, _) = db.top_mod(file);
        let function = top
            .all_funcs(&db)
            .iter()
            .copied()
            .find(|function| {
                function
                    .name(&db)
                    .to_opt()
                    .is_some_and(|id| id.data(&db) == "one_call")
            })
            .expect("fixture function");
        let (diagnostics, typed) = super::super::check_func_body(&db, function);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let obligation = typed.checked_conditions().next().unwrap();
        let discharge = || {
            super::discharge_selected_requirement(
                &db,
                obligation.declaration,
                obligation.predicate,
                obligation.args.clone(),
                obligation.caller,
                obligation.context,
            )
        };
        assert!(matches!(
            discharge().outcome,
            super::Discharge::Holds(super::RequirementProof::Evaluated)
        ));
        let proof = crate::analysis::ty::trait_resolution::enter_proof_search();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(discharge));
        let body_view =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| typed.checked_conditions()));
        let written_view = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::written_condition_uses(&db, super::ItemKind::Func(function))
        }));
        drop(proof);
        assert!(
            body_view.is_err(),
            "cached checked-condition view crossed the proof boundary"
        );
        assert!(
            written_view.is_err(),
            "cached authored-condition view crossed the proof boundary"
        );
        assert!(
            result.is_err(),
            "cached condition discharge crossed the proof boundary"
        );
    }
}
