//! Recursive type functions: definition checks and unfolding.
//!
//! `recursive type fn F<P.., const N: usize>() -> K { match N { .. } }` is a
//! family of types. A full application with a ground subject is the arm its
//! subject selects, with the arguments substituted and the arm's const
//! payloads evaluated; a full application with a symbolic subject is an
//! opaque type equal only to itself.
//!
//! Termination and containment:
//! - Definition checking evaluates nothing. It checks the declaration's shape,
//!   that every recursive call forwards the other parameters unchanged and
//!   passes a subject the arm's patterns prove smaller (`N - k` with no
//!   underflow, `N / k` away from 0, or a smaller literal), that no other type
//!   function is applied, and that const payloads are literals, parameters,
//!   such steps, or calls of a `const fn` named without generic arguments
//!   over these. So unfolding a ground application descends on a natural
//!   number and stops.
//! - Unfolding runs in the normalizer, which charges each unfolding to the
//!   same nesting and work limits as projections, refuses an application that
//!   is already being unfolded (a projection in an arm can lead back to it),
//!   and does not reuse an unfolding inside one normalization, so a result too
//!   large to write out reaches the work limit.
//! - Payloads run on the ordinary compile-time evaluator with its fuel. A
//!   payload call passes only values, so the computed types its evaluation
//!   can need are fixed by the program text rather than by the subject.
//! - If evaluation needs the application being unfolded, directly or through
//!   other applications, the step query below is re-entered. The query's
//!   cycle handler runs no queries: the cycle starts from the named cycle
//!   error and iterates. Evaluation cannot recover from an invalid type, so
//!   every application on the cycle fails in the first iteration and keeps
//!   failing, and the error a payload reports for an invalid input depends
//!   only on the kind of that input's error; the values therefore stop
//!   changing after a few iterations. A size taken of a type on the cycle
//!   reports the evaluator's own "depends on itself" error, which unfolding
//!   reports as the cycle, so through `size_of` every application on the
//!   cycle reports the same named error whichever is demanded first.

use num_bigint::BigUint;
use rustc_hash::FxHashMap;

use super::{
    binder::Binder,
    const_ty::{
        ConstTyData, ConstTyId, evaluate_type_level_const_ty, validate_unevaluated_const_ty,
    },
    diagnostics::{ComputedTypeError, RevealedBy, TyDiagCollection, TyLowerDiag},
    fold::{TyFoldable, TyFolder},
    trait_resolution::{PredicateListId, TraitSolveCx},
    ty_def::{InvalidCause, PrimTy, TyBase, TyData, TyId},
    ty_error::{collect_hir_ty_diags_deferred, diag_from_invalid_cause},
    ty_lower::{collect_generic_params, lower_hir_ty_deferred, lower_kind},
    visitor::{TyVisitable, TyVisitor, walk_const_ty, walk_ty},
};
use crate::{
    analysis::{
        HirAnalysisDb,
        analysis_pass::ModuleAnalysisPass,
        diagnostics::DiagnosticVoucher,
        name_resolution::{NameDomain, resolve_ident_to_bucket},
    },
    core::{
        semantic::{constraints_for, param_env},
        visitor::{Visitor, VisitorCtxt, walk_path},
    },
    hir_def::{
        ArithBinOp, BinOp, Body, Expr, GenericArg, GenericParam, GenericParamOwner, ImplTrait,
        ItemKind, LitKind, Partial, PathId, Stmt, TopLevelMod, TypeFnDef, TypeFnPat, TypeId,
        TypeKind, WhereClauseOwner, scope_graph::ScopeId,
    },
    semantic::diagnostics::Diagnosable,
    span::{DynLazySpan, params::LazyGenericArgSpan, path::LazyPathSpan},
};

pub(crate) struct TypeFnAnalysisPass;

impl ModuleAnalysisPass for TypeFnAnalysisPass {
    fn run_on_module<'db>(
        &mut self,
        db: &'db dyn HirAnalysisDb,
        top_mod: TopLevelMod<'db>,
    ) -> Vec<Box<dyn DiagnosticVoucher + 'db>> {
        top_mod
            .all_items(db)
            .iter()
            .filter_map(|item| match item {
                ItemKind::TypeFn(def) if item.top_mod(db) == top_mod => Some(*def),
                _ => None,
            })
            .flat_map(|def| check_definition(db, def).diags.clone())
            .map(|diag| diag.to_voucher())
            .collect()
    }
}

/// Whether `scope` is inside a recursive type fn, where types are templates
/// of its arms and are not unfolded.
pub(crate) fn in_definition<'db>(db: &'db dyn HirAnalysisDb, mut scope: ScopeId<'db>) -> bool {
    loop {
        if matches!(scope.item(), ItemKind::TypeFn(_)) {
            return true;
        }
        let Some(parent) = scope.parent(db) else {
            return false;
        };
        scope = parent;
    }
}

fn param_count<'db>(db: &'db dyn HirAnalysisDb, def: TypeFnDef<'db>) -> usize {
    collect_generic_params(db, def.into()).params(db).len()
}

/// The definition and the arguments of `ty` if it applies a recursive type
/// fn to at least all of its parameters.
pub(crate) fn as_computed_application<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
) -> Option<(TypeFnDef<'db>, &'db [TyId<'db>])> {
    let (base, args) = ty.decompose_ty_app(db);
    match base.data(db) {
        TyData::TyBase(TyBase::TypeFn(def)) if args.len() >= param_count(db, *def) => {
            Some((*def, args))
        }
        _ => None,
    }
}

/// Whether `value` mentions a recursive type fn.
pub(crate) fn contains_computed_application<'db, V>(db: &'db dyn HirAnalysisDb, value: V) -> bool
where
    V: TyVisitable<'db>,
{
    struct Finder<'db> {
        db: &'db dyn HirAnalysisDb,
        found: bool,
    }
    impl<'db> TyVisitor<'db> for Finder<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_type_fn(&mut self, _: TypeFnDef<'db>) {
            self.found = true;
        }
    }
    let mut finder = Finder { db, found: false };
    value.visit_with(&mut finder);
    finder.found
}

/// The error reported where a type is written when unfolding a computed type
/// in it failed with `cause`.
pub(crate) fn computed_type_failure_diag<'db>(
    span: DynLazySpan<'db>,
    cause: &InvalidCause<'db>,
) -> Option<TyDiagCollection<'db>> {
    match cause {
        InvalidCause::ComputedTypeDefinitionInvalid => None,
        InvalidCause::ComputedTypeCycle => Some(TyLowerDiag::ComputedTypeCycle(span).into()),
        InvalidCause::TypeNormalizationLimit => {
            Some(TyLowerDiag::TypeNormalizationLimit(span).into())
        }
        cause => {
            let fault = diag_from_invalid_cause(span.clone(), cause)?;
            Some(
                TyLowerDiag::RevealedConstFault {
                    site: span,
                    revealed_by: RevealedBy::ComputedType,
                    fault: Box::new(fault),
                }
                .into(),
            )
        }
    }
}

/// An implementation is identified by its header, and a computed application
/// is the same type as what it unfolds to, so a header naming one would give
/// one type two spellings with separate implementations. Report a computed
/// application anywhere in a trait implementation's header, aliases included.
pub(crate) fn impl_header_diagnostic<'db>(
    db: &'db dyn HirAnalysisDb,
    implementation: ImplTrait<'db>,
) -> Option<TyDiagCollection<'db>> {
    let header = implementation.candidate_trait_inst_result(db).ok()?;
    contains_computed_application(db, header)
        .then(|| TyLowerDiag::ComputedTypeImplHeader(implementation.span().ty().into()).into())
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub(crate) struct DefinitionCheck<'db> {
    /// The arms' types in the definition's own parameters, present only if the
    /// definition has no errors.
    pub(crate) arms: Option<Vec<TyId<'db>>>,
    pub(crate) diags: Vec<TyDiagCollection<'db>>,
}

/// Every check of a declaration. Unfolding uses the arms only if all pass.
#[salsa::tracked]
pub(crate) fn check_definition<'db>(
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
) -> DefinitionCheck<'db> {
    let shape = declaration_shape(db, def);
    let recurrence = check_recurrences(db, def);
    let mut diags = shape.diags.clone();
    diags.extend(recurrence.diags.iter().cloned());
    diags.extend(GenericParamOwner::TypeFn(def).diags(db));
    for predicate in WhereClauseOwner::TypeFn(def).clause(db).predicates(db) {
        diags.extend(predicate.diags(db));
    }
    let (Some(_), Some(return_kind)) = (&shape.lower_bounds, def.ret_kind(db).borrowed().to_opt())
    else {
        return DefinitionCheck { arms: None, diags };
    };
    if !diags.is_empty() {
        return DefinitionCheck { arms: None, diags };
    }

    let assumptions = constraints_for(db, def.into());
    let result_kind = lower_kind(return_kind);
    let solve_cx = TraitSolveCx::new(db, def.scope());
    let wf_assumptions = param_env(db, def.into());
    let mut arms = Vec::new();
    let mut lowered_calls = 0;
    for (idx, arm) in def.arms(db).iter().enumerate() {
        let span = def.span().body().match_().arms().arm(idx).ty();
        let hir_ty = arm.ty.to_opt().expect("checked arm shape");
        // Arms are templates: lowering them must not evaluate a payload,
        // which could need this definition again.
        let source_diags =
            collect_hir_ty_diags_deferred(db, def.scope(), hir_ty, span.clone(), assumptions);
        if !source_diags.is_empty() {
            diags.extend(source_diags);
            continue;
        }
        let ty = lower_hir_ty_deferred(db, hir_ty, def.scope(), assumptions);
        if let Some(diag) = ty.emit_diag(db, span.clone().into()) {
            diags.push(diag);
            continue;
        }
        let report = |error| -> TyDiagCollection<'db> {
            TyLowerDiag::ComputedTypeDeclaration {
                span: span.clone().into(),
                error,
            }
            .into()
        };
        if !result_kind.does_match(ty.kind(db)) || ty.is_const_ty(db) {
            diags.push(report(ComputedTypeError::ArmKind));
            continue;
        }

        let mut check = ArmTypeCheck {
            db,
            def,
            calls: FxHashMap::default(),
            span: span.clone().into(),
            diags: Vec::new(),
        };
        ty.visit_with(&mut check);
        lowered_calls += check.calls.values().sum::<usize>();
        let mut written_calls = FxHashMap::default();
        for path in &recurrence.calls_per_arm[idx] {
            let call = lower_hir_ty_deferred(
                db,
                TypeId::new(db, TypeKind::Path(Partial::Present(*path))),
                def.scope(),
                assumptions,
            );
            *written_calls.entry(call).or_insert(0usize) += 1;
        }
        if check.diags.is_empty() && check.calls != written_calls {
            check
                .diags
                .push(report(ComputedTypeError::HiddenRecursiveCall));
        }
        if check.diags.is_empty()
            && let Some(diag) = ty.emit_wf_diag(db, solve_cx, wf_assumptions, span.into())
        {
            check.diags.push(diag);
        }
        diags.extend(check.diags);
        arms.push(ty);
    }
    // Counted after lowering, so a call made through an alias is reported as
    // hidden rather than missing.
    if diags.is_empty() && lowered_calls == 0 {
        diags.push(
            TyLowerDiag::ComputedTypeDeclaration {
                span: def.span().into(),
                error: ComputedTypeError::MissingSelfCall,
            }
            .into(),
        );
    }
    DefinitionCheck {
        arms: diags.is_empty().then_some(arms),
        diags,
    }
}

/// Compares the recursive calls in a lowered arm with the calls written in
/// it, so an alias cannot hide a call the recursion check did not see.
struct ArmTypeCheck<'db> {
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
    calls: FxHashMap<TyId<'db>, usize>,
    span: DynLazySpan<'db>,
    diags: Vec<TyDiagCollection<'db>>,
}

impl<'db> ArmTypeCheck<'db> {
    fn report(&mut self, error: ComputedTypeError) {
        let diag = TyLowerDiag::ComputedTypeDeclaration {
            span: self.span.clone(),
            error,
        }
        .into();
        if !self.diags.contains(&diag) {
            self.diags.push(diag);
        }
    }
}

impl<'db> TyVisitor<'db> for ArmTypeCheck<'db> {
    fn db(&self) -> &'db dyn HirAnalysisDb {
        self.db
    }

    fn visit_ty(&mut self, ty: TyId<'db>) {
        let (base, args) = ty.decompose_ty_app(self.db);
        let TyData::TyBase(TyBase::TypeFn(def)) = base.data(self.db) else {
            walk_ty(self, ty);
            return;
        };
        let params = param_count(self.db, *def);
        if *def != self.def {
            self.report(ComputedTypeError::OtherTypeFnIndirect);
        } else if args.len() < params {
            self.report(ComputedTypeError::NotFullyApplied);
        } else {
            let call = TyId::foldl(self.db, base, &args[..params]);
            *self.calls.entry(call).or_default() += 1;
        }
        for arg in args {
            self.visit_ty(*arg);
        }
    }

    fn visit_const_ty(&mut self, value: &ConstTyId<'db>) {
        if matches!(value.data(self.db), ConstTyData::UnEvaluated { .. })
            && let Err(cause) = validate_unevaluated_const_ty(self.db, *value, None)
            && let Some(diag) = TyId::invalid(self.db, cause).emit_diag(self.db, self.span.clone())
        {
            self.diags.push(diag);
        }
        walk_const_ty(self, value);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub(crate) struct DeclarationShape<'db> {
    /// For each arm, the smallest subject it can see: its literal, or for the
    /// final wildcard the smallest value no literal arm matches. Present only
    /// for a complete declaration with a final wildcard.
    pub(crate) lower_bounds: Option<Vec<BigUint>>,
    pub(crate) diags: Vec<TyDiagCollection<'db>>,
}

#[salsa::tracked]
pub(crate) fn declaration_shape<'db>(
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
) -> DeclarationShape<'db> {
    let mut diags = Vec::new();
    // The parser reports missing syntax; the declaration only becomes unusable.
    let mut complete = def.ret_kind(db).is_present();
    let mut report = |span: DynLazySpan<'db>, error| {
        diags.push(TyLowerDiag::ComputedTypeDeclaration { span, error }.into())
    };

    let params = def.generic_params(db).data(db);
    match params.last() {
        Some(GenericParam::Const(subject)) => {
            let usize_ty = TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::Usize)));
            let subject_ty = subject.ty.to_opt().map(|ty| {
                lower_hir_ty_deferred(db, ty, def.scope(), PredicateListId::empty_list(db))
            });
            if subject_ty.is_some_and(|ty| ty != usize_ty) {
                report(
                    def.span().generic_params().into(),
                    ComputedTypeError::SubjectNotUsize,
                );
            }
            if subject.name.to_opt().is_some()
                && def.match_subject(db).to_opt().is_some()
                && subject.name.to_opt() != def.match_subject(db).to_opt()
            {
                report(
                    def.span().body().match_().subject().into(),
                    ComputedTypeError::MatchNotOnSubject,
                );
            }
            complete &= subject.name.to_opt().is_some() && subject_ty.is_some();
        }
        _ => report(
            def.span().generic_params().into(),
            ComputedTypeError::MissingSubject,
        ),
    }

    let clause = def.where_clause(db);
    if !clause.const_predicates(db).is_empty() {
        report(
            def.span().where_clause().into(),
            ComputedTypeError::ConstCondition,
        );
    }
    for (_, predicate) in clause.type_predicates(db) {
        let on_parameter = predicate.ty.to_opt().is_some_and(|ty| {
            let TypeKind::Path(Partial::Present(path)) = ty.data(db) else {
                return false;
            };
            path.as_ident(db).is_some_and(|name| {
                params.iter().any(|param| {
                    matches!(param, GenericParam::Type(param) if param.name.to_opt() == Some(name))
                })
            })
        });
        if !on_parameter {
            report(
                def.span().where_clause().into(),
                ComputedTypeError::BoundOnNonParameter,
            );
        }
    }

    let arms = def.arms(db);
    let mut literals = Vec::new();
    for (idx, arm) in arms.iter().enumerate() {
        let span = def.span().body().match_().arms().arm(idx);
        match arm.pat.to_opt() {
            Some(TypeFnPat::Lit(value)) => {
                if literals.contains(value.data(db)) {
                    report(span.into(), ComputedTypeError::DuplicateLiteral);
                } else {
                    literals.push(value.data(db).clone());
                }
            }
            Some(TypeFnPat::Wild) if idx + 1 != arms.len() => {
                report(span.into(), ComputedTypeError::WildcardNotLast)
            }
            Some(TypeFnPat::Wild) => {}
            None => complete = false,
        }
        complete &= arm.ty.to_opt().is_some();
    }
    let ends_in_wildcard = matches!(
        arms.last().and_then(|arm| arm.pat.to_opt()),
        Some(TypeFnPat::Wild)
    );
    let has_wildcard = arms
        .iter()
        .any(|arm| matches!(arm.pat.to_opt(), Some(TypeFnPat::Wild)));
    if !has_wildcard && def.match_subject(db).to_opt().is_some() {
        report(
            def.span().body().match_().arms().into(),
            ComputedTypeError::MissingWildcard,
        );
    }

    let lower_bounds = (diags.is_empty() && complete && ends_in_wildcard).then(|| {
        let mut first_unmatched = BigUint::from(0u32);
        while literals.contains(&first_unmatched) {
            first_unmatched += 1u32;
        }
        arms.iter()
            .map(|arm| match arm.pat.to_opt().expect("complete arm") {
                TypeFnPat::Lit(value) => value.data(db).clone(),
                TypeFnPat::Wild => first_unmatched.clone(),
            })
            .collect()
    });
    DeclarationShape {
        lower_bounds,
        diags,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::SalsaValue)]
pub(crate) struct RecurrenceCheck<'db> {
    /// The recursive calls written in each arm.
    pub(crate) calls_per_arm: Vec<Vec<PathId<'db>>>,
    pub(crate) diags: Vec<TyDiagCollection<'db>>,
}

/// Checks recursive calls and const payloads as written. No application is
/// unfolded or evaluated to establish that a subject decreases.
#[salsa::tracked]
pub(crate) fn check_recurrences<'db>(
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
) -> RecurrenceCheck<'db> {
    let Some(bounds) = &declaration_shape(db, def).lower_bounds else {
        return RecurrenceCheck {
            calls_per_arm: Vec::new(),
            diags: Vec::new(),
        };
    };
    let mut checker = RecurrenceChecker {
        db,
        def,
        lower_bound: BigUint::from(0u32),
        calls: Vec::new(),
        call_args: Vec::new(),
        diags: Vec::new(),
    };
    let mut calls_per_arm = Vec::new();
    for (idx, (arm, lower_bound)) in def.arms(db).iter().zip(bounds).enumerate() {
        let before = checker.calls.len();
        checker.lower_bound.clone_from(lower_bound);
        let ty = arm.ty.to_opt().expect("complete arm");
        let mut ctxt = VisitorCtxt::new(
            db,
            def.scope(),
            def.span().body().match_().arms().arm(idx).ty(),
        );
        checker.visit_ty(&mut ctxt, ty);
        calls_per_arm.push(checker.calls[before..].to_vec());
    }
    RecurrenceCheck {
        calls_per_arm,
        diags: checker.diags,
    }
}

struct RecurrenceChecker<'db> {
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
    /// The smallest subject the current arm can see.
    lower_bound: BigUint,
    calls: Vec<PathId<'db>>,
    /// Const arguments of recursive calls, which `check_call` judges.
    call_args: Vec<Body<'db>>,
    diags: Vec<TyDiagCollection<'db>>,
}

impl<'db> RecurrenceChecker<'db> {
    fn report(&mut self, span: DynLazySpan<'db>, error: ComputedTypeError) {
        self.diags
            .push(TyLowerDiag::ComputedTypeDeclaration { span, error }.into());
    }

    fn subject(&self) -> Option<crate::hir_def::IdentId<'db>> {
        let params = self.def.generic_params(self.db).data(self.db);
        params.last().and_then(|param| param.name().to_opt())
    }

    fn check_call(&self, path: PathId<'db>) -> Result<(), ComputedTypeError> {
        let db = self.db;
        let params = self.def.generic_params(db).data(db);
        let args = path.generic_args(db).data(db);
        if args.len() != params.len() {
            return Err(ComputedTypeError::CallArity);
        }
        for (arg, param) in args[..args.len() - 1].iter().zip(params) {
            let name = match arg {
                // `T` and `O` both parse as type arguments.
                GenericArg::Type(arg) => arg.ty.to_opt().and_then(|ty| match ty.data(db) {
                    TypeKind::Path(Partial::Present(path)) => path.as_ident(db),
                    _ => None,
                }),
                GenericArg::Const(arg) if matches!(param, GenericParam::Const(_)) => arg
                    .value
                    .to_opt()
                    .and_then(|body| match body_root(db, body) {
                        Some(Expr::Path(Partial::Present(path))) => path.as_ident(db),
                        _ => None,
                    }),
                _ => None,
            };
            if name.is_none() || name != param.name().to_opt() {
                return Err(ComputedTypeError::NotForwarded);
            }
        }
        match args.last() {
            Some(GenericArg::Const(arg)) => {
                let body = arg.value.to_opt().ok_or(ComputedTypeError::NotDecreasing)?;
                self.check_step(body, body_root(db, body).as_ref())
            }
            // A literal subject written without braces parses as a const
            // argument; anything else here is a type.
            _ => Err(ComputedTypeError::NotDecreasing),
        }
    }

    /// Whether `expr` is a subject strictly below the current arm's subject.
    fn check_step(
        &self,
        body: Body<'db>,
        expr: Option<&Expr<'db>>,
    ) -> Result<(), ComputedTypeError> {
        let db = self.db;
        match expr {
            Some(Expr::Lit(LitKind::Int(value))) if value.data(db) < &self.lower_bound => Ok(()),
            Some(Expr::Bin(lhs, rhs, BinOp::Arith(op))) => {
                let lhs = lhs.data(db, body).borrowed().to_opt();
                let rhs = rhs.data(db, body).borrowed().to_opt();
                let Some(Expr::Path(Partial::Present(path))) = lhs else {
                    return Err(ComputedTypeError::NotFromSubject);
                };
                if path.as_ident(db).is_none() || path.as_ident(db) != self.subject() {
                    return Err(ComputedTypeError::NotFromSubject);
                }
                let Some(Expr::Lit(LitKind::Int(amount))) = rhs else {
                    return Err(ComputedTypeError::StepNotLiteral);
                };
                let amount = amount.data(db);
                let zero = BigUint::from(0u32);
                match op {
                    ArithBinOp::Sub if amount > &zero => {
                        if amount <= &self.lower_bound {
                            Ok(())
                        } else {
                            Err(ComputedTypeError::MayUnderflow)
                        }
                    }
                    ArithBinOp::Div if amount >= &BigUint::from(2u32) => {
                        if self.lower_bound > zero {
                            Ok(())
                        } else {
                            Err(ComputedTypeError::DivisionAtZero)
                        }
                    }
                    _ => Err(ComputedTypeError::NotDecreasing),
                }
            }
            _ => Err(ComputedTypeError::NotDecreasing),
        }
    }

    /// A literal, a const parameter, or a safe step of the subject.
    fn is_payload_input(&self, body: Body<'db>, expr: &Expr<'db>) -> bool {
        match expr {
            Expr::Lit(LitKind::Int(_)) => true,
            Expr::Path(Partial::Present(path)) => path.as_ident(self.db).is_some_and(|name| {
                self.def
                    .generic_params(self.db)
                    .data(self.db)
                    .iter()
                    .any(|param| {
                        matches!(param, GenericParam::Const(param) if param.name.to_opt() == Some(name))
                    })
            }),
            Expr::Bin(..) => self.check_step(body, Some(expr)).is_ok(),
            _ => false,
        }
    }

    /// A payload input, or a call of a `const fn` named without generic
    /// arguments whose arguments are payload inputs. Passing only values
    /// keeps the types a payload's evaluation can reach fixed by the program
    /// text.
    fn is_payload(&self, body: Body<'db>) -> bool {
        let db = self.db;
        let Some(expr) = body_root(db, body) else {
            return false;
        };
        if self.is_payload_input(body, &expr) {
            return true;
        }
        let Expr::Call(callee, args) = expr else {
            return false;
        };
        let Some(Expr::Path(Partial::Present(path))) = callee.data(db, body).borrowed().to_opt()
        else {
            return false;
        };
        if path.as_ident(db).is_none() {
            return false;
        }
        let is_const_fn = resolve_ident_to_bucket(db, *path, self.def.scope())
            .pick(NameDomain::VALUE)
            .as_ref()
            .ok()
            .and_then(|res| res.scope())
            .is_some_and(
                |scope| matches!(scope, ScopeId::Item(ItemKind::Func(func)) if func.is_const(db)),
            );
        is_const_fn
            && args.iter().all(|arg| {
                arg.expr
                    .data(db, body)
                    .borrowed()
                    .to_opt()
                    .is_some_and(|expr| self.is_payload_input(body, expr))
            })
    }
}

impl<'db> Visitor<'db> for RecurrenceChecker<'db> {
    fn visit_generic_arg(
        &mut self,
        ctxt: &mut VisitorCtxt<'db, LazyGenericArgSpan<'db>>,
        arg: &GenericArg<'db>,
    ) {
        if let GenericArg::Const(arg) = arg {
            let admitted = arg
                .value
                .to_opt()
                .is_some_and(|body| self.call_args.contains(&body) || self.is_payload(body));
            if !admitted {
                self.report(
                    ctxt.span().expect("generic argument span").into(),
                    ComputedTypeError::PayloadNotAdmitted,
                );
            }
            // A payload's body is a value computation: a type fn it names is
            // evaluated, not a recursive call of the type.
            return;
        }
        crate::core::visitor::walk_generic_arg(self, ctxt, arg);
    }

    fn visit_path(&mut self, ctxt: &mut VisitorCtxt<'db, LazyPathSpan<'db>>, path: PathId<'db>) {
        // Qualified parents and their arguments are walked as well, so a call
        // inside `<Wrapper<Tree<..>> as Trait>::Out` is checked.
        if path.parent(self.db).is_none()
            && let Ok(resolved) =
                resolve_ident_to_bucket(self.db, path, self.def.scope()).pick(NameDomain::TYPE)
            && let Some(ScopeId::Item(ItemKind::TypeFn(def))) = resolved.scope()
        {
            let span: DynLazySpan<'db> = ctxt.span().expect("path span").into();
            if def == self.def {
                self.calls.push(path);
                for arg in path.generic_args(self.db).data(self.db) {
                    if let GenericArg::Const(arg) = arg
                        && let Some(body) = arg.value.to_opt()
                    {
                        self.call_args.push(body);
                    }
                }
                if let Err(error) = self.check_call(path) {
                    self.report(span, error);
                }
            } else {
                self.report(span, ComputedTypeError::OtherTypeFn);
            }
        }
        walk_path(self, ctxt, path);
    }
}

/// The expression of a const body, looking through one level of braces.
fn body_root<'db>(db: &'db dyn HirAnalysisDb, body: Body<'db>) -> Option<Expr<'db>> {
    let root = body.expr(db).data(db, body).clone().to_opt()?;
    if let Expr::Block(stmts, _) = &root
        && let [stmt] = stmts.as_slice()
        && let Partial::Present(Stmt::Expr(inner)) = stmt.data(db, body)
    {
        return inner.data(db, body).clone().to_opt();
    }
    Some(root)
}

/// One unfolding of a full application with ground arguments: `None` if its
/// subject is not yet a value; otherwise the selected arm with the arguments
/// substituted and its payloads evaluated, or an invalid type naming why it
/// cannot be computed. Nested applications in the result are not unfolded.
#[salsa::tracked(cycle_initial = unfold_application_cycle_initial)]
pub(crate) fn unfold_application<'db>(
    db: &'db dyn HirAnalysisDb,
    application: TyId<'db>,
) -> Option<TyId<'db>> {
    let (def, args) = as_computed_application(db, application)?;
    let params = param_count(db, def);
    if args.len() != params {
        return None;
    }
    let TyData::ConstTy(subject) = args[params - 1].data(db) else {
        return None;
    };
    let usize_ty = TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::Usize)));
    let subject = evaluate_type_level_const_ty(db, *subject, Some(usize_ty));
    if let ConstTyData::Invalid(fault) = subject.data(db) {
        return Some(TyId::invalid(
            db,
            fault.invalid_cause(db).unwrap_or(InvalidCause::Other),
        ));
    }
    let value = subject.integer_value(db)?.to_biguint()?;

    let Some(arms) = &check_definition(db, def).arms else {
        return Some(TyId::invalid(
            db,
            InvalidCause::ComputedTypeDefinitionInvalid,
        ));
    };
    let selected = def
        .arms(db)
        .iter()
        .position(|arm| match arm.pat.to_opt() {
            Some(TypeFnPat::Lit(pattern)) => pattern.data(db) == &value,
            Some(TypeFnPat::Wild) => true,
            None => false,
        })
        .expect("a checked match ends in a wildcard");

    let mut args = args.to_vec();
    args[params - 1] = TyId::const_ty(db, subject);
    let result = Binder::bind(def.into(), arms[selected]).instantiate(db, &args);
    let mut payloads = PayloadEvaluator { fault: None };
    let result = result.fold_with(db, &mut payloads);
    Some(match payloads.fault {
        // A payload that needed its own value, directly or through another
        // computed type, is the same cycle as reaching this application again.
        Some(InvalidCause::ConstEvalRecursiveConst { .. }) => {
            TyId::invalid(db, InvalidCause::ComputedTypeCycle)
        }
        Some(cause) => TyId::invalid(db, cause),
        None => result,
    })
}

fn unfold_application_cycle_initial<'db>(
    db: &'db dyn HirAnalysisDb,
    _: salsa::Id,
    _application: TyId<'db>,
) -> Option<TyId<'db>> {
    Some(TyId::invalid(db, InvalidCause::ComputedTypeCycle))
}

/// Evaluates the ground const arguments of an unfolded arm, recording the
/// first that faults.
struct PayloadEvaluator<'db> {
    fault: Option<InvalidCause<'db>>,
}

impl<'db> TyFolder<'db> for PayloadEvaluator<'db> {
    fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        if self.fault.is_some() {
            return ty;
        }
        if let TyData::ConstTy(value) = ty.data(db)
            && !ty.has_param(db)
            && !ty.has_var(db)
            && matches!(
                value.data(db),
                ConstTyData::UnEvaluated { .. }
                    | ConstTyData::Abstract(..)
                    | ConstTyData::Computation { .. }
            )
        {
            let evaluated = evaluate_type_level_const_ty(db, *value, None);
            if let ConstTyData::Invalid(fault) = evaluated.data(db) {
                self.fault = Some(fault.invalid_cause(db).unwrap_or(InvalidCause::Other));
                return ty;
            }
            return TyId::const_ty(db, evaluated);
        }
        ty.super_fold_with(db, self)
    }
}
