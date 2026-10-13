use crate::{
    hir_def::{GenericArg, PathId, TypeId, TypeKind, scope_graph::ScopeId},
    span::{params::LazyGenericArgSpan, path::LazyPathSpan, types::LazyTySpan},
    visitor::{Visitor, VisitorCtxt, prelude::DynLazySpan, walk_generic_arg, walk_path, walk_type},
};

use crate::analysis::{
    HirAnalysisDb,
    name_resolution::{
        ExpectedPathKind, PathRes, diagnostics::PathResDiag,
        resolve_path_with_observer_and_premises,
    },
    ty::visitor::{TyVisitor, walk_ty},
};

use super::{
    const_ty::{ConstBodyLowering, ConstTyData, HoleAnchor, LoweringContext, ty_is_fully_ground},
    diagnostics::{TyDiagCollection, TyLowerDiag},
    ty_def::{InvalidCause, TyData, TyId},
    ty_lower::lower_hir_ty_in_mode,
};
use crate::visitor::prelude::LazyTraitRefSpan;

/// Collect all type-lowering diagnostics for a HIR type written at the
/// position whose premises are `premises`: the type is lowered and resolved
/// under them, and its type applications' requirements are checked under them.
///
/// This encapsulates the two-phase error collection strategy:
/// 1. First tries to get precise HIR-based errors (path resolution, visibility)
/// 2. Falls back to semantic `InvalidCause` errors if none found
pub fn collect_hir_ty_diags<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: TypeId<'db>,
    span: LazyTySpan<'db>,
    premises: crate::analysis::semantic::SolverPremises<'db>,
) -> Vec<TyDiagCollection<'db>> {
    collect_hir_ty_diags_in_mode(db, hir_ty, span, premises, ConstBodyLowering::Eager)
}

/// Collect structural diagnostics for a declaration template. Anonymous const
/// bodies are checked by the declaration's body-validation owner.
pub(crate) fn collect_hir_ty_diags_deferred<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: TypeId<'db>,
    span: LazyTySpan<'db>,
    premises: crate::analysis::semantic::SolverPremises<'db>,
) -> Vec<TyDiagCollection<'db>> {
    collect_hir_ty_diags_in_mode(db, hir_ty, span, premises, ConstBodyLowering::Deferred)
}

fn collect_hir_ty_diags_in_mode<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: TypeId<'db>,
    span: LazyTySpan<'db>,
    premises: crate::analysis::semantic::SolverPremises<'db>,
    const_bodies: ConstBodyLowering,
) -> Vec<TyDiagCollection<'db>> {
    // Try precise HIR-based errors first
    let diags = collect_ty_lower_errors_in_mode(db, hir_ty, span.clone(), premises, const_bodies);
    if !diags.is_empty() {
        return diags;
    }

    // Fall back to semantic errors
    let (scope, assumptions) = (premises.scope(), premises.assumptions());
    let ty = lower_hir_ty_in_mode(db, hir_ty, scope, assumptions, const_bodies);
    emit_invalid_ty_error(db, ty, span.clone().into())
        .or_else(|| normalization_limit_diag(db, premises, ty, span.into()))
        .into_iter()
        .collect()
}

/// Surface a reduction limit at the written projection before later passes
/// consume its invalid normalized result. The existing reducer owns the limit.
pub(crate) fn normalization_limit_diag<'db>(
    db: &'db dyn HirAnalysisDb,
    premises: crate::analysis::semantic::SolverPremises<'db>,
    ty: TyId<'db>,
    span: DynLazySpan<'db>,
) -> Option<TyDiagCollection<'db>> {
    if !ty.has_projection(db) {
        return None;
    }
    let normalized = super::normalize::normalize_ty(db, ty, premises);
    super::fold::ty_contains(
        db,
        normalized,
        TyId::invalid(db, InvalidCause::TypeNormalizationLimit),
    )
    .then(|| TyLowerDiag::TypeNormalizationLimit(span).into())
}

/// Type-lowering errors for a HIR type written at the position whose premises
/// are `premises`.
pub fn collect_ty_lower_errors<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: TypeId<'db>,
    span: LazyTySpan<'db>,
    premises: crate::analysis::semantic::SolverPremises<'db>,
) -> Vec<TyDiagCollection<'db>> {
    collect_ty_lower_errors_in_mode(db, hir_ty, span, premises, ConstBodyLowering::Eager)
}

fn collect_ty_lower_errors_in_mode<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: TypeId<'db>,
    span: LazyTySpan<'db>,
    premises: crate::analysis::semantic::SolverPremises<'db>,
    const_bodies: ConstBodyLowering,
) -> Vec<TyDiagCollection<'db>> {
    let mut vis = HirTyErrVisitor {
        db,
        premises,
        diags: Vec::new(),
        const_bodies,
    };
    let mut ctxt = VisitorCtxt::new(db, premises.scope(), span);
    vis.visit_ty(&mut ctxt, hir_ty);
    vis.diags
}

pub(crate) fn collect_generic_arg_errors<'db>(
    db: &'db dyn HirAnalysisDb,
    arg: &GenericArg<'db>,
    span: LazyGenericArgSpan<'db>,
    premises: crate::analysis::semantic::SolverPremises<'db>,
) -> Vec<TyDiagCollection<'db>> {
    let mut visitor = HirTyErrVisitor {
        db,
        premises,
        diags: Vec::new(),
        const_bodies: super::const_ty::ConstBodyLowering::Eager,
    };
    let mut ctxt = VisitorCtxt::new(db, premises.scope(), span);
    visitor.visit_generic_arg(&mut ctxt, arg);
    visitor.diags
}

/// Errors for the requirements of type applications in a written type, such
/// as the bounds on a family's parameters. Lowering normalizes, so a type that
/// lowers without error can still have applied a family to arguments that do
/// not meet those bounds.
///
/// `context` is the position context of the written type: its normalization
/// scope is the type's lexical scope and its premises are those in force there.
pub(crate) fn collect_application_requirement_errors<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: TypeId<'db>,
    span: LazyTySpan<'db>,
    context: crate::analysis::semantic::ImplEnv<'db>,
) -> Vec<TyDiagCollection<'db>> {
    let mut visitor = ApplicationRequirementVisitor {
        db,
        context,
        diags: Vec::new(),
    };
    let mut ctxt = VisitorCtxt::new(db, context.normalization_scope(db), span);
    visitor.visit_ty(&mut ctxt, hir_ty);
    visitor.diags
}

/// The requirement errors for the types a written path resolved to, one per
/// path segment.
fn application_requirement_diags<'db>(
    db: &'db dyn HirAnalysisDb,
    premises: crate::analysis::semantic::SolverPremises<'db>,
    path_span: &LazyPathSpan<'db>,
    type_uses: Vec<(PathId<'db>, TyId<'db>)>,
    context: Option<crate::analysis::semantic::ImplEnv<'db>>,
) -> Vec<TyDiagCollection<'db>> {
    let queries = premises.queries(db);
    type_uses
        .into_iter()
        .filter_map(|(used_path, ty)| {
            let span: DynLazySpan<'db> = path_span
                .clone()
                .segment(used_path.segment_index(db))
                .into();
            queries
                .application_requirements(db, ty, context)
                .into_diag(span.clone())
                .or_else(|| projection_const_fault(db, premises, ty, span))
        })
        .collect()
}

/// A projection whose output, once its arguments are known, contains a const
/// that faults when evaluated (`Slot<{N - 5}>` at `N = 2`). Normalization
/// reports the fault instead of keeping the unevaluated value, so the written
/// projection is where the user sees it.
fn projection_const_fault<'db>(
    db: &'db dyn HirAnalysisDb,
    premises: crate::analysis::semantic::SolverPremises<'db>,
    ty: TyId<'db>,
    span: DynLazySpan<'db>,
) -> Option<TyDiagCollection<'db>> {
    if !matches!(ty.data(db), TyData::AssocTy(_) | TyData::QualifiedTy(_)) {
        return None;
    }
    let normalized = super::normalize::normalize_ty(db, ty, premises);
    let cause = normalized.invalid_cause(db)?;
    if is_const_fault(&cause) {
        diag_from_invalid_cause(span, &cause)
    } else {
        None
    }
}

/// Whether `cause` is a fault of evaluating a const.
pub(crate) fn is_const_fault(cause: &InvalidCause<'_>) -> bool {
    matches!(
        cause,
        InvalidCause::ConstEvalUnsupported { .. }
            | InvalidCause::ConstEvalAssertionFailed { .. }
            | InvalidCause::ConstEvalNonConstCall { .. }
            | InvalidCause::ConstEvalDivisionByZero { .. }
            | InvalidCause::ConstEvalOutOfBounds { .. }
            | InvalidCause::ConstEvalInvalidOperation { .. }
            | InvalidCause::ConstEvalInvalidBorrow { .. }
            | InvalidCause::ConstEvalInvalidProviderUse { .. }
            | InvalidCause::ConstEvalVariantMismatch { .. }
            | InvalidCause::ConstEvalUninitializedLocal { .. }
            | InvalidCause::ConstEvalInvariant { .. }
            | InvalidCause::ConstEvalArithmeticOverflow { .. }
            | InvalidCause::ConstEvalNegativeExponent { .. }
            | InvalidCause::ConstEvalStepLimitExceeded { .. }
            | InvalidCause::ConstEvalRecursionLimitExceeded { .. }
            | InvalidCause::ConstEvalRecursiveConst { .. }
    )
}

/// The types a resolved path segment applies, whose requirements its use must
/// meet. An alias use applies what the alias's right-hand side applies,
/// instantiated with the use's arguments, as if written at the use.
fn push_type_uses<'db>(
    type_uses: &mut Vec<(PathId<'db>, TyId<'db>)>,
    path: PathId<'db>,
    reso: &PathRes<'db>,
) {
    match reso {
        PathRes::Ty(ty) => type_uses.push((path, *ty)),
        PathRes::TyAlias(_, ty, applied) => {
            type_uses.push((path, *ty));
            type_uses.extend(applied.iter().map(|&applied| (path, applied)));
        }
        _ => {}
    }
}

/// The types applied by the paths written in `hir_ty`, as each segment
/// resolves before normalization (alias uses contribute their own applied
/// types), keeping only the family applications whose requirements a use must
/// meet. Paths are resolved with `minter`, the lowering context of the type's
/// owner, so constants in them are the owner's own. Const expressions in the
/// type are not visited, and applications still holding a constant hole are
/// left out (their requirements are checked where the hole is filled).
pub(crate) fn written_type_uses<'db>(
    db: &'db dyn HirAnalysisDb,
    hir_ty: TypeId<'db>,
    span: LazyTySpan<'db>,
    premises: crate::analysis::semantic::SolverPremises<'db>,
    minter: &LoweringContext<'db>,
) -> Vec<TyId<'db>> {
    struct WrittenUses<'a, 'db> {
        db: &'db dyn HirAnalysisDb,
        premises: crate::analysis::semantic::SolverPremises<'db>,
        minter: &'a LoweringContext<'db>,
        uses: Vec<TyId<'db>>,
    }
    impl<'db> Visitor<'db> for WrittenUses<'_, 'db> {
        fn visit_body(
            &mut self,
            _: &mut VisitorCtxt<'db, crate::span::item::LazyBodySpan<'db>>,
            _: crate::hir_def::Body<'db>,
        ) {
        }

        fn visit_path(
            &mut self,
            ctxt: &mut VisitorCtxt<'db, LazyPathSpan<'db>>,
            path: PathId<'db>,
        ) {
            assert_eq!(ctxt.scope(), self.premises.scope());
            let mut type_uses = Vec::new();
            let mut collect =
                |path: PathId<'db>, reso: &PathRes<'db>| push_type_uses(&mut type_uses, path, reso);
            let _ = resolve_path_with_observer_and_premises(
                self.db,
                path,
                self.premises,
                false,
                &mut collect,
                self.minter,
            );
            let db = self.db;
            self.uses
                .extend(type_uses.into_iter().map(|(_, ty)| ty).filter(|&ty| {
                    let has_requirements = matches!(
                        ty.decompose_ty_app(db).0.data(db),
                        TyData::TyBase(super::ty_def::TyBase::TypeFn(_))
                    ) || ty.family_parameter_bounds(db).is_some()
                        || ty.lacks_args_for_parameter_bounds(db).is_some();
                    has_requirements && !super::layout_holes::ty_contains_const_hole(db, ty)
                }));
            walk_path(self, ctxt, path);
        }
    }
    let mut visitor = WrittenUses {
        db,
        premises,
        minter,
        uses: Vec::new(),
    };
    let mut ctxt = VisitorCtxt::new(db, premises.scope(), span);
    visitor.visit_ty(&mut ctxt, hir_ty);
    visitor.uses
}

struct ApplicationRequirementVisitor<'db> {
    db: &'db dyn HirAnalysisDb,
    context: crate::analysis::semantic::ImplEnv<'db>,
    diags: Vec<TyDiagCollection<'db>>,
}

impl<'db> Visitor<'db> for ApplicationRequirementVisitor<'db> {
    fn visit_body(
        &mut self,
        _: &mut VisitorCtxt<'db, crate::span::item::LazyBodySpan<'db>>,
        _: crate::hir_def::Body<'db>,
    ) {
        // Const expressions in types are checked as bodies of their own.
    }

    fn visit_path(&mut self, ctxt: &mut VisitorCtxt<'db, LazyPathSpan<'db>>, path: PathId<'db>) {
        let scope = ctxt.scope();
        let path_span = ctxt.span().unwrap();
        let mut type_uses = Vec::new();
        let mut collect = |path: PathId<'db>, reso: &PathRes<'db>| {
            push_type_uses(&mut type_uses, path, reso);
        };
        let context = LoweringContext::for_const_bodies(
            HoleAnchor::TemplatePath {
                path,
                scope,
                assumptions: self.context.assumptions(self.db),
            },
            ConstBodyLowering::Eager,
        )
        .with_context(self.context);
        // Resolution errors are reported by lowering; only uses that resolved
        // are checked here.
        // The visitor's root scope is the context's normalization scope, and
        // written types introduce no scopes of their own (const bodies are
        // skipped above), so these are the premises of this path's position.
        assert_eq!(scope, self.context.normalization_scope(self.db));
        let _ = resolve_path_with_observer_and_premises(
            self.db,
            path,
            self.context.premises(self.db),
            false,
            &mut collect,
            &context,
        );
        self.diags.extend(application_requirement_diags(
            self.db,
            self.context.premises(self.db),
            &path_span,
            type_uses,
            Some(self.context),
        ));
        walk_path(self, ctxt, path);
    }
}

struct HirTyErrVisitor<'db> {
    db: &'db dyn HirAnalysisDb,
    diags: Vec<TyDiagCollection<'db>>,
    /// Premises of the written type's position. Written types introduce no
    /// scopes and const bodies are skipped, so every path visited is there.
    premises: crate::analysis::semantic::SolverPremises<'db>,
    const_bodies: ConstBodyLowering,
}

impl<'db> HirTyErrVisitor<'db> {
    fn path_context(&self, path: PathId<'db>, scope: ScopeId<'db>) -> LoweringContext<'db> {
        assert_eq!(scope, self.premises.scope());
        LoweringContext::for_const_bodies(
            HoleAnchor::TemplatePath {
                path,
                scope,
                assumptions: self.premises.assumptions(),
            },
            self.const_bodies,
        )
    }

    fn push_opt_diag(&mut self, diag: Option<TyDiagCollection<'db>>) {
        if let Some(diag) = diag {
            self.diags.push(diag)
        }
    }
}

impl<'db> Visitor<'db> for HirTyErrVisitor<'db> {
    fn visit_generic_arg(
        &mut self,
        ctxt: &mut VisitorCtxt<'db, LazyGenericArgSpan<'db>>,
        arg: &GenericArg<'db>,
    ) {
        // Generic args are syntactically ambiguous: `String<N>` may parse `N` as a type
        // even when `String` expects a const generic arg. Avoid emitting spurious
        // type-expected diagnostics for const-like paths in generic-arg position.
        if let GenericArg::Type(type_arg) = arg
            && let Some(hir_ty) = type_arg.ty.to_opt()
            && let TypeKind::Path(path_partial) = hir_ty.data(self.db)
            && let Some(path) = path_partial.to_opt()
            && let Ok(resolved) = resolve_path_with_observer_and_premises(
                self.db,
                path,
                self.premises,
                true,
                &mut |_, _| {},
                &self.path_context(path, ctxt.scope()),
            )
        {
            let is_const_like = match resolved {
                PathRes::Const(..) | PathRes::TraitConst(..) | PathRes::InherentConst(..) => true,
                PathRes::Ty(ty) | PathRes::TyAlias(_, ty, _) => {
                    matches!(ty.data(self.db), TyData::ConstTy(_))
                }
                PathRes::EnumVariant(v) => v.ty.is_unit_variant_only_enum(self.db),
                _ => false,
            };

            if is_const_like {
                if let Some(span) = ctxt.span() {
                    let path_span = span.into_type_arg().ty().into_path_type().path();

                    // Preserve path visibility diagnostics even though we suppress
                    // "expected type" errors for const-like paths in generic-arg
                    // position.
                    let scope = ctxt.scope();
                    let mut invisible = None;
                    let mut check_visibility = |path: PathId<'db>, reso: &PathRes<'db>| {
                        if invisible.is_some() {
                            return;
                        }
                        if !reso.is_visible_from(self.db, scope) {
                            invisible = Some((path, reso.name_span(self.db)));
                        }
                    };

                    let resolved = resolve_path_with_observer_and_premises(
                        self.db,
                        path,
                        self.premises,
                        true,
                        &mut check_visibility,
                        &self.path_context(path, scope),
                    );
                    match resolved {
                        Ok(_) => {
                            if let Some((path, deriv_span)) = invisible
                                && let Some(ident) = path.ident(self.db).to_opt()
                            {
                                let span = path_span
                                    .clone()
                                    .segment(path.segment_index(self.db))
                                    .ident();
                                let diag = PathResDiag::Invisible(span.into(), ident, deriv_span);
                                self.diags.push(diag.into());
                            }
                        }
                        Err(err) => {
                            if let Some(diag) = err.into_diag(
                                self.db,
                                path,
                                path_span.clone(),
                                ExpectedPathKind::Value,
                            ) {
                                self.diags.push(diag.into());
                            }
                        }
                    }

                    // Walk the underlying path to validate any nested generic arguments,
                    // but don't validate the path itself as a type.
                    let mut path_ctxt = VisitorCtxt::new(ctxt.db(), ctxt.scope(), path_span);
                    walk_path(self, &mut path_ctxt, path);
                }
                return;
            }
        }

        walk_generic_arg(self, ctxt, arg);
    }

    fn visit_body(
        &mut self,
        _ctxt: &mut VisitorCtxt<'db, crate::core::span::item::LazyBodySpan<'db>>,
        _body: crate::core::hir_def::Body<'db>,
    ) {
        // Skip traversing bodies when collecting type-lowering errors from type
        // positions. This avoids emitting name-resolution diagnostics for const
        // expressions that appear inside types (e.g., array lengths or const
        // generic arguments), which are handled by dedicated const-ty lowering.
    }

    fn visit_ty(&mut self, ctxt: &mut VisitorCtxt<'db, LazyTySpan<'db>>, hir_ty: TypeId<'db>) {
        let ty = lower_hir_ty_in_mode(
            self.db,
            hir_ty,
            ctxt.scope(),
            self.premises.assumptions(),
            self.const_bodies,
        );

        // This will report errors with nested types that are fundamental to the nested type,
        // but will not catch cases where the nested type is fine on its own, but incompatible
        // with the current type we're visiting. If !did_find_child_err, we use a TyVisitor to
        // report a diag about a nested invalid type; the downside of this is that the diag's
        // span will be too wide (it'll be the span of the current type, not the nested type).
        let before = self.diags.len();
        // If the semantic type is a const type, don't traverse the underlying HIR type
        // structure as a normal type. Const expressions inside types are validated via
        // const-ty lowering/evaluation, and walking their HIR representation as a type
        // produces spurious "expected type" diagnostics for paths like `SALT`.
        if let TyData::ConstraintTerm(goal) = ty.data(self.db) {
            // A goal's first argument is its subject, unlike a trait reference.
            // Check the resolved declaration and visit the original arguments
            // without resolving the goal spelling as an ordinary type path.
            if let TypeKind::Path(path) = hir_ty.data(self.db)
                && let Some(path) = path.to_opt()
            {
                let idx = path.segment_index(self.db);
                if !crate::analysis::name_resolution::is_scope_visible_from(
                    self.db,
                    goal.def(self.db).scope(),
                    ctxt.scope(),
                ) {
                    let span = ctxt
                        .span()
                        .unwrap()
                        .into_path_type()
                        .path()
                        .segment(idx)
                        .ident();
                    self.diags.push(
                        PathResDiag::Invisible(
                            span.into(),
                            path.ident(self.db).unwrap(),
                            goal.def(self.db).scope().name_span(self.db),
                        )
                        .into(),
                    );
                }
                ctxt.with_new_ctxt(
                    |span| span.into_path_type().path().segment(idx).generic_args(),
                    |ctxt| self.visit_generic_arg_list(ctxt, path.generic_args(self.db)),
                );
            }
        } else if !ty.is_const_ty(self.db) {
            walk_type(self, ctxt, hir_ty);
        }
        let did_fild_child_err = self.diags.len() > before;

        let span = ctxt.span().unwrap().into();
        match ty.data(self.db) {
            TyData::TyApp(base, _) if matches!(base.data(self.db), TyData::Invalid(..)) => {
                let TyData::Invalid(cause) = base.data(self.db) else {
                    unreachable!()
                };
                self.push_opt_diag(diag_from_invalid_cause(span, cause));
            }
            TyData::Invalid(cause) => self.push_opt_diag(diag_from_invalid_cause(span, cause)),
            _ => {
                // The span of a diag found here will cover the current type, not the nested
                // type. For example:
                //   Foo<true>
                //   ^^^^^^^^^ expected `u32`, but `bool` is given
                // (ideally this would only underline `true`).
                //
                // We could match other TyId structures manually to refine the spans
                // for common error cases.
                if !did_fild_child_err && ty.has_invalid(self.db) {
                    self.push_opt_diag(emit_invalid_ty_error(self.db, ty, span))
                }
            }
        }
    }

    fn visit_path(&mut self, ctxt: &mut VisitorCtxt<'db, LazyPathSpan<'db>>, path: PathId<'db>) {
        let scope = ctxt.scope();
        let path_span = ctxt.span().unwrap();

        let mut invisible = None;
        let mut type_uses = Vec::new();
        let mut check_visibility = |path: PathId<'db>, reso: &PathRes<'db>| {
            push_type_uses(&mut type_uses, path, reso);
            if invisible.is_some() {
                return;
            }
            if !reso.is_visible_from(self.db, scope) {
                invisible = Some((path, reso.name_span(self.db)));
            }
        };

        let resolved = resolve_path_with_observer_and_premises(
            self.db,
            path,
            self.premises,
            false,
            &mut check_visibility,
            &self.path_context(path, scope),
        );
        let res = match resolved {
            Ok(res) => res,

            Err(err) => {
                if let Some(diag) =
                    err.into_diag(self.db, path, path_span.clone(), ExpectedPathKind::Type)
                {
                    self.diags.push(diag.into());
                }
                return;
            }
        };

        // Written types introduce no scopes and const bodies are skipped, so
        // this path is at the written type's position.
        assert_eq!(scope, self.premises.scope());
        self.diags.extend(application_requirement_diags(
            self.db,
            self.premises,
            &path_span,
            type_uses,
            None,
        ));

        if !matches!(res, PathRes::Ty(_) | PathRes::TyAlias(..)) {
            let ident = path.ident(self.db).unwrap();
            let span = path_span.clone().segment(path.segment_index(self.db));
            self.diags
                .push(PathResDiag::ExpectedType(span.into(), ident, res.kind_name()).into());
        }
        if let Some((path, deriv_span)) = invisible {
            let span = path_span.segment(path.segment_index(self.db)).ident();
            let ident = path.ident(self.db);
            let diag = PathResDiag::Invisible(span.into(), ident.unwrap(), deriv_span);
            self.diags.push(diag.into());
        }

        walk_path(self, ctxt, path);
    }

    fn visit_trait_ref(
        &mut self,
        ctxt: &mut VisitorCtxt<'db, LazyTraitRefSpan<'db>>,
        trait_ref: crate::core::hir_def::TraitRefId<'db>,
    ) {
        let scope = ctxt.scope();
        let span = ctxt.span().unwrap();
        let Some(path) = trait_ref.path(self.db).to_opt() else {
            return;
        };

        // Visibility check for trait paths mirrors visit_path but expects a Trait.
        let mut invisible = None;
        let mut check_visibility =
            |p: PathId<'db>, reso: &crate::analysis::name_resolution::PathRes<'db>| {
                if invisible.is_some() {
                    return;
                }
                if !reso.is_visible_from(self.db, scope) {
                    invisible = Some((p, reso.name_span(self.db)));
                }
            };

        // TODO(diags): In the future, refine this to walk only generic args
        // under the trait ref and surface kind/arg mismatches with precise
        // spans when available from semantic lowering.
        let resolved = resolve_path_with_observer_and_premises(
            self.db,
            path,
            self.premises,
            false,
            &mut check_visibility,
            &self.path_context(path, scope),
        );
        match resolved {
            Ok(res) => {
                if !matches!(res, crate::analysis::name_resolution::PathRes::Trait(_)) {
                    // Expected a trait in this context
                    let ident = path.ident(self.db).unwrap();
                    let seg_span = span.clone().name();
                    self.diags.push(
                        crate::analysis::name_resolution::diagnostics::PathResDiag::ExpectedTrait(
                            seg_span.into(),
                            ident,
                            res.kind_name(),
                        )
                        .into(),
                    );
                }

                if let Some((_p, deriv_span)) = invisible {
                    let seg_span = span.name();
                    let ident = path.ident(self.db).unwrap();
                    let diag =
                        crate::analysis::name_resolution::diagnostics::PathResDiag::Invisible(
                            seg_span.into(),
                            ident,
                            deriv_span,
                        );
                    self.diags.push(diag.into());
                }
            }
            Err(err) => {
                if let Some(diag) = err.into_diag(
                    self.db,
                    path,
                    span.clone().path(),
                    crate::analysis::name_resolution::ExpectedPathKind::Trait,
                ) {
                    self.diags.push(diag.into());
                }
            }
        }

        // Do not recurse into the trait path via visit_path to avoid emitting
        // type-expected diagnostics for trait-qualified segments. Generic
        // arguments under the trait path are checked elsewhere during lowering.
    }
}

pub(crate) fn first_invalid_ty_cause<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
) -> Option<InvalidCause<'db>> {
    struct InvalidCauseVisitor<'db> {
        db: &'db dyn HirAnalysisDb,
        first: Option<InvalidCause<'db>>,
        reportable: Option<InvalidCause<'db>>,
    }
    impl<'db> TyVisitor<'db> for InvalidCauseVisitor<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_invalid(&mut self, cause: &InvalidCause<'db>) {
            self.first.get_or_insert_with(|| cause.clone());
            if !matches!(
                cause,
                InvalidCause::NotAType(_)
                    | InvalidCause::ComputedTypeDefinitionInvalid
                    | InvalidCause::PathResolutionFailed { .. }
                    | InvalidCause::ParseError
                    | InvalidCause::Other
            ) {
                self.reportable.get_or_insert_with(|| cause.clone());
            }
        }
    }

    if !ty.has_invalid(db) {
        return None;
    }

    let mut visitor = InvalidCauseVisitor {
        db,
        first: None,
        reportable: None,
    };
    visitor.visit_ty(ty);
    visitor.reportable.or(visitor.first)
}

/// The first failure among deferred const terms whose captures are fully
/// ground. A concrete target demands them so a failing specialization is
/// reported where it is written rather than at a later layout demand.
pub(crate) fn demanded_ground_const_cause<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
) -> Option<InvalidCause<'db>> {
    struct GroundConstDemand<'db> {
        db: &'db dyn HirAnalysisDb,
        cause: Option<InvalidCause<'db>>,
    }
    impl<'db> TyVisitor<'db> for GroundConstDemand<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_ty(&mut self, ty: TyId<'db>) {
            if self.cause.is_some() {
                return;
            }
            if let TyData::ConstTy(const_ty) = ty.data(self.db)
                && matches!(
                    const_ty.data(self.db),
                    ConstTyData::UnEvaluated { .. } | ConstTyData::Computation { .. }
                )
                && ty_is_fully_ground(self.db, ty)
            {
                self.cause =
                    first_invalid_ty_cause(self.db, const_ty.evaluate(self.db, None).ty(self.db));
                return;
            }
            walk_ty(self, ty);
        }
    }

    let mut visitor = GroundConstDemand { db, cause: None };
    visitor.visit_ty(ty);
    visitor.cause
}

pub fn emit_invalid_ty_error<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
    span: DynLazySpan<'db>,
) -> Option<TyDiagCollection<'db>> {
    first_invalid_ty_cause(db, ty).and_then(|cause| diag_from_invalid_cause(span, &cause))
}

pub(crate) fn diag_from_invalid_cause<'db>(
    span: DynLazySpan<'db>,
    cause: &InvalidCause<'db>,
) -> Option<TyDiagCollection<'db>> {
    Some(match cause.clone() {
        InvalidCause::NotFullyApplied => TyLowerDiag::ExpectedStarKind(span).into(),

        InvalidCause::KindMismatch { expected, given } => TyLowerDiag::InvalidTypeArgKind {
            span,
            expected,
            given,
        }
        .into(),

        InvalidCause::TooManyGenericArgs { expected, given } => TyLowerDiag::TooManyGenericArgs {
            span,
            expected,
            given,
        }
        .into(),

        InvalidCause::StringTooLarge { max, given } => {
            TyLowerDiag::StringTooLarge { span, max, given }.into()
        }

        InvalidCause::InvalidConstParamTy => TyLowerDiag::InvalidConstParamTy(span).into(),

        InvalidCause::RecursiveConstParamTy => TyLowerDiag::RecursiveConstParamTy(span).into(),

        InvalidCause::ConstTyMismatch { expected, given } => TyLowerDiag::ConstTyMismatch {
            span,
            expected,
            given,
        }
        .into(),

        InvalidCause::ConstTyExpected { expected } => {
            TyLowerDiag::ConstTyExpected { span, expected }.into()
        }

        InvalidCause::NormalTypeExpected { given } => {
            TyLowerDiag::NormalTypeExpected { span, given }.into()
        }

        InvalidCause::UnboundTypeAliasParam {
            alias,
            n_given_args,
        } => TyLowerDiag::UnboundTypeAliasParam {
            span,
            alias,
            n_given_args,
        }
        .into(),

        InvalidCause::AliasCycle(cycle) => TyLowerDiag::TypeAliasCycle {
            cycle: cycle.to_vec(),
        }
        .into(),

        InvalidCause::InvalidConstTyExpr { body } => {
            TyLowerDiag::InvalidConstTyExpr(body.span().into()).into()
        }

        InvalidCause::ConstEvalUnsupported { body, expr } => {
            TyLowerDiag::ConstEvalUnsupported(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalAssertionFailed {
            body,
            expr,
            message,
        } => TyLowerDiag::ConstEvalAssertionFailed {
            span: expr.span(body).into(),
            message,
        }
        .into(),

        InvalidCause::ConstEvalNonConstCall { body, expr } => {
            TyLowerDiag::ConstEvalNonConstCall(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalDivisionByZero { body, expr } => {
            TyLowerDiag::ConstEvalDivisionByZero(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalOutOfBounds { body, expr } => {
            TyLowerDiag::ConstEvalOutOfBounds(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalInvalidOperation {
            body,
            expr,
            message,
        } => TyLowerDiag::ConstEvalInvalidOperation {
            span: expr.span(body).into(),
            message,
        }
        .into(),

        InvalidCause::ConstEvalInvalidBorrow { body, expr } => {
            TyLowerDiag::ConstEvalInvalidBorrow(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalInvalidProviderUse { body, expr } => {
            TyLowerDiag::ConstEvalInvalidProviderUse(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalVariantMismatch { body, expr } => {
            TyLowerDiag::ConstEvalVariantMismatch(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalUninitializedLocal { body, expr } => {
            TyLowerDiag::ConstEvalUninitializedLocal(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalInvariant {
            body,
            expr,
            message,
        } => TyLowerDiag::ConstEvalInvariant {
            span: expr.span(body).into(),
            message,
        }
        .into(),

        InvalidCause::ConstEvalArithmeticOverflow { body, expr } => {
            TyLowerDiag::ConstEvalArithmeticOverflow(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalNegativeExponent { body, expr } => {
            TyLowerDiag::ConstEvalNegativeExponent(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalStepLimitExceeded { body, expr } => {
            TyLowerDiag::ConstEvalStepLimitExceeded(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalRecursionLimitExceeded { body, expr } => {
            TyLowerDiag::ConstEvalRecursionLimitExceeded(expr.span(body).into()).into()
        }

        InvalidCause::ConstEvalRecursiveConst { body, expr } => {
            TyLowerDiag::ConstEvalRecursiveConst(expr.span(body).into()).into()
        }

        InvalidCause::TypeLoweringCycle => TyLowerDiag::TypeLoweringCycle(span).into(),
        InvalidCause::TypeNormalizationLimit => TyLowerDiag::TypeNormalizationLimit(span).into(),
        // The definition reports its own errors.
        InvalidCause::ComputedTypeDefinitionInvalid | InvalidCause::NotAType(_) => return None,

        // These errors should be caught and reported elsewhere
        InvalidCause::PathResolutionFailed { .. }
        | InvalidCause::ParseError
        | InvalidCause::Other => return None,
    })
}
