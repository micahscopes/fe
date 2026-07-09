//! Semantic diagnostics helpers.
//!
//! This module is the home for traversal API helpers that produce
//! `TyDiagCollection` / diagnostics. Over time, diagnostic-focused
//! logic from `core::semantic` is being migrated here to keep the main
//! traversal surface free of diagnostic concerns.

use rustc_hash::FxHashMap;
use smallvec1::SmallVec;

use crate::analysis::HirAnalysisDb;
use crate::analysis::name_resolution;
use crate::analysis::ty;
use crate::analysis::ty::diagnostics::{
    FuncBodyDiag, TraitConstraintDiag, TyDiagCollection, TyLowerDiag,
};
use crate::analysis::ty::normalize::normalize_ty;
use crate::analysis::ty::trait_lower::lower_impl_trait;
use crate::analysis::ty::ty_check::check_anon_const_body;
use crate::analysis::ty::ty_def::{InvalidCause, TyId};
use crate::analysis::ty::ty_error::collect_ty_lower_errors;
use crate::hir_def::scope_graph::ScopeId;
use crate::hir_def::{
    Contract, Enum, EnumVariant, FieldParent, Func, GenericParam, GenericParamOwner,
    GenericParamView, IdentId, Impl, ImplTrait, ItemKind, Partial, PathId, Struct, Trait,
    TypeAlias, TypeBound, VariantKind, WhereClauseOwner,
};
use crate::span::DynLazySpan;

use crate::analysis::ty::adt_def::AdtRef;
use crate::analysis::ty::binder::Binder;
use crate::analysis::ty::trait_def::ImplementorId;
use crate::semantic::{
    FieldView, FuncParamView, ImplAssocTypeView, InherentImplAdmissibility, SuperTraitRefView,
    VariantView, WherePredicateBoundView, WherePredicateView, constraints_for,
    header_constraints_for, lower_hir_kind_local, param_env,
};

/// Unified "pull" diagnostics surface for HIR items and views.
pub trait Diagnosable<'db> {
    type Diagnostic;
    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic>;
}

/// Shared helper for duplicate name diagnostics.
pub(crate) fn check_duplicate_names<'db, F>(
    names: impl Iterator<Item = Option<IdentId<'db>>>,
    create_diag: F,
) -> SmallVec<[TyDiagCollection<'db>; 2]>
where
    F: Fn(SmallVec<[u16; 4]>) -> TyDiagCollection<'db>,
{
    let mut defs = FxHashMap::<IdentId<'db>, SmallVec<[u16; 4]>>::default();
    for (i, name) in names.enumerate() {
        if let Some(name) = name {
            defs.entry(name).or_default().push(i as u16);
        }
    }
    defs.into_values()
        .filter_map(|idxs| (idxs.len() > 1).then_some(create_diag(idxs)))
        .collect()
}

fn const_ty_mismatch_diag<'db>(
    span: DynLazySpan<'db>,
    expected: TyId<'db>,
    given: TyId<'db>,
) -> TyDiagCollection<'db> {
    TyLowerDiag::ConstTyMismatch {
        span,
        expected,
        given,
    }
    .into()
}

/// Trait-satisfiability diagnostics raised while checking an associated-const
/// initializer (e.g. an unsatisfied `Ty: Trait` bound behind a
/// `<Ty as Trait>::CONST` read). These have no other reporting site for impl
/// assoc-const bodies, so they are surfaced from `diags_assoc_consts`.
fn extract_satisfiability<'db>(diag: &FuncBodyDiag<'db>) -> Option<TyDiagCollection<'db>> {
    match diag {
        FuncBodyDiag::Ty(collection @ TyDiagCollection::Satisfiability(_)) => {
            Some(collection.clone())
        }
        _ => None,
    }
}

fn cyclic_trait_ref_diag<'db>(span: DynLazySpan<'db>, context: &str) -> TyDiagCollection<'db> {
    TraitConstraintDiag::InfiniteBoundRecursion(
        span,
        format!("cyclic trait reference prevented lowering this {context}"),
    )
    .into()
}

impl<'db> SuperTraitRefView<'db> {
    /// Diagnostics for this super-trait reference in its owner's context.
    /// Uses the trait's `Self` as subject and checks WF; kind mismatch is emitted
    /// elsewhere via `Trait::diags_super_traits`.
    pub fn diags(self, db: &'db dyn HirAnalysisDb) -> Option<TyDiagCollection<'db>> {
        use name_resolution::{ExpectedPathKind, diagnostics::PathResDiag};
        use ty::trait_lower::{self, TraitRefLowerError};
        use ty::trait_resolution::check_trait_inst_wf;

        let span = self.span();
        let subject = self.subject_self(db);
        let scope = self.owner.scope();
        let assumptions = self.assumptions(db);
        let tr = self.trait_ref(db);

        let inst = match trait_lower::lower_trait_ref(db, subject, tr, scope, assumptions, None) {
            Ok(i) => i,
            Err(TraitRefLowerError::PathResError(err)) => {
                let path = tr.path(db).unwrap();
                let diag = err.into_diag(db, path, span.path(), ExpectedPathKind::Trait)?;
                return Some(diag.into());
            }
            Err(TraitRefLowerError::InvalidDomain(res)) => {
                let path = tr.path(db).unwrap();
                let ident = path.ident(db).unwrap();
                return Some(
                    PathResDiag::ExpectedTrait(span.path().into(), ident, res.kind_name()).into(),
                );
            }
            Err(TraitRefLowerError::Cycle) => {
                return Some(cyclic_trait_ref_diag(
                    span.path().into(),
                    "super-trait bound",
                ));
            }
            Err(TraitRefLowerError::UnsafeLocalBoundBlanketImpl | TraitRefLowerError::Ignored) => {
                return None;
            }
        };

        // Do not emit when subject contains assoc types of params
        if inst.self_ty(db).contains_assoc_ty_of_param(db) {
            return None;
        }

        check_trait_inst_wf(
            db,
            ty::trait_resolution::ProvisionEnv::for_scope(scope, assumptions).solve_cx(db),
            inst,
        )
        .into_diag(span.into())
    }
}

impl<'db> WherePredicateView<'db> {
    /// Aggregate diagnostics for this where-predicate:
    /// - Subject-level errors (const/concrete or path-domain remapped)
    /// - Per-bound trait diagnostics
    /// - Per-bound kind consistency
    pub fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        // A boundless predicate is the constraint-application form (`where
        // Eq<T>`): the whole written type is the predicate, not a subject with
        // trait bounds, so it has its own diagnostics path.
        if self.is_boundless(db) {
            return self.constraint_application_diags(db);
        }

        let Some(subject) = self.subject_ty(db) else {
            return Vec::new();
        };

        if let Some(diag) = self.diag_subject_ty(db, subject) {
            return vec![diag];
        }

        self.bound_diags(db, subject)
    }

    /// Diagnostics for a boundless constraint-application predicate (`where
    /// Eq<T>`). A concrete trait head is collected as an obligation and enforced
    /// at use sites, so it needs no declaration-side diagnostic. A head that is
    /// not a trait (most importantly an abstract `* -> Constraint` parameter
    /// `where P<T>`, which is intentionally not yet supported, wiring-party
    /// D2/D7c) is rejected BY NAME here rather than silently dropped.
    fn constraint_application_diags(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<TyDiagCollection<'db>> {
        use crate::analysis::name_resolution::diagnostics::PathResDiag;
        use crate::analysis::name_resolution::{ExpectedPathKind, PathRes, resolve_path};
        use crate::analysis::ty::diagnostics::TraitConstraintDiag;
        use crate::analysis::ty::ty_def::Kind;
        use crate::core::hir_def::types::TypeKind as HirTyKind;

        let Some(hir_ty) = self.subject_hir_ty(db) else {
            return Vec::new();
        };
        let HirTyKind::Path(path) = hir_ty.data(db) else {
            return Vec::new();
        };
        let Some(path) = path.to_opt() else {
            return Vec::new();
        };

        let owner_item = ItemKind::from(self.clause.owner);
        let scope = owner_item.scope();
        let assumptions = header_constraints_for(db, owner_item);
        let head = path.strip_generic_args(db);
        let ty_path_span = self.span().ty().into_path_type().path();

        // Is `kind` the kind `* -> Constraint` of a constraint constructor?
        let is_constraint_ctor = |kind: &Kind| {
            matches!(
                kind,
                Kind::Abs(inner)
                    if inner.0.does_match(&Kind::Star) && inner.1.does_match(&Kind::Constraint)
            )
        };

        match resolve_path(db, head, scope, assumptions, false) {
            // Concrete trait application: collected + enforced at use sites.
            Ok(PathRes::Trait(_)) => Vec::new(),
            // The abstract head: a `* -> Constraint` parameter applied (`where
            // P<T>`). Genuine variable-headed solving is build-trigger-gated
            // research (FCO_ABSTRACT_HEAD_RESEARCH_DOSSIER.md). Name the
            // monomorphize-per-trait workaround instead of a bare "expected
            // trait" or a silent drop.
            Ok(PathRes::Ty(ty)) if is_constraint_ctor(ty.kind(db)) => {
                match path.ident(db).to_opt() {
                    Some(param) => vec![
                        TraitConstraintDiag::ConstraintCtorParamUnsupported {
                            span: ty_path_span.into(),
                            param,
                        }
                        .into(),
                    ],
                    None => Vec::new(),
                }
            }
            // Any other non-trait head.
            Ok(res) => match path.ident(db).to_opt() {
                Some(ident) => {
                    vec![
                        PathResDiag::ExpectedTrait(ty_path_span.into(), ident, res.kind_name())
                            .into(),
                    ]
                }
                None => Vec::new(),
            },
            Err(err) => err
                .into_diag(db, head, ty_path_span, ExpectedPathKind::Trait)
                .map(|d| vec![d.into()])
                .unwrap_or_default(),
        }
    }

    /// Diagnostic for this predicate's subject type, if any:
    /// - Path-resolution domain errors are remapped to precise diagnostics.
    /// - Const subjects are rejected.
    /// - Fully concrete, non-generic subjects are rejected.
    fn diag_subject_ty(
        self,
        db: &'db dyn HirAnalysisDb,
        subject: TyId<'db>,
    ) -> Option<TyDiagCollection<'db>> {
        use crate::analysis::name_resolution::diagnostics::PathResDiag;
        use crate::analysis::name_resolution::{ExpectedPathKind, resolve_path};

        // Path-resolution failures are carried via the subject's InvalidCause.
        let owner_item = ItemKind::from(self.clause.owner);
        let assumptions = header_constraints_for(db, owner_item);
        if let Some(InvalidCause::PathResolutionFailed { path }) = subject.invalid_cause(db) {
            // Re-run name resolution on the failed path and surface a precise diagnostic
            // at the type path span within the where-predicate.
            let ty_span = self.span().ty().into_path_type().path();
            match resolve_path(db, path, owner_item.scope(), assumptions, false) {
                Ok(res) => {
                    // Resolved to a non-type domain
                    if let Some(ident) = path.ident(db).to_opt() {
                        let diag =
                            PathResDiag::ExpectedType(ty_span.into(), ident, res.kind_name());
                        return Some(diag.into());
                    }
                }
                Err(inner) => {
                    if let Some(diag) = inner.into_diag(db, path, ty_span, ExpectedPathKind::Type) {
                        return Some(diag.into());
                    }
                }
            }
        }
        let span = self.span().ty().into();

        if subject.is_const_ty(db) {
            return Some(TraitConstraintDiag::ConstTyBound(span, subject).into());
        }

        if !subject.has_invalid(db) && !subject.has_param(db) {
            return Some(TraitConstraintDiag::ConcreteTypeBound(span, subject).into());
        }

        None
    }
}

impl<'db> WherePredicateBoundView<'db> {
    /// Diagnostics for this trait bound, given an explicit subject type.
    /// Mirrors legacy visitor behavior for path errors, kind mismatch, and satisfiability.
    pub(crate) fn diags_for_subject(
        self,
        db: &'db dyn HirAnalysisDb,
        subject: ty::ty_def::TyId<'db>,
    ) -> Vec<TyDiagCollection<'db>> {
        use name_resolution::{ExpectedPathKind, diagnostics::PathResDiag};
        use ty::trait_lower::{self, TraitRefLowerError};
        use ty::trait_resolution::check_trait_inst_wf;

        let mut out = Vec::new();
        let owner_item = ItemKind::from(self.pred.clause.owner);
        let scope = owner_item.scope();
        let assumptions = header_constraints_for(db, owner_item);
        let is_trait_self_subject =
            matches!(owner_item, ItemKind::Trait(_)) && self.pred.is_self_subject(db);
        let tr = self.trait_ref(db);
        let span = self.trait_ref_span();

        match trait_lower::lower_trait_ref(
            db,
            subject,
            tr,
            scope,
            assumptions,
            ty::trait_resolution::constraint::enclosing_trait_self_ty(db, scope),
        ) {
            Ok(inst) => {
                let expected = inst.def(db).self_param(db).kind(db);
                if !expected.does_match(subject.kind(db)) {
                    out.push(
                        TraitConstraintDiag::TraitArgKindMismatch {
                            span: span.clone(),
                            expected: expected.clone(),
                            actual: subject,
                        }
                        .into(),
                    );
                }

                if inst.self_ty(db).contains_assoc_ty_of_param(db) {
                    return out;
                }

                // For trait-level `Self: Bound` constraints, treat as preconditions;
                // do not emit unsatisfied bound diagnostics here.
                if !is_trait_self_subject
                    && let Some(diag) = check_trait_inst_wf(
                        db,
                        ty::trait_resolution::ProvisionEnv::for_scope(scope, assumptions)
                            .solve_cx(db),
                        inst,
                    )
                    .into_diag(span.into())
                {
                    out.push(diag);
                }
            }
            Err(TraitRefLowerError::PathResError(err)) => {
                if let Some(path) = tr.path(db).to_opt()
                    && let Some(diag) =
                        err.into_diag(db, path, span.path(), ExpectedPathKind::Trait)
                {
                    out.push(diag.into());
                }
            }
            Err(TraitRefLowerError::InvalidDomain(res)) => {
                if let Some(path) = tr.path(db).to_opt()
                    && let Some(ident) = path.ident(db).to_opt()
                {
                    out.push(
                        PathResDiag::ExpectedTrait(span.path().into(), ident, res.kind_name())
                            .into(),
                    );
                }
            }
            Err(TraitRefLowerError::Cycle) => {
                out.push(cyclic_trait_ref_diag(span.path().into(), "trait bound"));
            }
            Err(TraitRefLowerError::UnsafeLocalBoundBlanketImpl | TraitRefLowerError::Ignored) => {}
        }

        out
    }

    /// Diagnostics for this trait bound, deriving the subject from the predicate's LHS.
    /// Returns a single-element vec with the subject error if subject lowering fails.
    pub fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let subject = match self.pred.subject_ty(db) {
            Some(s) => s,
            None => return Vec::new(),
        };
        self.diags_for_subject(db, subject)
    }
}

impl<'db> Func<'db> {
    pub fn diags_const_fn(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        // Const-safety diagnostics are handled by the const-check pass on the body.
        let _ = db;
        Vec::new()
    }

    /// Diagnostics related to parameters (duplicate names/labels).
    pub fn diags_parameters(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        check_duplicate_names(self.params(db).map(|v| v.name(db)), |idxs| {
            TyLowerDiag::DuplicateArgName(self, idxs).into()
        })
        .into_iter()
        .collect()
    }

    /// Diagnostics related to the explicit return type (kind/const checks).
    pub fn diags_return(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let mut diags = Vec::new();
        if self.has_explicit_return_ty(db) {
            // First, surface name-resolution/path-domain errors on the return type itself
            let errs = self.ret_ty_errors(db);
            if !errs.is_empty() {
                return errs;
            }

            // Then run kind/const checks on the lowered semantic type
            let ret = self.return_ty(db);
            let span = self.span().ret_ty().into();
            if !ret.has_star_kind(db) {
                diags.push(TyLowerDiag::ExpectedStarKind(span).into());
            } else if ret.is_const_ty(db) {
                diags.push(TyLowerDiag::NormalTypeExpected { span, given: ret }.into());
            } else if ty::ty_contains_const_hole(db, ret) {
                diags.push(TyLowerDiag::ConstHoleInValuePosition { span, ty: ret }.into());
            } else if let ty::trait_resolution::WellFormedness::IllFormed { goal, subgoal } =
                ty::trait_resolution::check_ty_wf(
                    db,
                    ty::trait_resolution::TraitSolveCx::new(db, self.scope())
                        .with_assumptions(param_env(db, self.into())),
                    ret,
                )
            {
                diags.push(
                    TraitConstraintDiag::TraitBoundNotSat {
                        span,
                        primary_goal: goal,
                        unsat_subgoal: subgoal,
                        required_by: None,
                    }
                    .into(),
                );
            }
        }
        diags
    }

    /// Diagnostics for function parameter types:
    /// - For all params: star kind required and reject const types
    /// - For self param: enforce exact `Self` type shape
    ///   Note: WF/invalid errors are still surfaced via the general type walker.
    pub fn diags_param_types(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        self.params(db).flat_map(|v| v.diags(db)).collect()
    }
}

impl<'db> Diagnosable<'db> for FuncParamView<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        self.ty_diags(db)
    }
}

impl<'db> Diagnosable<'db> for TypeAlias<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        let mut out = self.ty_errors(db);
        out.extend(self.ty_wf_errors(db));
        out.extend(GenericParamOwner::TypeAlias(self).diags(db));
        out
    }
}

impl<'db> Trait<'db> {
    /// Diagnostics for associated type defaults (bounds satisfaction), in the trait's context.
    pub fn diags_assoc_defaults(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use ty::trait_resolution::PredicateListId;

        let mut diags = Vec::new();
        let assumptions = param_env(db, self.into());
        for assoc in self.assoc_types(db) {
            let Some(default_ty) = assoc.default_ty(db) else {
                continue;
            };
            // A7.1 (trait-side analog of the leg-1 discharge): a GUARDED GAT
            // default may assume its own param guards (`type Elem<T: Marker>: ..
            // = DefaultStore<T>` assumes `T: Marker`) when checking its RHS
            // bound. The guard subjects are trait-side rigids, minted at the same
            // `TraitType` scope as `default_ty`'s own rigids, so NO remap is
            // needed -- merge them into the assumptions for this default's bound
            // check only (discharge context; never `param_env`). Empty for every
            // unguarded default (byte-identical baseline).
            let guards = assoc.gat_param_guard_insts(db);
            let default_assumptions = if guards.is_empty() {
                assumptions
            } else {
                let mut merged = assumptions.list(db).clone();
                merged.extend(guards.into_iter().map(|(_, g)| g));
                PredicateListId::new(db, merged)
            };
            for trait_inst in assoc.bounds_on_subject(db, default_ty) {
                match ty::trait_resolution::is_goal_satisfiable(
                    db,
                    ty::trait_resolution::ProvisionEnv::for_scope(self.scope(), default_assumptions)
                        .solve_cx(db),
                    trait_inst,
                ) {
                    ty::trait_resolution::GoalSatisfiability::Satisfied(_) => {}
                    ty::trait_resolution::GoalSatisfiability::UnSat(_) => {
                        diags.push(
                            TraitConstraintDiag::TraitBoundNotSat {
                                span: self.span().into(),
                                primary_goal: trait_inst,
                                unsat_subgoal: None,
                                required_by: None,
                            }
                            .into(),
                        );
                    }
                    _ => {}
                }
            }
        }
        diags
    }

    /// A4.4 (mb2-a4.1): const generic params on associated types are DEFERRED.
    /// Emit a clean decl-site diagnostic for each const param on this trait's
    /// associated-type declarations (`gat_param_ty`'s resolver-`invalid` stays
    /// the backstop). Anti-cascade: a clear single message per param rather
    /// than a spray of downstream `invalid`s.
    pub fn diags_gat_const_params(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let mut diags = Vec::new();
        for assoc in self.assoc_types(db) {
            for (j, param) in assoc.generic_params(db).data(db).iter().enumerate() {
                if matches!(param, GenericParam::Const(_)) {
                    diags.push(
                        TyLowerDiag::GatConstParamUnsupported {
                            span: assoc.span().generic_params().param(j).into(),
                        }
                        .into(),
                    );
                }
            }
        }
        diags
    }

    /// A7.1 leg 0 (decl-site guard WF): each GAT param TRAIT bound (`type
    /// Elem<T: G>` -> `T: G`) is the guard the A7 duality assumes callee-side
    /// (leg 1) and proves caller-side (leg 2). A guard whose trait path does NOT
    /// resolve is dropped by the SSOT accessor `gat_param_guard_insts` on both
    /// legs symmetrically (sound, but silent); this leg re-runs the SAME
    /// lowering and OWNS the visible error, so the "silently dropped on both
    /// sides" state cannot survive in accepted code. For a guard that DOES
    /// lower, its args are checked for well-formedness. Mirror of
    /// `WherePredicateBoundView::diags_for_subject`'s path/domain/cycle arms.
    pub fn diags_gat_param_guards(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use name_resolution::{ExpectedPathKind, diagnostics::PathResDiag};
        use ty::trait_lower::{self, TraitRefLowerError};
        use ty::trait_resolution::{ProvisionEnv, check_ty_wf};
        use ty::ty_lower::gat_param_ty;

        let mut diags = Vec::new();
        let assumptions = crate::semantic::constraints_for(db, self.into());
        let owner_self = self.self_param(db);
        for (assoc_idx, assoc) in self.assoc_types(db).enumerate() {
            let decl_scope = ScopeId::TraitType(self, assoc_idx as u16);
            for (j, param) in assoc.generic_params(db).data(db).iter().enumerate() {
                let GenericParam::Type(p) = param else {
                    continue;
                };
                let subject = gat_param_ty(db, param, j, decl_scope);
                for (k, bound) in p.bounds.iter().enumerate() {
                    let crate::hir_def::TypeBound::Trait(tr) = bound else {
                        continue;
                    };
                    let bound_span = assoc
                        .span()
                        .generic_params()
                        .param(j)
                        .into_type_param()
                        .bounds()
                        .bound(k)
                        .trait_bound();
                    match trait_lower::lower_trait_ref(
                        db,
                        subject,
                        *tr,
                        decl_scope,
                        assumptions,
                        Some(owner_self),
                    ) {
                        Ok(inst) => {
                            // The guard lowered; check its args are well-formed.
                            let solve_cx =
                                ProvisionEnv::for_scope(self.scope(), assumptions).solve_cx(db);
                            for &arg in inst.args(db) {
                                if let Some(diag) = check_ty_wf(db, solve_cx, arg)
                                    .into_diag(bound_span.clone().path().into())
                                {
                                    diags.push(diag);
                                }
                            }
                        }
                        Err(TraitRefLowerError::PathResError(err)) => {
                            if let Some(path) = tr.path(db).to_opt()
                                && let Some(diag) = err.into_diag(
                                    db,
                                    path,
                                    bound_span.path(),
                                    ExpectedPathKind::Trait,
                                )
                            {
                                diags.push(diag.into());
                            }
                        }
                        Err(TraitRefLowerError::InvalidDomain(res)) => {
                            if let Some(path) = tr.path(db).to_opt()
                                && let Some(ident) = path.ident(db).to_opt()
                            {
                                diags.push(
                                    PathResDiag::ExpectedTrait(
                                        bound_span.path().into(),
                                        ident,
                                        res.kind_name(),
                                    )
                                    .into(),
                                );
                            }
                        }
                        Err(TraitRefLowerError::Cycle) => {
                            diags.push(cyclic_trait_ref_diag(
                                bound_span.path().into(),
                                "trait bound",
                            ));
                        }
                        Err(
                            TraitRefLowerError::UnsafeLocalBoundBlanketImpl
                            | TraitRefLowerError::Ignored,
                        ) => {}
                    }
                }
            }
        }
        diags
    }

    /// Diagnostics for generic parameter issues (duplicates, defined in parent).
    pub fn diags_generic_params(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let owner = GenericParamOwner::Trait(self);
        let mut out: Vec<TyDiagCollection> = owner.diags_check_duplicate_names(db).collect();
        out.extend(owner.diags_params_defined_in_parent(db));
        out
    }

    /// Diagnostics for super-traits (semantic, kind-mismatch only).
    pub fn diags_super_traits(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use ty::trait_resolution::check_trait_inst_wf;

        let mut diags = Vec::new();
        for view in self.super_trait_refs(db) {
            if let Some((expected, actual)) = view.kind_mismatch_for_self(db) {
                diags.push(
                    TraitConstraintDiag::TraitArgKindMismatch {
                        span: view.span(),
                        expected,
                        actual,
                    }
                    .into(),
                );
            }

            // Additionally, ensure that the super-trait reference is well-formed
            if let Ok(inst) = view.trait_inst(db)
                && let Some(diag) = check_trait_inst_wf(
                    db,
                    ty::trait_resolution::ProvisionEnv::for_scope(
                        self.scope(),
                        view.assumptions(db),
                    )
                    .solve_cx(db),
                    inst,
                )
                .into_diag(view.span().into())
            {
                diags.push(diag);
            }
        }
        diags
    }
}

impl<'db> Impl<'db> {
    /// Impl-specific preconditions and implementor-type diagnostics.
    /// Generic parameter diagnostics are handled by `Diagnosable::diags`.
    pub fn diags_preconditions(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use ty::diagnostics::ImplDiag;

        let mut out = self.ty_errors(db);
        match self.inherent_impl_admissibility(db) {
            InherentImplAdmissibility::Admissible { .. } => {}
            InherentImplAdmissibility::NotAllowed { ty, is_nominal } => {
                let base = ty.base_ty(db);
                out.push(
                    ImplDiag::InherentImplIsNotAllowed {
                        primary: self.span().target_ty().into(),
                        ty: base.pretty_print(db).to_string(),
                        is_nominal,
                    }
                    .into(),
                );
                return out;
            }
            InherentImplAdmissibility::InvalidTy { ty } => {
                if let Some(diag) =
                    ty::ty_error::emit_invalid_ty_error(db, ty, self.span().target_ty().into())
                {
                    out.push(diag);
                }
                return out;
            }
            InherentImplAdmissibility::IllFormed { goal, subgoal, .. } => {
                out.push(
                    TraitConstraintDiag::TraitBoundNotSat {
                        span: self.span().target_ty().into(),
                        primary_goal: goal,
                        unsat_subgoal: subgoal,
                        required_by: None,
                    }
                    .into(),
                );
            }
            InherentImplAdmissibility::IllFormedConstPredicate { predicate, .. } => {
                out.push(
                    TraitConstraintDiag::ConstPredicateNotSat {
                        span: self.span().target_ty().into(),
                        predicate,
                    }
                    .into(),
                );
            }
        }

        out
    }

    /// Declaration diagnostics for associated consts in inherent impl blocks:
    /// every const must have a value, and body checking runs in `BodyAnalysisPass`.
    pub fn diags_assoc_consts(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use ty::diagnostics::ImplDiag;

        let mut diags = Vec::new();
        let assumptions = constraints_for(db, self.into());
        let target_enum = self
            .admissible_inherent_impl_ty(db)
            .and_then(|ty| ty.as_enum(db));
        let mut seen: rustc_hash::FxHashMap<IdentId<'db>, DynLazySpan<'db>> = Default::default();
        for impl_const in self.assoc_consts(db) {
            let Some(name) = impl_const.name(db) else {
                continue;
            };

            let name_span: DynLazySpan = impl_const.span().name().into();
            // Duplicate within this same impl block.
            if let Some(first_span) = seen.get(&name) {
                diags.push(
                    ImplDiag::InherentConstConflict {
                        primary: name_span.clone(),
                        conflict_with: first_span.clone(),
                        const_name: name,
                    }
                    .into(),
                );
                continue;
            }
            seen.insert(name, name_span.clone());

            // Conflict with an overlapping *other* inherent impl block. Caught
            // here so an unreferenced duplicate is still diagnosed (path
            // resolution would otherwise only report it at a use site).
            if let Some(other) =
                crate::analysis::name_resolution::earliest_conflicting_inherent_const_impl(
                    db, self, name,
                )
                && let Some(conflict_with) = other
                    .assoc_consts(db)
                    .find(|c| c.name(db) == Some(name))
                    .map(|c| c.span().name().into())
            {
                diags.push(
                    ImplDiag::InherentConstConflict {
                        primary: name_span,
                        conflict_with,
                        const_name: name,
                    }
                    .into(),
                );
                continue;
            }

            // A const sharing a variant's name could never be referenced
            // (variants take precedence in path resolution), so reject it.
            if let Some(enum_) = target_enum
                && let Some(variant) = enum_.variants(db).find(|v| v.name(db) == Some(name))
            {
                let variant_span = EnumVariant::new(enum_, variant.idx).span().name().into();
                diags.push(
                    ImplDiag::InherentConstShadowsVariant {
                        primary: name_span,
                        variant_span,
                        const_name: name,
                    }
                    .into(),
                );
                continue;
            }

            // A const that shares a name with an inherent function shadows it:
            // `S::name` resolves to the const, so the function is unreachable.
            if let Some(fn_span) = name_resolution::shadowed_inherent_fn_for_const(db, self, name) {
                diags.push(
                    ImplDiag::InherentConstShadowsFn {
                        primary: name_span,
                        fn_span,
                        const_name: name,
                    }
                    .into(),
                );
                continue;
            }

            if impl_const.value_body(db).is_none() {
                diags.push(
                    ImplDiag::InherentConstMissingValue {
                        primary: impl_const.span().ty().into(),
                        const_name: name,
                    }
                    .into(),
                );
                continue;
            }

            // Report unresolvable/invalid type annotations; checking the body
            // against an invalid expected type would only produce noise.
            if let Some(hir_ty) = impl_const.hir_ty(db) {
                let ty_diags = ty::ty_error::collect_hir_ty_diags(
                    db,
                    self.scope(),
                    hir_ty,
                    impl_const.span().ty(),
                    assumptions,
                );
                if !ty_diags.is_empty() {
                    diags.extend(ty_diags);
                }
            }

            // The const value body is type-checked by `BodyAnalysisPass`, which
            // surfaces a value/declared-type mismatch as a plain `TypeMismatch`
            // (same as a top-level `const`); nothing is reported for it here.
        }
        diags
    }
}

impl<'db> ImplTrait<'db> {
    /// Lower the implementor view and report validity diagnostics (WF, conflicts, kind mismatch).
    /// Returns the implementor view if successful, or None if critical errors occurred.
    pub(crate) fn diags_implementor_validity(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> (
        Option<Binder<ImplementorId<'db>>>,
        Vec<TyDiagCollection<'db>>,
    ) {
        self.implementor_with_errors(db)
    }

    /// Diagnostics for missing associated types (required by the trait).
    pub fn diags_missing_assoc_types(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<TyDiagCollection<'db>> {
        use ty::diagnostics::ImplDiag;
        use ty::trait_lower::lower_impl_trait;

        let mut diags = Vec::new();
        let Some(implementor) = lower_impl_trait(db, self) else {
            return diags;
        };
        let implementor = implementor.instantiate_identity();
        let trait_hir = implementor.trait_def(db);
        let impl_types = implementor.types(db);

        for assoc in trait_hir.assoc_types(db) {
            let Some(name) = assoc.name(db) else { continue };
            let has_impl = impl_types.get(&name).is_some();
            let has_default = assoc.default_ty(db).is_some();
            if !has_impl && !has_default {
                diags.push(
                    ImplDiag::MissingAssociatedType {
                        primary: self.span().ty().into(),
                        type_name: name,
                        trait_: trait_hir,
                    }
                    .into(),
                );
            }
        }
        diags
    }

    /// Diagnostics for GAT signature conformance of impl associated types
    /// against the trait declaration (mb2-a4.1). Pure binder-shape comparison
    /// (SOLVER-FREE) via [`gat_signature_conforms`]:
    /// - 6-0023 param count mismatch,
    /// - 6-0024 param sort mismatch (type vs const),
    /// - 6-0025 param kind mismatch,
    /// - 6-0026 impl declares an assoc type the trait does not have (was
    ///   silent).
    ///
    /// The DECL is the authority (the same `assoc_decl_arity` the A3 G1 gate
    /// reads). This closes the escaped-rigid / conflation hazard where an impl
    /// `type Ptr<T>` with a divergent binder is projected without a re-check.
    pub fn diags_assoc_type_conformance(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<TyDiagCollection<'db>> {
        use ty::diagnostics::ImplDiag;
        use ty::ty_lower::{GatSigMismatch, gat_signature_conforms};

        let mut diags = Vec::new();
        let Some(implementor) = lower_impl_trait(db, self) else {
            return diags;
        };
        let implementor = implementor.instantiate_identity();
        let trait_hir = implementor.trait_def(db);

        for (impl_idx, def) in self.types(db).iter().enumerate() {
            let Some(name) = def.name.to_opt() else { continue };

            let Some(decl) = trait_hir.assoc_ty(db, name) else {
                // 6-0026: impl declares an assoc type the trait does not have.
                diags.push(
                    ImplDiag::AssocTypeNotDefinedInTrait {
                        primary: self.span().associated_type(impl_idx).name().into(),
                        trait_: trait_hir,
                        type_name: name,
                    }
                    .into(),
                );
                continue;
            };

            // Trait-side declaration span (secondary "declared here" label).
            let trait_decl_span = trait_hir
                .assoc_types(db)
                .find(|v| v.name(db) == Some(name))
                .map(|v| v.span());

            match gat_signature_conforms(db, decl, def) {
                Ok(()) => {
                    // A4.4 (mb2-a4.1): the binder shape conforms; if it uses
                    // const GAT params (both sides agree on `const`), that
                    // feature is deferred. Emit a clean decl-site diagnostic at
                    // each impl-side const param (the trait-side is flagged by
                    // `Trait::diags_gat_const_params`). Anti-cascade: no raw
                    // resolver `invalid` spray.
                    for (j, param) in def.generic_params.data(db).iter().enumerate() {
                        if matches!(param, GenericParam::Const(_)) {
                            diags.push(
                                TyLowerDiag::GatConstParamUnsupported {
                                    span: self
                                        .span()
                                        .associated_type(impl_idx)
                                        .generic_params()
                                        .param(j)
                                        .into(),
                                }
                                .into(),
                            );
                        }

                        // A7.1 leg 0 (impl-side authority rule): GAT parameter
                        // TRAIT bounds are declared on the trait DECL, which is
                        // the single guard authority (`gat_param_guard_insts`).
                        // The impl binder may only REPEAT kind bounds (compared
                        // by `gat_signature_conforms`); a trait bound here would
                        // let the impl assume a guard the decl never demanded of
                        // callers, so it is rejected. Zero baseline inhabitants
                        // (no impl GAT binder carries a trait bound today), so
                        // this strictness lands on a new leg only.
                        if let GenericParam::Type(p) = param
                            && p.bounds
                                .iter()
                                .any(|b| matches!(b, crate::hir_def::TypeBound::Trait(_)))
                        {
                            diags.push(
                                TyLowerDiag::GatImplBinderTraitBound {
                                    span: self
                                        .span()
                                        .associated_type(impl_idx)
                                        .generic_params()
                                        .param(j)
                                        .into(),
                                }
                                .into(),
                            );
                        }
                    }
                }
                Err(GatSigMismatch::ParamCount { expected, given }) => {
                    // Anchor at the assoc type NAME (always resolvable): the
                    // impl may have zero generic params (`type Ptr = ...`), in
                    // which case the generic-param-list span is empty.
                    let trait_decl_span = trait_decl_span
                        .map(|s| s.name().into())
                        .unwrap_or_else(|| self.span().ty().into());
                    diags.push(
                        ImplDiag::AssocTypeParamNumMismatch {
                            primary: self.span().associated_type(impl_idx).name().into(),
                            trait_decl_span,
                            type_name: name,
                            expected,
                            given,
                        }
                        .into(),
                    );
                }
                Err(GatSigMismatch::ParamSort { param_idx }) => {
                    let trait_decl_span = trait_decl_span
                        .map(|s| s.generic_params().param(param_idx).into())
                        .unwrap_or_else(|| self.span().ty().into());
                    diags.push(
                        ImplDiag::AssocTypeParamSortMismatch {
                            primary: self
                                .span()
                                .associated_type(impl_idx)
                                .generic_params()
                                .param(param_idx)
                                .into(),
                            trait_decl_span,
                            type_name: name,
                            param_idx,
                        }
                        .into(),
                    );
                }
                Err(GatSigMismatch::ParamKind {
                    param_idx,
                    expected,
                    given,
                }) => {
                    let trait_decl_span = trait_decl_span
                        .map(|s| s.generic_params().param(param_idx).into())
                        .unwrap_or_else(|| self.span().ty().into());
                    diags.push(
                        ImplDiag::AssocTypeParamKindMismatch {
                            primary: self
                                .span()
                                .associated_type(impl_idx)
                                .generic_params()
                                .param(param_idx)
                                .into(),
                            trait_decl_span,
                            type_name: name,
                            param_idx,
                            expected,
                            given,
                        }
                        .into(),
                    );
                }
            }
        }
        diags
    }

    /// Diagnostics for missing associated consts (required by the trait).
    pub fn diags_missing_assoc_consts(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<TyDiagCollection<'db>> {
        use ty::diagnostics::ImplDiag;
        use ty::trait_lower::lower_impl_trait;

        let mut diags = Vec::new();
        let Some(implementor) = lower_impl_trait(db, self) else {
            return diags;
        };
        let implementor = implementor.instantiate_identity();
        let trait_hir = implementor.trait_def(db);

        // Check that all required trait consts are implemented
        for trait_const in trait_hir.assoc_consts(db) {
            let Some(name) = trait_const.name(db) else {
                continue;
            };
            let has_impl = self.const_(db, name).is_some();
            let has_default = trait_const.has_default(db);
            if !has_impl && !has_default {
                diags.push(
                    ImplDiag::MissingAssociatedConst {
                        primary: self.span().ty().into(),
                        const_name: name,
                        trait_: trait_hir,
                    }
                    .into(),
                );
            }
        }
        diags
    }

    /// Diagnostics for associated const values and validity.
    pub fn diags_assoc_consts(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use ty::diagnostics::ImplDiag;

        let Some(implementor) = lower_impl_trait(db, self) else {
            return Vec::new();
        };
        let implementor = implementor.instantiate_identity();
        let trait_hir = implementor.trait_def(db);
        let trait_args = implementor.trait_(db).args(db);

        let mut diags = Vec::new();
        for impl_const in self.assoc_consts(db) {
            let Some(name) = impl_const.name(db) else {
                continue;
            };

            if trait_hir.const_(db, name).is_some() {
                // Const is defined in trait - check it has a value
                if !impl_const.has_value(db) {
                    diags.push(
                        ImplDiag::MissingAssociatedConstValue {
                            primary: impl_const.span().ty().into(),
                            const_name: name,
                            trait_: trait_hir,
                        }
                        .into(),
                    );
                }
            } else {
                // Const is not defined in trait
                diags.push(
                    ImplDiag::ConstNotDefinedInTrait {
                        primary: impl_const.span().name().into(),
                        trait_: trait_hir,
                        const_name: name,
                    }
                    .into(),
                );
            }
        }

        // Validate the impl const's declared (header) type: surface lowering
        // errors, and require it to match the trait's declaration. Body
        // checking lives in the body analysis pass.
        for impl_const in self.assoc_consts(db) {
            let Some(name) = impl_const.name(db) else {
                continue;
            };
            let Some(impl_header_ty) = impl_const.ty(db) else {
                continue;
            };
            let scope = self.scope();
            let assumptions = constraints_for(db, self.into());
            if impl_header_ty.has_invalid(db) {
                let errs = impl_const.hir_ty(db).map(|hir_ty| {
                    collect_ty_lower_errors(db, scope, hir_ty, impl_const.span().ty(), assumptions)
                });
                match errs {
                    Some(errs) if !errs.is_empty() => diags.extend(errs),
                    _ => {
                        if let Some(diag) =
                            impl_header_ty.emit_diag(db, impl_const.span().ty().into())
                        {
                            diags.push(diag);
                        }
                    }
                }
                continue;
            }

            let Some(trait_const) = trait_hir.const_(db, name) else {
                continue;
            };
            let Some(expected) = trait_const.ty_binder(db) else {
                continue;
            };
            let instantiated_expected_ty = expected.instantiate(db, trait_args);
            if instantiated_expected_ty.has_invalid(db) {
                continue;
            }

            let expected_ty = normalize_ty(db, instantiated_expected_ty, scope, assumptions);
            let normalized_impl_header_ty = normalize_ty(db, impl_header_ty, scope, assumptions);
            if expected_ty != normalized_impl_header_ty {
                diags.push(
                    ImplDiag::ConstTyMismatchWithTrait {
                        primary: impl_const.span().ty().into(),
                        trait_decl_span: trait_const.span().ty().into(),
                        const_name: name,
                        trait_ty: expected_ty,
                        impl_ty: normalized_impl_header_ty,
                    }
                    .into(),
                );
            }

            // An associated-const initializer that reads `<Ty as Trait>::CONST`
            // carries a `Ty: Trait` trait obligation. For a DERIVE-PROVIDER-
            // generated impl this is the SOLE site that checks the generated
            // const body, so an unsatisfied bound (e.g. a derived
            // `impl AbiSize for Bad` whose `HEAD_SIZE` folds
            // `<NoAbi as AbiSize>::HEAD_SIZE` for a concrete field type lacking
            // `AbiSize`) must be surfaced here, otherwise the unsatisfiable
            // const-ref reaches semantic lowering and panics instead of
            // producing `6-0003`. Gate on the `Desugared(Derive)` origin:
            // error/msg-generated ABI impls already report the same
            // `Ty: AbiSize`/`Encode` bounds via their encode/decode lowering, so
            // surfacing here too would DUPLICATE them.
            if matches!(
                self.origin(db),
                crate::span::HirOrigin::Desugared(crate::span::DesugaredOrigin::Derive(_))
            ) && let Some(body) = impl_const.value_body(db)
            {
                let (body_diags, _) = check_anon_const_body(db, body, instantiated_expected_ty);
                diags.extend(body_diags.iter().filter_map(extract_satisfiability));
            }
        }

        // Const value bodies are type-checked by the body analysis pass
        // (`check_impl_trait_const_bodies`), which surfaces a value/declared-type
        // mismatch as a plain `TypeMismatch`, same as a top-level `const`.

        diags
    }

    /// Diagnostics for associated consts with recursive definitions
    /// (`const C: u32 = Self::C`, or cycles through other consts).
    ///
    /// On concrete impls every const is forced and must reach a concrete
    /// value: recursion either surfaces as a `RecursiveConst` evaluation
    /// error (through the eval-query cycle recovery) or "evaluates" to a
    /// symbolic self-reference (the salsa cycle in `evaluate_const_ty`
    /// recovers with the unevaluated form). On generic impls a
    /// param-dependent const legitimately stays abstract, so recursion is
    /// instead detected by walking the abstract form's resolution chain.
    pub fn diags_assoc_const_evaluability(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<TyDiagCollection<'db>> {
        use ty::assoc_const::AssocConstUse;
        use ty::const_ty::{
            ConstTyData, const_body_resolution_reenters, const_ty_from_assoc_const_use,
        };
        use ty::diagnostics::ImplDiag;

        // Recursion is user-written; expanded impls are compiler output.
        if !matches!(self.origin(db), crate::span::HirOrigin::Raw(_)) {
            return Vec::new();
        }
        let Some(implementor) = lower_impl_trait(db, self) else {
            return Vec::new();
        };
        let implementor = implementor.instantiate_identity();
        let trait_hir = implementor.trait_def(db);
        let inst = implementor.trait_inst(db);
        let scope = self.scope();
        let assumptions = constraints_for(db, self.into());

        let mut diags = Vec::new();
        for trait_const in trait_hir.assoc_consts(db) {
            let Some(name) = trait_const.name(db) else {
                continue;
            };
            let assoc = AssocConstUse::new(scope, assumptions, inst, name);
            let Some(const_ty) = const_ty_from_assoc_const_use(db, assoc) else {
                continue;
            };
            let declared_ty = trait_const
                .ty_binder(db)
                .map(|binder| binder.instantiate(db, inst.args(db)));
            let evaluated = const_ty.evaluate(db, declared_ty);
            if matches!(evaluated.data(db), ConstTyData::Evaluated(..)) {
                continue;
            }
            if evaluated.ty(db).has_invalid(db) {
                // Other invalid causes are reported by the body/header
                // checks; recursion surfacing as an eval error is this
                // diagnostic's job.
                if !matches!(
                    evaluated.ty(db).invalid_cause(db),
                    Some(ty::ty_def::InvalidCause::ConstEvalRecursiveConst { .. })
                ) {
                    continue;
                }
            } else {
                // A non-evaluated, non-invalid result is only recursion when
                // the const-ref resolution chain actually loops back to this
                // body. Anything else — generic-impl deferral, ambiguous or
                // otherwise erroneous references — is the body checks' job.
                let ConstTyData::UnEvaluated {
                    body: start_body,
                    ty: Some(start_ty),
                    generic_args: start_args,
                    ..
                } = const_ty.data(db)
                else {
                    continue;
                };
                if start_ty.has_invalid(db)
                    || !const_body_resolution_reenters(db, *start_body, *start_ty, start_args)
                {
                    continue;
                }
            }
            let primary = match self.const_(db, name) {
                Some(impl_const) => impl_const.span().name().into(),
                None => self.span().ty().into(),
            };
            diags.push(
                ImplDiag::RecursiveAssocConst {
                    primary,
                    const_name: name,
                }
                .into(),
            );
        }
        diags
    }

    /// Diagnostics for associated type bounds on implemented assoc types.
    ///
    /// Two legs, gated on the trait DECL's GAT arity (the `assoc_decl_arity`
    /// authority the A3 G1 gate reads):
    ///
    /// - **Arity 0** (a baseline assoc-type bound, `type Item: Bound`): the
    ///   unchanged permissive UnSat-only discharge. BYTE-IDENTICAL to pre-A4.2
    ///   (input-disjointness: the strict leg below only touches `arity > 0`).
    /// - **Arity > 0** (a GAT bound, `type Elem<T>: Producer<T>`, mb2-a4.2):
    ///   the bound is a UNIVERSAL claim over ALL rigid `T`, so it is discharged
    ///   on the STRICT path ([`type_fn_induct::strict_prove`]) rather than the
    ///   permissive solver. The goal is MIXED-OWNER today: the subject comes
    ///   from the impl RHS (`Store<T_impl>`, owner `ImplTraitType`) while the
    ///   trait-decl bound arg is `T_trait` (owner `TraitType`), so a correct
    ///   blanket impl spuriously UnSats (the two rigids read as distinct -- the
    ///   failure A2b.1 guarded out). [`GatOwnerRemap`] rewrites the
    ///   `TraitType`-owned bound params to the SAME impl-side rigids
    ///   `gat_param_ty` minted into the RHS (owner-exact, minted by the same
    ///   helper, so interned identity matches), reconciling subject and bound
    ///   onto ONE rigid. Only strict `Proven` is accepted: the permissive
    ///   UnSat-only pattern counts the depth-cap give-up as success, a real
    ///   hole for a "for all T" claim that later consumer legs will trust.
    ///
    /// v1 restriction (mb2-a4.2): a GAT param BOUND (`type Elem<T: Copy>: ...`)
    /// is NOT added as an assumption here. Sound-but-stricter: an RHS needing
    /// `T: Copy` is rejected even though the decl demands it of callers. The
    /// relaxation is UNSOUND unless the DUAL caller-side obligation (use sites
    /// prove `X: Copy`) lands in the SAME slice; neither exists yet, they are
    /// one future paired slice.
    pub fn diags_assoc_types_bounds(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<TyDiagCollection<'db>> {
        use ty::fold::TraitScopeSubstFolder;
        use ty::fold::TyFoldable as _;
        use ty::trait_lower::lower_impl_trait;
        use ty::trait_resolution::{GoalSatisfiability, PredicateListId, ProvisionEnv, is_goal_satisfiable};
        use ty::ty_lower::{gat_param_ty, gat_signature_conforms};
        use ty::type_fn_induct::{StrictResult, strict_prove};

        // Owner-exact GAT param remap (mb2-a4.2). Rewrites each trait-decl GAT
        // param (`TyParam{owner == TraitType(t, decl_idx), idx j}`) to the
        // impl-side rigid `gat_param_ty(db, def_params[j], j,
        // ImplTraitType(imp, def_idx))` -- minted by the SAME helper the impl
        // RHS's own params were minted with (`path_resolver.rs`
        // `ImplTraitTypeParam` arm), so interned identity matches by
        // construction and subject + bound end up sharing ONE rigid. Owner-exact
        // on the single def-node scope of THIS decl; the LOCAL idx is looked up
        // only within `def_params`, so an impl/caller param sharing a numeric
        // index (owner != `decl_scope`) is left intact (the S2 non-capture
        // discipline). The `.get(idx)` guard is belt over the A4.1 arity check.
        struct GatOwnerRemap<'db, 'a> {
            decl_scope: ScopeId<'db>,
            impl_owner: ScopeId<'db>,
            def_params: &'a [GenericParam<'db>],
        }

        impl<'db> ty::fold::TyFolder<'db> for GatOwnerRemap<'db, '_> {
            fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
                match ty.data(db) {
                    ty::ty_def::TyData::TyParam(param)
                        if !param.is_effect() && param.owner == self.decl_scope =>
                    {
                        match self.def_params.get(param.idx) {
                            Some(p) => gat_param_ty(db, p, param.idx, self.impl_owner),
                            None => ty,
                        }
                    }

                    ty::ty_def::TyData::ConstTy(const_ty) => match const_ty.data(db) {
                        ty::const_ty::ConstTyData::TyParam(param, _)
                            if !param.is_effect() && param.owner == self.decl_scope =>
                        {
                            match self.def_params.get(param.idx) {
                                Some(p) => gat_param_ty(db, p, param.idx, self.impl_owner),
                                None => ty,
                            }
                        }

                        _ => ty.super_fold_with(db, self),
                    },

                    _ => ty.super_fold_with(db, self),
                }
            }
        }

        let mut diags = Vec::new();
        let Some(implementor) = lower_impl_trait(db, self) else {
            return diags;
        };
        let implementor = implementor.instantiate_identity();
        let trait_args = implementor.trait_(db).args(db);
        let trait_hir = implementor.trait_def(db);
        let trait_scope = trait_hir.scope();
        let assumptions = param_env(db, self.into());
        let solve_cx = ProvisionEnv::for_scope(self.scope(), assumptions).solve_cx(db);

        // FIX 2 (mb2-a5.0 / steering-08): mirror `strict_prove`'s assumptions-side
        // pre-flight. If the impl's own assumptions carry an `Invalid` (e.g. a
        // typo'd where-clause, already diagnosed elsewhere), `strict_prove`
        // declines on EVERY goal below (`Invalid` unifies with everything), which
        // would spray a spurious `TraitBoundNotSat` for every GAT bound in this
        // impl. Suppress the bound diagnostics in that case, symmetric to the
        // existing goal-side `has_invalid` guard (this is the assumptions half of
        // the same pre-flight). `HAS_VAR` is deliberately NOT checked: it cannot
        // legitimately occur here and should stay loud if it ever does.
        if crate::analysis::ty::visitor::collect_flags(db, assumptions)
            .contains(crate::analysis::ty::ty_def::TyFlags::HAS_INVALID)
        {
            return diags;
        }

        for assoc in implementor.assoc_type_views(db) {
            let Some(name) = assoc.name(db) else { continue };

            // The trait DECL is the arity authority; its idx (position in
            // `trait.types`, == the enumeration order of `assoc_types`) is also
            // the GAT bound params' owner idx.
            let decl = trait_hir.assoc_ty(db, name);
            let decl_idx = trait_hir
                .assoc_types(db)
                .position(|v| v.name(db) == Some(name));
            let arity = decl
                .map(|d| d.generic_params.data(db).len())
                .unwrap_or(0);

            // The impl-side def (idx = position in `impl.types`), for the remap
            // target owner + param binders. May be absent if the binding came
            // from a merged trait default (A4.3 forward-compat); in v1 arity>0
            // defaults are guarded out so this is present for every GAT.
            //
            // FIX 3 (mb2-a5.0 / steering-08): key the explicit-vs-merged split on
            // the SAME condition the merge used. `assoc_type_bindings_for_trait_inst`
            // treats an impl assoc as explicitly bound iff `v.name(db).zip(v.ty(db))`
            // is `Some`, i.e. iff `ImplAssocTypeView::ty(db).is_some()`. A
            // parse-broken def has a name but no ty; it must route to the
            // merged-default (None) leg, not be taken for an explicit binding whose
            // owner-remap against a trait-side-rigid subject would fail spuriously.
            let def_lookup = self
                .assoc_types(db)
                .enumerate()
                .find(|(_, v)| v.name(db) == Some(name) && v.ty(db).is_some())
                .map(|(def_idx, _)| (def_idx, &self.types(db)[def_idx]));

            // ---- A7.1 leg 1 (callee): the GAT-bound duality relaxation.
            // Fold this decl's GAT param guards (`type Elem<T: G>` -> `T: G`)
            // through the SAME pipeline the RHS goal takes, then merge them into
            // the strict-prove ASSUMPTIONS. The impl RHS may now assume `T: G`
            // exactly as the decl demands `G` of every caller (leg 2, the dual).
            //
            // Injection is confined to this discharge context (I1): guards NEVER
            // enter `param_env` / `collect_constraints` / `extend_all_bounds`,
            // so the A5.0 FIX-1 pin `gat_bound_no_assoc_owned_param_in_param_env`
            // stays green and stays teeth. `CanonicalGoalQuery::new` runs
            // `extend_all_bounds` over these assumptions per-query, closing the
            // guard under supertraits INSIDE the discharge query and nowhere
            // else. The guard's subject rigid is the SAME interned rigid the RHS
            // subject carries (I2), by construction of the shared folders +
            // `gat_param_ty` mint (explicit leg: `GatOwnerRemap` to the impl-side
            // rigid; merged-default leg: no remap, trait-side rigid already
            // shared). See mb2-fable-steering-10, "A7 callee side".
            let decl_view = trait_hir.assoc_types(db).find(|v| v.name(db) == Some(name));
            let mut discharge_guards: Vec<ty::trait_def::TraitInstId<'db>> = Vec::new();
            if arity > 0
                && let Some(decl_view) = decl_view
            {
                for (_j, guard) in decl_view.gat_param_guard_insts(db) {
                    // Step 1 (both legs): trait Self + item params -> this impl's
                    // trait args, through the shared `TraitScopeSubstFolder`.
                    let mut folder = TraitScopeSubstFolder {
                        trait_scope,
                        trait_args,
                    };
                    let guard = guard.fold_with(db, &mut folder);
                    // Step 2 (explicit leg only): decl rigids -> the impl-side
                    // rigids the RHS subject carries. Merged-default leg (None):
                    // no remap; subject and guard already share the trait-side
                    // rigid.
                    let guard = if let (Some((def_idx, def)), Some(decl_idx)) = (def_lookup, decl_idx)
                    {
                        let mut remap = GatOwnerRemap {
                            decl_scope: ScopeId::TraitType(trait_hir, decl_idx as u16),
                            impl_owner: ScopeId::ImplTraitType(self, def_idx as u16),
                            def_params: def.generic_params.data(db),
                        };
                        guard.fold_with(db, &mut remap)
                    } else {
                        guard
                    };
                    discharge_guards.push(guard);
                }
            }

            // FIX 2 parity (A7.1): a guard carrying `Invalid` (an unlowerable
            // arg, already diagnosed at the decl by `diags_gat_param_guards`)
            // would make `strict_prove` decline every RHS bound and spray noise.
            // Suppress this assoc type's bound diagnostics, symmetric to the
            // goal-side `has_invalid` guard below and the assumptions-side FIX-2
            // pre-flight above.
            if discharge_guards
                .iter()
                .any(|g| g.args(db).iter().any(|ty| ty.has_invalid(db)))
            {
                continue;
            }

            let discharge_assumptions = if discharge_guards.is_empty() {
                assumptions
            } else {
                let mut merged = assumptions.list(db).clone();
                merged.extend(discharge_guards.iter().copied());
                PredicateListId::new(db, merged)
            };

            for bound_inst in assoc.bounds(db) {
                let mut folder = TraitScopeSubstFolder {
                    trait_scope,
                    trait_args,
                };
                let bound_inst = bound_inst.fold_with(db, &mut folder);

                // ---- Arity-0 baseline leg: permissive UnSat-only. BYTE-IDENTICAL.
                if arity == 0 {
                    if let GoalSatisfiability::UnSat(_) =
                        is_goal_satisfiable(db, solve_cx, bound_inst)
                    {
                        let assoc_ty_span = self
                            .associated_type_span(db, name)
                            .map(|s| s.ty().into())
                            .unwrap_or_else(|| self.span().ty().into());

                        diags.push(
                            TraitConstraintDiag::TraitBoundNotSat {
                                span: assoc_ty_span,
                                primary_goal: bound_inst,
                                unsat_subgoal: None,
                                required_by: None,
                            }
                            .into(),
                        );
                    }
                    continue;
                }

                // ---- Arity>0 GAT leg: two shapes reach here.
                //
                //  (a) EXPLICIT impl def (`def_lookup` present, mb2-a4.2): the
                //      goal is MIXED-OWNER -- the subject is the impl RHS
                //      (`Store<T_impl>`, owner `ImplTraitType`), the bound arg is
                //      the trait-decl param (`T_trait`, owner `TraitType`). The
                //      owner-exact `GatOwnerRemap` rewrites `T_trait` to the SAME
                //      impl-side rigid the RHS carries, reconciling subject +
                //      bound onto ONE rigid, then strict-proves.
                //
                //  (b) MERGED trait default (`def_lookup` None, mb2-a4.3): the
                //      subject IS the trait-side default RHS
                //      (`assoc_type_bindings_for_trait_inst` merged it via
                //      `instantiate_scoped`, which left the GAT params rigid as
                //      `TraitType`-owned), so subject and bound arg are ALREADY
                //      the SAME trait-side rigid -- there is NO impl-side rigid to
                //      remap onto, so the remap is SKIPPED. The bound is still
                //      discharged STRICTLY under THIS impl's `param_env` (uniform
                //      per-impl check; the trait-site `diags_assoc_defaults` check
                //      stays advisory, the binding-site strict check is the one
                //      that licenses projection of the inherited default). A
                //      merged default carries only the trait's own assumptions,
                //      so re-discharging under the impl env can only ADD
                //      assumptions (the impl's where-clauses), never smuggle an
                //      impl-specific obligation past: if the impl needs an extra
                //      assumption to satisfy the default's bound and lacks it,
                //      the strict prove rejects it here.
                let goal = match def_lookup {
                    Some((def_idx, def)) => {
                        // Anti-cascade: a non-conforming binder was already
                        // diagnosed by `diags_assoc_type_conformance`
                        // (6-0023..25). Skip its bound so the user sees exactly
                        // the signature mismatch, no trailing noise.
                        let (Some(decl), Some(decl_idx)) = (decl, decl_idx) else {
                            continue;
                        };
                        if gat_signature_conforms(db, decl, def).is_err() {
                            continue;
                        }

                        let mut remap = GatOwnerRemap {
                            decl_scope: ScopeId::TraitType(trait_hir, decl_idx as u16),
                            impl_owner: ScopeId::ImplTraitType(self, def_idx as u16),
                            def_params: def.generic_params.data(db),
                        };
                        bound_inst.fold_with(db, &mut remap)
                    }
                    None => bound_inst,
                };

                // Suppress on already-invalid goals: `strict_prove`'s pre-flight
                // declines HAS_INVALID, so a diagnostic here would be pure noise
                // on code with an existing error (e.g. a const-GAT param, whose
                // decl-site diagnostic A4.4 already emitted).
                if goal.args(db).iter().any(|ty| ty.has_invalid(db)) {
                    continue;
                }

                if strict_prove(db, solve_cx.origin_ingot(), goal, discharge_assumptions)
                    != StrictResult::Proven
                {
                    let assoc_ty_span = self
                        .associated_type_span(db, name)
                        .map(|s| s.ty().into())
                        .unwrap_or_else(|| self.span().ty().into());

                    diags.push(
                        TraitConstraintDiag::TraitBoundNotSat {
                            span: assoc_ty_span,
                            primary_goal: goal,
                            unsat_subgoal: None,
                            required_by: None,
                        }
                        .into(),
                    );
                }
            }
        }
        diags
    }

    /// Diagnostics for trait-ref WF and satisfiability for this impl-trait.
    pub fn diags_trait_ref_and_wf(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use ty::trait_lower::lower_impl_trait;
        use ty::trait_resolution::{
            self, GoalSatisfiability, WellFormedness, check_trait_inst_wf,
            constraint::collect_constraints,
        };

        let mut diags = Vec::new();
        let Some(implementor) = lower_impl_trait(db, self) else {
            return diags;
        };
        let implementor = implementor.instantiate_identity();
        let trait_inst = implementor.trait_(db);
        let trait_def = implementor.trait_def(db);

        let assumptions = collect_constraints(db, self.into()).instantiate_identity();
        let solve_cx =
            trait_resolution::ProvisionEnv::for_scope(self.scope(), assumptions).solve_cx(db);

        if let WellFormedness::IllFormed { goal, subgoal } =
            check_trait_inst_wf(db, solve_cx, trait_inst)
        {
            diags.push(
                TraitConstraintDiag::TraitBoundNotSat {
                    span: self.span().trait_ref().into(),
                    primary_goal: goal,
                    unsat_subgoal: subgoal,
                    required_by: None,
                }
                .into(),
            );
            return diags;
        }

        let is_satisfied = |goal, span: DynLazySpan<'db>, out: &mut Vec<_>| {
            match trait_resolution::is_goal_satisfiable(db, solve_cx, goal) {
                GoalSatisfiability::Satisfied(_) | GoalSatisfiability::ContainsInvalid => {}
                GoalSatisfiability::NeedsConfirmation(_) => {}
                GoalSatisfiability::UnSat(_) => {
                    out.push(
                        TraitConstraintDiag::TraitBoundNotSat {
                            span,
                            primary_goal: goal,
                            unsat_subgoal: None,
                            required_by: None,
                        }
                        .into(),
                    );
                }
            }
        };

        let target_ty_span: DynLazySpan<'db> = self.span().ty().into();
        for super_trait in trait_def.super_traits(db) {
            let super_trait = super_trait.instantiate(db, trait_inst.args(db));
            is_satisfied(super_trait, target_ty_span.clone(), &mut diags)
        }

        diags
    }

    /// Diagnostics for implemented associated types' WF and invalid types.
    pub fn diags_assoc_types_wf(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        self.assoc_types(db)
            .flat_map(|view| view.diags(db))
            .collect()
    }
}

impl<'db> Diagnosable<'db> for ImplAssocTypeView<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        self.ty_diags(db)
    }
}

impl<'db> Diagnosable<'db> for Struct<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        let mut out = Vec::new();

        out.extend(check_duplicate_names(
            FieldParent::Struct(self).fields(db).map(|v| v.name(db)),
            |idxs| TyLowerDiag::DuplicateFieldName(FieldParent::Struct(self), idxs).into(),
        ));

        for v in FieldParent::Struct(self).fields(db) {
            out.extend(v.diags(db));
        }

        for pred in WhereClauseOwner::Struct(self).clause(db).predicates(db) {
            out.extend(pred.diags(db));
        }

        out.extend(GenericParamOwner::Struct(self).diags(db));
        out
    }
}

impl<'db> VariantView<'db> {
    /// Diagnostics for tuple-variant element types: star-kind and non-const checks.
    /// Returns an empty list if this is not a tuple variant.
    pub fn diags_tuple_elems_wf(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use crate::hir_def::types::TypeKind as HirTyKind;
        use name_resolution::{PathRes, resolve_path};
        use ty::trait_resolution::{ProvisionEnv, check_ty_wf};
        use ty::ty_lower::lower_hir_ty;

        let mut out = Vec::new();
        let VariantKind::Tuple(tuple_id) = self.kind(db) else {
            return out;
        };

        let enum_ = self.owner;
        let var = EnumVariant::new(enum_, self.idx);
        let scope = var.scope();
        let assumptions = constraints_for(db, enum_.into());

        for (elem_idx, p) in tuple_id.data(db).iter().enumerate() {
            let Some(hir_ty) = p.to_opt() else {
                continue;
            };

            let span = self.span().tuple_type().elem_ty(elem_idx);

            // For non-const subjects, surface name-resolution/path-domain errors first.
            let is_const_path = match hir_ty.data(db) {
                HirTyKind::Path(path) => {
                    if let Some(path) = path.to_opt() {
                        matches!(
                            resolve_path(db, path, scope, assumptions, true),
                            Ok(PathRes::Const(..))
                        )
                    } else {
                        false
                    }
                }
                _ => false,
            };

            if !is_const_path {
                let mut errs = ty::ty_error::collect_ty_lower_errors(
                    db,
                    scope,
                    hir_ty,
                    span.clone(),
                    assumptions,
                );
                if !errs.is_empty() {
                    out.append(&mut errs);
                    continue;
                }
            }

            let ty = lower_hir_ty(db, hir_ty, scope, assumptions);
            if ty.has_invalid(db) {
                continue;
            }
            if !ty.has_star_kind(db) {
                out.push(TyLowerDiag::ExpectedStarKind(span.clone().into()).into());
                continue;
            }
            if ty.is_const_ty(db) {
                out.push(
                    TyLowerDiag::NormalTypeExpected {
                        span: span.clone().into(),
                        given: ty,
                    }
                    .into(),
                );
                continue;
            }

            // Well-formedness (trait bounds and const predicates) for element type.
            if let Some(diag) = check_ty_wf(
                db,
                ProvisionEnv::for_scope(scope, assumptions).solve_cx(db),
                ty,
            )
            .into_diag(span.clone().into())
            {
                out.push(diag);
            }
        }

        out
    }
}

impl<'db> Diagnosable<'db> for FieldView<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        self.ty_diags(db)
    }
}

impl<'db> Diagnosable<'db> for Enum<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        let mut out = Vec::new();

        out.extend(check_duplicate_names(
            self.variants(db).map(|v| v.name(db)),
            |idxs| TyLowerDiag::DuplicateVariantName(self, idxs).into(),
        ));

        for v in self.variants(db) {
            if matches!(v.kind(db), VariantKind::Record(_)) {
                out.extend(check_duplicate_names(
                    v.fields(db).map(|f| f.name(db)),
                    |idxs| {
                        TyLowerDiag::DuplicateFieldName(
                            FieldParent::Variant(EnumVariant::new(self, v.idx)),
                            idxs,
                        )
                        .into()
                    },
                ));
                for f in v.fields(db) {
                    out.extend(f.diags(db));
                }
            } else if matches!(v.kind(db), VariantKind::Tuple(_)) {
                out.extend(v.diags_tuple_elems_wf(db));
            }
        }

        for pred in WhereClauseOwner::Enum(self).clause(db).predicates(db) {
            out.extend(pred.diags(db));
        }

        out.extend(GenericParamOwner::Enum(self).diags(db));
        out
    }
}

impl<'db> Diagnosable<'db> for Contract<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        let mut out = Vec::new();
        out.extend(check_duplicate_names(
            FieldParent::Contract(self).fields(db).map(|v| v.name(db)),
            |idxs| TyLowerDiag::DuplicateFieldName(FieldParent::Contract(self), idxs).into(),
        ));
        for v in FieldParent::Contract(self).fields(db) {
            out.extend(v.diags(db));
        }
        out
    }
}

impl<'db> Diagnosable<'db> for AdtRef<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        match self {
            AdtRef::Struct(s) => s.diags(db),
            AdtRef::Enum(e) => e.diags(db),
        }
    }
}

impl<'db> GenericParamOwner<'db> {
    pub fn diags_params_defined_in_parent(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl Iterator<Item = TyDiagCollection<'db>> + 'db {
        self.params(db).filter_map(|param| {
            param
                .diag_param_defined_in_parent(db)
                .map(TyDiagCollection::from)
        })
    }

    pub fn diags_check_duplicate_names(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> impl Iterator<Item = TyDiagCollection<'db>> + 'db {
        let params_iter = self.params(db).map(|v| v.name().to_opt());
        check_duplicate_names(params_iter, |idxs| {
            TyDiagCollection::from(TyLowerDiag::DuplicateGenericParamName(self, idxs))
        })
        .into_iter()
    }

    pub fn diags_non_trailing_defaults(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<TyDiagCollection<'db>> {
        let mut out = Vec::new();
        let mut default_idxs = Vec::new();
        for view in self.params(db) {
            let is_defaulted_type =
                matches!(view.param, GenericParam::Type(tp) if tp.default_ty.is_some());
            if is_defaulted_type {
                default_idxs.push(view.idx);
            } else if !default_idxs.is_empty() {
                for &idx in &default_idxs {
                    let span = self.param_view(db, idx).span();
                    out.push(TyLowerDiag::NonTrailingDefaultGenericParam(span).into());
                }
                break;
            }
        }
        out
    }

    pub fn diags_const_param_types(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use ty::ty_def::{InvalidCause, TyData};

        let mut out = Vec::new();
        let param_set = ty::ty_lower::collect_generic_params(db, self);
        for view in self.params(db) {
            let GenericParam::Const(c) = view.param else {
                continue;
            };
            if c.ty.to_opt().is_none() {
                continue;
            }
            if let Some(ty) = param_set.param_by_original_idx(db, view.idx) {
                let cause_opt = match ty.data(db) {
                    TyData::Invalid(cause) => Some(cause.clone()),
                    TyData::ConstTy(ct) => match ct.ty(db).data(db) {
                        TyData::Invalid(cause) => Some(cause.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(cause) = cause_opt {
                    let span = view.span().into_const_param().ty();
                    match cause {
                        InvalidCause::InvalidConstParamTy => {
                            out.push(TyLowerDiag::InvalidConstParamTy(span.into()).into());
                        }
                        InvalidCause::RecursiveConstParamTy => {
                            out.push(TyLowerDiag::RecursiveConstParamTy(span.into()).into());
                        }
                        InvalidCause::ConstTyExpected { expected } => {
                            out.push(
                                TyLowerDiag::ConstTyExpected {
                                    span: span.into(),
                                    expected,
                                }
                                .into(),
                            );
                        }
                        InvalidCause::ConstTyMismatch { expected, given } => {
                            out.push(const_ty_mismatch_diag(span.into(), expected, given));
                        }
                        _ => {}
                    }
                }
            }
        }
        out
    }

    pub fn diags_default_forward_refs(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<TyDiagCollection<'db>> {
        use ty::{
            ty_def::{TyId, TyParam},
            ty_lower::lower_hir_ty,
            visitor::{TyVisitable, TyVisitor},
        };

        let mut out = Vec::new();
        // Forward-ref checking only needs parameter occurrences in default types.
        // Full assumptions can create non-converging cycles on malformed defaults
        // (e.g. `T = Self`) and should not panic diagnostics collection.
        let assumptions = ty::trait_resolution::PredicateListId::empty_list(db);
        let scope = self.scope();

        for view in self.params(db) {
            let default_ty = match view.param {
                GenericParam::Type(tp) => tp.default_ty,
                GenericParam::Const(_) => None,
            };
            let Some(default_ty) = default_ty else {
                continue;
            };

            if default_ty.is_self_ty(db) {
                continue;
            }

            let lowered = lower_hir_ty(db, default_ty, scope, assumptions);

            struct Collector<'db> {
                db: &'db dyn HirAnalysisDb,
                scope: ScopeId<'db>,
                out: Vec<usize>,
            }
            impl<'db> TyVisitor<'db> for Collector<'db> {
                fn db(&self) -> &'db dyn HirAnalysisDb {
                    self.db
                }
                fn visit_param(&mut self, tp: &TyParam<'db>) {
                    if !tp.is_trait_self() && tp.owner == self.scope {
                        self.out.push(tp.original_idx(self.db));
                    }
                }
                fn visit_const_param(&mut self, tp: &TyParam<'db>, _ty: TyId<'db>) {
                    if tp.owner == self.scope {
                        self.out.push(tp.original_idx(self.db));
                    }
                }
            }

            let mut collector = Collector {
                db,
                scope,
                out: Vec::new(),
            };
            lowered.visit_with(&mut collector);

            for j in collector.out.into_iter().filter(|j| *j >= view.idx) {
                if let Some(name) = self.param_view(db, j).param.name().to_opt() {
                    let span = view.span();
                    out.push(TyLowerDiag::GenericDefaultForwardRef { span, name }.into());
                }
            }
        }

        out
    }

    pub fn diags_kind_bounds(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        let mut out = Vec::new();
        let param_set = ty::ty_lower::collect_generic_params(db, self);

        for view in self.params(db) {
            let GenericParam::Type(tp) = view.param else {
                continue;
            };
            let Some(ty) = param_set.param_by_original_idx(db, view.idx) else {
                continue;
            };
            let actual = ty.kind(db);

            for (i, bound) in tp.bounds.iter().enumerate() {
                if let TypeBound::Kind(Partial::Present(kb)) = bound {
                    let expected = lower_hir_kind_local(kb);
                    if !actual.does_match(&expected) {
                        let span = view.span().into_type_param().bounds().bound(i).kind_bound();
                        out.push(
                            TyLowerDiag::InconsistentKindBound {
                                span: span.into(),
                                ty,
                                bound: expected,
                            }
                            .into(),
                        );
                    }
                }
            }
        }

        out
    }

    pub fn diags_trait_bounds(self, db: &'db dyn HirAnalysisDb) -> Vec<TyDiagCollection<'db>> {
        use name_resolution::{ExpectedPathKind, diagnostics::PathResDiag};
        use ty::trait_lower::{self, TraitRefLowerError};
        use ty::trait_resolution::check_trait_inst_wf;

        let mut out = Vec::new();
        let param_set = ty::ty_lower::collect_generic_params(db, self);
        let scope = self.scope();
        let assumptions = header_constraints_for(db, self.into());

        for view in self.params(db) {
            let GenericParam::Type(tp) = view.param else {
                continue;
            };
            let Some(subject) = param_set.param_by_original_idx(db, view.idx) else {
                continue;
            };

            for (i, bound) in tp.bounds.iter().enumerate() {
                let TypeBound::Trait(tr) = bound else {
                    continue;
                };
                let span = view
                    .span()
                    .into_type_param()
                    .bounds()
                    .bound(i)
                    .trait_bound();
                match trait_lower::lower_trait_ref(
                    db,
                    subject,
                    *tr,
                    scope,
                    assumptions,
                    ty::trait_resolution::constraint::enclosing_trait_self_ty(db, scope),
                ) {
                    Ok(inst) => {
                        let expected = inst.def(db).self_param(db).kind(db);
                        if !expected.does_match(subject.kind(db)) {
                            out.push(
                                TraitConstraintDiag::TraitArgKindMismatch {
                                    span: span.clone(),
                                    expected: expected.clone(),
                                    actual: subject,
                                }
                                .into(),
                            );
                        }

                        if inst.self_ty(db).contains_assoc_ty_of_param(db) {
                            continue;
                        }

                        if let Some(diag) = check_trait_inst_wf(
                            db,
                            ty::trait_resolution::ProvisionEnv::for_scope(scope, assumptions)
                                .solve_cx(db),
                            inst,
                        )
                        .into_diag(span.into())
                        {
                            out.push(diag);
                        }
                    }
                    Err(TraitRefLowerError::PathResError(err)) => {
                        if let Some(path) = tr.path(db).to_opt()
                            && let Some(diag) =
                                err.into_diag(db, path, span.path(), ExpectedPathKind::Trait)
                        {
                            out.push(diag.into());
                        }
                    }
                    Err(TraitRefLowerError::InvalidDomain(res)) => {
                        if let Some(path) = tr.path(db).to_opt()
                            && let Some(ident) = path.ident(db).to_opt()
                        {
                            out.push(
                                PathResDiag::ExpectedTrait(
                                    span.path().into(),
                                    ident,
                                    res.kind_name(),
                                )
                                .into(),
                            );
                        }
                    }
                    Err(TraitRefLowerError::Cycle) => {
                        out.push(cyclic_trait_ref_diag(span.path().into(), "trait bound"));
                    }
                    Err(
                        TraitRefLowerError::UnsafeLocalBoundBlanketImpl
                        | TraitRefLowerError::Ignored,
                    ) => {}
                }
            }
        }

        out
    }
}

impl<'db> GenericParamView<'db> {
    pub fn diag_param_defined_in_parent(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<TyLowerDiag<'db>> {
        use crate::analysis::name_resolution::{PathRes, resolve_path};
        use crate::analysis::ty::trait_resolution::PredicateListId;

        let name = self.param.name().to_opt()?;
        let parent_scope = self.owner.scope().parent_item(db)?.scope();
        let path = PathId::from_ident(db, name);
        let span = self.span();

        match resolve_path(
            db,
            path,
            parent_scope,
            PredicateListId::empty_list(db),
            false,
        ) {
            Ok(r @ PathRes::Ty(ty)) if ty.is_param(db) => {
                Some(TyLowerDiag::GenericParamAlreadyDefinedInParent {
                    span,
                    conflict_with: r.name_span(db).unwrap(),
                    name,
                })
            }
            _ => None,
        }
    }
}

impl<'db> Diagnosable<'db> for GenericParamOwner<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        let mut out = Vec::new();
        out.extend(self.diags_check_duplicate_names(db));
        out.extend(self.diags_const_param_types(db));
        out.extend(self.diags_params_defined_in_parent(db));
        out.extend(self.diags_kind_bounds(db));
        out.extend(self.diags_trait_bounds(db));
        out.extend(self.diags_non_trailing_defaults(db));
        out.extend(self.diags_default_forward_refs(db));
        out
    }
}

impl<'db> Diagnosable<'db> for Func<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        use ty::canonical::Canonical;
        use ty::method_table::probe_method;

        let mut out = Vec::new();
        out.extend(self.diags_const_fn(db));
        out.extend(self.diags_parameters(db));
        out.extend(self.diags_param_types(db));
        out.extend(self.diags_return(db));

        for pred in WhereClauseOwner::Func(self).clause(db).predicates(db) {
            out.extend(pred.diags(db));
        }

        // Method conflict check only for inherent impls
        if let Some(crate::hir_def::scope_graph::ScopeId::Item(ItemKind::Impl(impl_))) =
            self.scope().parent(db)
            && let Some(func_def) = self.as_callable(db)
            && let Some(self_ty) = impl_.admissible_inherent_impl_ty(db)
        {
            let ingot = self.top_mod(db).ingot(db);
            for &cand in probe_method(
                db,
                ingot,
                Canonical::new(db, self_ty),
                func_def.name(db).expect("impl methods have names"),
            ) {
                if cand.def != func_def {
                    out.push(
                        ty::diagnostics::ImplDiag::ConflictMethodImpl {
                            primary: func_def,
                            conflict_with: cand.def,
                        }
                        .into(),
                    );
                    break;
                }
            }
        }

        out.extend(GenericParamOwner::Func(self).diags(db));
        out
    }
}

impl<'db> Diagnosable<'db> for Trait<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        let mut out = Vec::new();
        out.extend(self.diags_assoc_defaults(db));
        out.extend(self.diags_gat_const_params(db));
        out.extend(self.diags_gat_param_guards(db));
        out.extend(self.diags_super_traits(db));

        for pred in WhereClauseOwner::Trait(self).clause(db).predicates(db) {
            out.extend(pred.diags(db));
        }

        out.extend(GenericParamOwner::Trait(self).diags(db));
        out
    }
}

impl<'db> Diagnosable<'db> for Impl<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        let mut out = self.diags_preconditions(db);
        out.extend(self.diags_assoc_consts(db));
        out.extend(GenericParamOwner::Impl(self).diags(db));
        out
    }
}

impl<'db> Diagnosable<'db> for ImplTrait<'db> {
    type Diagnostic = TyDiagCollection<'db>;

    fn diags(self, db: &'db dyn HirAnalysisDb) -> Vec<Self::Diagnostic> {
        // Early path/domain/WF checks; bail out on errors to avoid noisy follow-ups
        let (implementor_opt, validity_diags) = self.diags_implementor_validity(db);
        let Some(implementor) = implementor_opt else {
            return validity_diags;
        };

        let mut out = validity_diags;
        out.extend(implementor.skip_binder().diags_method_conformance(db));
        out.extend(self.diags_trait_ref_and_wf(db));
        out.extend(self.diags_assoc_types_wf(db));
        out.extend(self.diags_missing_assoc_types(db));
        out.extend(self.diags_assoc_type_conformance(db));
        out.extend(self.diags_assoc_types_bounds(db));
        out.extend(self.diags_missing_assoc_consts(db));
        out.extend(self.diags_assoc_consts(db));
        out.extend(self.diags_assoc_const_evaluability(db));
        out.extend(GenericParamOwner::ImplTrait(self).diags(db));
        out
    }
}
