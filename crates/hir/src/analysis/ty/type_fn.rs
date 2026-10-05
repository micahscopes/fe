//! Declaration checks for pure computed structural types.
//!
//! Match partitioning establishes the lower bound used to prove that recursive
//! calls decrease. It does not evaluate arms or constitute permission to unfold
//! them: body checking and termination checking must also succeed.

use num_bigint::BigUint;
use rustc_hash::{FxHashMap, FxHashSet};
use salsa::plumbing::AsId;
use std::cell::RefCell;

use crate::{
    analysis::{
        HirAnalysisDb,
        analysis_pass::ModuleAnalysisPass,
        diagnostics::DiagnosticVoucher,
        name_resolution::{NameDomain, resolve_ident_to_bucket},
    },
    core::visitor::{Visitor, VisitorCtxt, walk_generic_arg, walk_path},
    hir_def::{
        ArithBinOp, BinOp, Body, ConstGenericArgValue, Expr, GenericArg, GenericParam,
        GenericParamOwner, ItemKind, KindBound, LitKind, Partial, PathId, Stmt, TopLevelMod,
        TypeFnDef, TypeFnPat, TypeId, TypeKind, WhereClauseOwner, scope_graph::ScopeId,
    },
    semantic::diagnostics::Diagnosable,
    span::DynLazySpan,
};

use super::{
    const_ty::{ConstTyData, ConstTyId, validate_unevaluated_const_ty},
    diagnostics::{TyDiagCollection, TyLowerDiag},
    trait_resolution::{
        PredicateListId, TraitSolveCx, check_ty_wf, constraint::collect_constraints,
    },
    ty_def::{Kind, PrimTy, TyBase, TyData, TyId},
    ty_lower::{lower_hir_ty_deferred, lower_kind},
    visitor::{TyVisitable, TyVisitor, walk_const_ty},
};

pub(super) mod induction;

pub(super) fn in_definition<'db>(db: &'db dyn HirAnalysisDb, mut scope: ScopeId<'db>) -> bool {
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
                ItemKind::TypeFn(def) => Some(*def),
                _ => None,
            })
            .flat_map(|def| check_definition_body(db, def).diags.clone())
            .map(|diag| diag.to_voucher())
            .collect()
    }
}

/// Header indexing must not treat a computed application as a nominal head
/// while later dispatch sees its expanded result. Inspect the deferred header
/// so aliases and concrete subjects cannot erase the application first.
pub(crate) fn impl_header_diagnostic<'db>(
    db: &'db dyn HirAnalysisDb,
    implementation: crate::hir_def::ImplTrait<'db>,
) -> Option<TyDiagCollection<'db>> {
    struct ComputedHead<'db> {
        db: &'db dyn HirAnalysisDb,
        found: bool,
    }
    impl<'db> TyVisitor<'db> for ComputedHead<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_type_fn(&mut self, _: TypeFnDef<'db>) {
            self.found = true;
        }
    }
    let header = implementation.candidate_trait_inst_result(db).ok()?;
    let mut visitor = ComputedHead { db, found: false };
    header.visit_with(&mut visitor);
    visitor
        .found
        .then(|| TyLowerDiag::ComputedTypeImplHeader(implementation.span().into()).into())
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub(super) struct DefinitionBodyCheck<'db> {
    /// Validated templates, still in the definition's parameter scope.
    /// Pure payload admission and the reduction boundary are checked separately.
    pub(super) arms: Option<Vec<TyId<'db>>>,
    pub(super) diags: Vec<TyDiagCollection<'db>>,
}

type DefinitionCheckKey = (usize, salsa::Id);

// A const payload can ask for the size of the computed type being validated.
// Catch that re-entry before it starts another normalization and CTFE query.
// The guard is local to one query thread and one database and definition.
thread_local! {
    static ACTIVE_DEFINITION_CHECKS: RefCell<Vec<(DefinitionCheckKey, bool)>> =
        const { RefCell::new(Vec::new()) };
}

fn definition_check_key(db: &dyn HirAnalysisDb, def: TypeFnDef<'_>) -> DefinitionCheckKey {
    (
        db as *const dyn HirAnalysisDb as *const () as usize,
        def.as_id(),
    )
}

struct DefinitionCheckGuard(DefinitionCheckKey);

impl DefinitionCheckGuard {
    fn enter(db: &dyn HirAnalysisDb, def: TypeFnDef<'_>) -> Self {
        let key = definition_check_key(db, def);
        ACTIVE_DEFINITION_CHECKS.with(|checks| checks.borrow_mut().push((key, false)));
        Self(key)
    }

    fn was_reentered(&self) -> bool {
        ACTIVE_DEFINITION_CHECKS.with(|checks| {
            checks
                .borrow()
                .iter()
                .rev()
                .find(|(key, _)| *key == self.0)
                .is_some_and(|(_, reentered)| *reentered)
        })
    }
}

impl Drop for DefinitionCheckGuard {
    fn drop(&mut self) {
        ACTIVE_DEFINITION_CHECKS.with(|checks| {
            let popped = checks.borrow_mut().pop();
            debug_assert_eq!(popped.map(|(key, _)| key), Some(self.0));
        });
    }
}

pub(super) fn definition_check_is_active(db: &dyn HirAnalysisDb, def: TypeFnDef<'_>) -> bool {
    let key = definition_check_key(db, def);
    ACTIVE_DEFINITION_CHECKS.with(|checks| {
        let mut checks = checks.borrow_mut();
        if let Some((_, reentered)) = checks.iter_mut().rev().find(|(active, _)| *active == key) {
            *reentered = true;
            true
        } else {
            false
        }
    })
}

#[salsa::tracked(return_ref, cycle_fn=check_definition_body_cycle_recover, cycle_initial=check_definition_body_cycle_initial)]
pub(super) fn check_definition_body<'db>(
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
) -> DefinitionBodyCheck<'db> {
    let guard = DefinitionCheckGuard::enter(db, def);
    let templates = check_definition_templates(db, def);
    let mut checked = templates.clone();
    if let Some(arms) = &templates.arms {
        let assumptions = *collect_constraints(db, def.into()).skip_binder();
        let cx = TraitSolveCx::new(db, def.scope()).with_assumptions(assumptions);
        for (index, &ty) in arms.iter().enumerate() {
            let span = def.span().body().match_().arms().arm(index).ty();
            if let Some(diag) = check_ty_wf(db, cx, ty).into_diag(span.into()) {
                checked.diags.push(diag);
            }
        }
    }
    if !checked.diags.is_empty() {
        checked.arms = None;
    }
    if guard.was_reentered() {
        checked.arms = None;
        checked.diags.push(
            TyLowerDiag::ComputedTypeDeclaration {
                span: def.span().into(),
                reason: "checking this computed type depends on its own result".into(),
            }
            .into(),
        );
    }
    checked
}

fn check_definition_body_cycle_initial<'db>(
    _db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
) -> DefinitionBodyCheck<'db> {
    DefinitionBodyCheck {
        arms: None,
        diags: vec![
            TyLowerDiag::ComputedTypeDeclaration {
                span: def.span().into(),
                reason: "checking this computed type depends on its own result".into(),
            }
            .into(),
        ],
    }
}

fn check_definition_body_cycle_recover<'db>(
    _db: &'db dyn HirAnalysisDb,
    _value: &DefinitionBodyCheck<'db>,
    _count: u32,
    _def: TypeFnDef<'db>,
) -> salsa::CycleRecoveryAction<DefinitionBodyCheck<'db>> {
    salsa::CycleRecoveryAction::Iterate
}

/// Source, kind and decreasing-recursion checks precede inductive formation.
/// These templates alone are not a certificate that their result is well formed.
#[salsa::tracked(return_ref)]
pub(super) fn check_definition_templates<'db>(
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
) -> DefinitionBodyCheck<'db> {
    let recurrence = check_recurrences(db, def);
    let mut diags: Vec<_> = recurrence.diags.iter().cloned().map(Into::into).collect();
    diags.extend(GenericParamOwner::TypeFn(def).diags(db));
    for predicate in WhereClauseOwner::TypeFn(def).clause(db).predicates(db) {
        diags.extend(predicate.diags(db));
    }
    if !diags.is_empty() {
        return DefinitionBodyCheck { arms: None, diags };
    }
    let assumptions = *collect_constraints(db, def.into()).skip_binder();
    let result_kind = lower_kind(
        db,
        def.scope(),
        &def.ret_kind(db).to_opt().expect("checked return kind"),
    );
    let mut arms = Vec::new();
    for (idx, arm) in def.arms(db).iter().enumerate() {
        let span = def.span().body().match_().arms().arm(idx).ty();
        let hir_ty = arm.ty.to_opt().expect("checked arm shape");
        let source_diags = super::ty_error::collect_hir_ty_diags(
            db,
            def.scope(),
            hir_ty,
            span.clone(),
            assumptions,
        );
        if !source_diags.is_empty() {
            diags.extend(source_diags);
            continue;
        }
        let ty = lower_hir_ty_deferred(db, hir_ty, def.scope(), assumptions);
        if let Some(diag) = ty.emit_diag(db, span.clone().into()) {
            diags.push(diag);
            continue;
        }
        if !result_kind.does_match(ty.kind(db)) || ty.is_const_ty(db) {
            diags.push(
                TyLowerDiag::ComputedTypeDeclaration {
                    span: span.clone().into(),
                    reason: "every arm must produce a type of the declared return kind".into(),
                }
                .into(),
            );
        }
        let mut visitor = ArmTypeCheck {
            db,
            def,
            calls: FxHashMap::default(),
            span: span.into(),
            diags: Vec::new(),
        };
        ty.visit_with(&mut visitor);
        let mut expected_calls = FxHashMap::default();
        for path in &recurrence.calls_per_arm[idx] {
            let application = lower_hir_ty_deferred(
                db,
                TypeId::new(db, TypeKind::Path(Partial::Present(*path))),
                def.scope(),
                assumptions,
            );
            *expected_calls.entry(application).or_insert(0usize) += 1;
        }
        if visitor.calls != expected_calls {
            visitor
                .report("a type alias or projection changes the checked recursive call structure");
        }
        diags.extend(visitor.diags);
        arms.push(ty);
    }
    DefinitionBodyCheck {
        arms: diags.is_empty().then_some(arms),
        diags,
    }
}

struct ArmTypeCheck<'db> {
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
    calls: FxHashMap<TyId<'db>, usize>,
    span: DynLazySpan<'db>,
    diags: Vec<TyDiagCollection<'db>>,
}

impl ArmTypeCheck<'_> {
    fn report(&mut self, reason: &str) {
        self.diags.push(
            TyLowerDiag::ComputedTypeDeclaration {
                span: self.span.clone(),
                reason: reason.into(),
            }
            .into(),
        );
    }
}

impl<'db> TyVisitor<'db> for ArmTypeCheck<'db> {
    fn db(&self) -> &'db dyn HirAnalysisDb {
        self.db
    }

    fn visit_ty(&mut self, ty: TyId<'db>) {
        let (head, args) = ty.decompose_ty_app(self.db);
        if let TyData::TyBase(TyBase::TypeFn(def)) = head.data(self.db) {
            *self.calls.entry(ty).or_default() += 1;
            if *def != self.def {
                self.report(
                    "a type alias or qualified path refers to another recursive type function",
                );
            }
            if args.len() != def.generic_params(self.db).data(self.db).len() {
                self.report("a recursive type function must be fully applied");
            }
            for arg in args {
                self.visit_ty(*arg);
            }
        } else {
            super::visitor::walk_ty(self, ty);
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

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
struct RecurrenceCheck<'db> {
    calls_per_arm: Vec<Vec<PathId<'db>>>,
    diags: Vec<TyLowerDiag<'db>>,
}

#[cfg(test)]
fn recurrence_diagnostics<'db>(
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
) -> &'db Vec<TyLowerDiag<'db>> {
    &check_recurrences(db, def).diags
}

/// This check is independent of normalization. No recursive application is
/// executed to establish that its subject decreases.
#[salsa::tracked(return_ref)]
fn check_recurrences<'db>(db: &'db dyn HirAnalysisDb, def: TypeFnDef<'db>) -> RecurrenceCheck<'db> {
    let shape = declaration_shape(db, def);
    let Some(bounds) = &shape.lower_bounds else {
        return RecurrenceCheck {
            calls_per_arm: Vec::new(),
            diags: shape.diags.clone(),
        };
    };
    let mut checker = RecurrenceChecker {
        db,
        def,
        lower_bound: BigUint::from(0u32),
        calls: Vec::new(),
        diags: Vec::new(),
    };
    let mut calls_per_arm = Vec::new();
    for (idx, (arm, lower_bound)) in def.arms(db).iter().zip(bounds).enumerate() {
        let before = checker.calls.len();
        checker.lower_bound.clone_from(lower_bound);
        let ty = arm
            .ty
            .to_opt()
            .expect("complete arm checked by declaration_shape");
        let mut ctxt = VisitorCtxt::new(
            db,
            def.scope(),
            def.span().body().match_().arms().arm(idx).ty(),
        );
        checker.visit_ty(&mut ctxt, ty);
        calls_per_arm.push(checker.calls[before..].to_vec());
    }
    if checker.calls.is_empty() {
        checker.report(
            def.span().into(),
            "a recursive type function must contain a self-call",
        );
    }
    RecurrenceCheck {
        calls_per_arm,
        diags: checker.diags,
    }
}

struct RecurrenceChecker<'db> {
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
    lower_bound: BigUint,
    calls: Vec<PathId<'db>>,
    diags: Vec<TyLowerDiag<'db>>,
}

impl<'db> RecurrenceChecker<'db> {
    fn report(&mut self, span: DynLazySpan<'db>, reason: &str) {
        self.diags.push(TyLowerDiag::ComputedTypeDeclaration {
            span,
            reason: reason.into(),
        });
    }

    fn check_call(&self, path: PathId<'db>) -> Result<(), &'static str> {
        let db = self.db;
        let params = self.def.generic_params(db).data(db);
        let args = path.generic_args(db).data(db);
        if args.len() != params.len() {
            return Err("a recursive call must supply every parameter");
        }
        for (arg, param) in args[..args.len() - 1].iter().zip(params) {
            let name = match arg {
                GenericArg::Type(arg) => arg.ty.to_opt().and_then(|ty| match ty.data(db) {
                    TypeKind::Path(Partial::Present(path)) => path.as_ident(db),
                    _ => None,
                }),
                GenericArg::Const(arg) if matches!(param, GenericParam::Const(_)) => {
                    match arg.value {
                        ConstGenericArgValue::Expr(Partial::Present(body)) => {
                            match body_root(db, body) {
                                Some(Expr::Path(Partial::Present(path))) => path.as_ident(db),
                                _ => None,
                            }
                        }
                        _ => None,
                    }
                }
                _ => None,
            };
            if name.is_none() || name != param.name().to_opt() {
                return Err(
                    "recursive calls must forward invariant parameters unchanged and in order",
                );
            }
        }
        let Some(GenericArg::Const(arg)) = args.last() else {
            return Err("the recursive subject must be a decreasing const expression");
        };
        let ConstGenericArgValue::Expr(Partial::Present(body)) = arg.value else {
            return Err("the recursive subject must be a decreasing const expression");
        };
        self.check_step(body, body_root(db, body))
    }

    fn check_step(&self, body: Body<'db>, expr: Option<Expr<'db>>) -> Result<(), &'static str> {
        let db = self.db;
        let subject = self
            .def
            .generic_params(db)
            .data(db)
            .last()
            .and_then(|param| param.name().to_opt());
        match expr {
            Some(Expr::Lit(LitKind::Int(value))) if value.data(db) < &self.lower_bound => Ok(()),
            Some(Expr::Bin(lhs, rhs, BinOp::Arith(op))) => {
                let lhs = body.exprs(db)[lhs].borrowed().to_opt();
                let rhs = body.exprs(db)[rhs].borrowed().to_opt();
                let Some(Expr::Path(Partial::Present(path))) = lhs else {
                    return Err("the recursive subject must decrease from the declared subject");
                };
                let Some(Expr::Lit(LitKind::Int(amount))) = rhs else {
                    return Err("the recursion step must use a literal amount");
                };
                if path.as_ident(db) != subject {
                    return Err("the recursive subject must decrease from the declared subject");
                }
                let amount = amount.data(db);
                match op {
                    ArithBinOp::Sub if amount > &BigUint::from(0u32) => {
                        if amount <= &self.lower_bound {
                            Ok(())
                        } else {
                            Err("the recursive subtraction may underflow in this arm")
                        }
                    }
                    ArithBinOp::Div if amount >= &BigUint::from(2u32) => {
                        if self.lower_bound > BigUint::from(0u32) {
                            Ok(())
                        } else {
                            Err("recursive division does not decrease the subject at zero")
                        }
                    }
                    _ => Err("the recursive subject must strictly decrease"),
                }
            }
            _ => Err("the recursive subject must strictly decrease"),
        }
    }

    fn is_payload_input(&self, body: Body<'db>, expr: &Expr<'db>) -> bool {
        match expr {
            Expr::Lit(LitKind::Int(_)) => true,
            Expr::Path(Partial::Present(path)) => path.as_ident(self.db).is_some_and(|name| {
                self.def.generic_params(self.db).data(self.db).iter().any(|param| {
                    matches!(param, GenericParam::Const(param) if param.name.to_opt() == Some(name))
                })
            }),
            Expr::Bin(..) => self.check_step(body, Some(expr.clone())).is_ok(),
            _ => false,
        }
    }

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
        let Some(Expr::Path(Partial::Present(path))) = body.exprs(db)[callee].borrowed().to_opt()
        else {
            return false;
        };
        let bucket = resolve_ident_to_bucket(db, *path, self.def.scope());
        let is_const = bucket
            .pick(NameDomain::VALUE)
            .as_ref()
            .ok()
            .and_then(|res| res.scope())
            .is_some_and(
                |scope| matches!(scope, ScopeId::Item(ItemKind::Func(func)) if func.is_const(db)),
            );
        is_const
            && args.iter().all(|arg| {
                body.exprs(db)[arg.expr]
                    .borrowed()
                    .to_opt()
                    .is_some_and(|expr| self.is_payload_input(body, expr))
            })
    }
}

impl<'db> Visitor<'db> for RecurrenceChecker<'db> {
    fn visit_generic_arg(
        &mut self,
        ctxt: &mut VisitorCtxt<'db, crate::core::span::lazy_spans::LazyGenericArgSpan<'db>>,
        arg: &GenericArg<'db>,
    ) {
        if let GenericArg::Const(arg) = arg {
            let admitted = match arg.value {
                ConstGenericArgValue::Expr(Partial::Present(body)) => self.is_payload(body),
                _ => false,
            };
            if !admitted {
                self.report(ctxt.span().expect("generic argument span").into(),
                    "computed type payloads require literals, invariant consts, safe subject steps, or a const function over these inputs");
            }
        }
        walk_generic_arg(self, ctxt, arg);
    }

    fn visit_path(
        &mut self,
        ctxt: &mut VisitorCtxt<'db, crate::core::span::lazy_spans::LazyPathSpan<'db>>,
        path: PathId<'db>,
    ) {
        // Qualified parents and their type arguments are traversed by the
        // ordinary HIR visitor, including `<Wrapper<Tree<...>> as Trait>::Out`.
        if path.parent(self.db).is_none() {
            let bucket = resolve_ident_to_bucket(self.db, path, self.def.scope());
            if let Ok(resolved) = bucket.pick(NameDomain::TYPE)
                && let Some(ScopeId::Item(ItemKind::TypeFn(def))) = resolved.scope()
            {
                let span = ctxt.span().expect("path visitor has a path span").into();
                if def == self.def {
                    self.calls.push(path);
                    if let Err(reason) = self.check_call(path) {
                        self.report(span, reason);
                    }
                } else {
                    self.report(
                        span,
                        "recursive type function bodies cannot call another type function",
                    );
                }
            }
        }
        walk_path(self, ctxt, path);
    }
}

fn body_root<'db>(db: &'db dyn HirAnalysisDb, body: Body<'db>) -> Option<Expr<'db>> {
    let root = body.exprs(db)[body.expr(db)].clone().to_opt();
    if let Some(Expr::Block(stmts)) = &root
        && stmts.len() == 1
        && let Some(Stmt::Expr(inner)) = body.stmts(db)[stmts[0]].borrowed().to_opt()
    {
        return body.exprs(db)[*inner].clone().to_opt();
    }
    root
}

#[derive(Debug, Clone, PartialEq, Eq, salsa::Update)]
pub(super) struct DeclarationShape<'db> {
    /// Present only for a complete, disjoint match with a final wildcard.
    /// Entries correspond to source arms, including literal arms.
    pub(super) lower_bounds: Option<Vec<BigUint>>,
    pub(super) diags: Vec<TyLowerDiag<'db>>,
}

#[salsa::tracked(return_ref)]
pub(super) fn declaration_shape<'db>(
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
) -> DeclarationShape<'db> {
    let mut diags = Vec::new();
    let mut report = |span: DynLazySpan<'db>, reason: &str| {
        diags.push(TyLowerDiag::ComputedTypeDeclaration {
            span,
            reason: reason.into(),
        });
    };
    let params = def.generic_params(db).data(db);
    match params.last() {
        Some(GenericParam::Const(subject)) => {
            let usize_ty = TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::Usize)));
            let subject_ty = subject.ty.to_opt().map(|ty| {
                lower_hir_ty_deferred(db, ty, def.scope(), PredicateListId::empty_list(db))
            });
            if subject_ty != Some(usize_ty) {
                report(
                    def.span().generic_params().into(),
                    "the final recursion subject must have type `usize`",
                );
            }
            if subject.name.to_opt().is_none()
                || subject.name.to_opt() != def.match_subject(db).to_opt()
            {
                report(
                    def.span().body().match_().subject().into(),
                    "match must inspect the final recursion subject",
                );
            }
        }
        _ => report(
            def.span().generic_params().into(),
            "a recursive type function requires a final const recursion subject",
        ),
    }

    match def.ret_kind(db).to_opt() {
        // The parser reports a missing return kind.
        None => {}
        Some(kind)
            if let Some(name) = super::ty_lower::unknown_named_kind(db, def.scope(), &kind) =>
        {
            report(
                def.span().ret_kind().into(),
                &format!("`{name}` does not resolve to a kind marker"),
            )
        }
        Some(kind) if mentions_constraint(db, def.scope(), &kind) => report(
            def.span().ret_kind().into(),
            "a pure computed type cannot return kind `Constraint`",
        ),
        Some(_) => {}
    }

    let clause = def.where_clause(db);
    if !clause.const_predicates(db).is_empty() {
        report(
            def.span().where_clause().into(),
            "recursive type functions do not admit const where predicates",
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
                "recursive type function where clauses may bound only type parameters",
            );
        }
    }

    let arms = def.arms(db);
    let mut literals = FxHashSet::default();
    for (idx, arm) in arms.iter().enumerate() {
        let span = def.span().body().match_().arms().arm(idx);
        match arm.pat.to_opt() {
            Some(TypeFnPat::Lit(value)) => {
                if !literals.insert(value.data(db).clone()) {
                    report(span.clone().into(), "duplicate literal match arm");
                }
            }
            Some(TypeFnPat::Wild) if idx + 1 != arms.len() => report(
                span.clone().into(),
                "the wildcard must be the final match arm",
            ),
            Some(TypeFnPat::Wild) => {}
            None => report(span.clone().into(), "incomplete match pattern"),
        }
        if arm.ty.to_opt().is_none() {
            report(span.into(), "every match arm must produce a type");
        }
    }
    if !matches!(
        arms.last().and_then(|arm| arm.pat.to_opt()),
        Some(TypeFnPat::Wild)
    ) {
        report(
            def.span().body().match_().arms().into(),
            "a final wildcard arm is required",
        );
    }

    let lower_bounds = if diags.is_empty() {
        let mut first_unmatched = BigUint::from(0u32);
        while literals.contains(&first_unmatched) {
            first_unmatched += 1u32;
        }
        Some(
            arms.iter()
                .map(
                    |arm| match arm.pat.to_opt().expect("complete arm checked above") {
                        TypeFnPat::Lit(value) => value.data(db).clone(),
                        TypeFnPat::Wild => first_unmatched.clone(),
                    },
                )
                .collect(),
        )
    } else {
        None
    };
    DeclarationShape {
        lower_bounds,
        diags,
    }
}

fn mentions_constraint(db: &dyn HirAnalysisDb, scope: ScopeId<'_>, kind: &KindBound) -> bool {
    match kind {
        KindBound::Named(_) => lower_kind(db, scope, kind) == Kind::Constraint,
        KindBound::Abs(lhs, rhs) => {
            lhs.borrowed()
                .to_opt()
                .is_some_and(|kind| mentions_constraint(db, scope, kind))
                || rhs
                    .borrowed()
                    .to_opt()
                    .is_some_and(|kind| mentions_constraint(db, scope, kind))
        }
        KindBound::Mono => false,
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use salsa::Setter;

    use super::*;
    use crate::test_db::{HirAnalysisTestDb, format_diagnostics};

    fn definition<'db>(db: &'db HirAnalysisTestDb, file: common::file::File) -> TypeFnDef<'db> {
        let (top, _) = db.top_mod(file);
        top.all_items(db)
            .iter()
            .find_map(|item| match item {
                ItemKind::TypeFn(def) => Some(*def),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn computed_type_match_bounds_follow_unmatched_values_not_arm_count() {
        for (patterns, expected) in [
            ("0 => T\n2 => T\n_ => T", vec![0u32, 2, 1]),
            ("2 => T\n0 => T\n1 => T\n_ => T", vec![2, 0, 1, 3]),
            ("5 => T\n_ => T", vec![5, 0]),
            ("_ => T", vec![0]),
        ] {
            let mut db = HirAnalysisTestDb::default();
            let source = format!(
                "recursive type fn Tree<T, const O: usize, const N: usize>() -> (*) {{ match N {{ {patterns} }} }}"
            );
            let file = db.new_stand_alone(Utf8PathBuf::from("bounds.fe"), &source);
            let checked = declaration_shape(&db, definition(&db, file));
            assert!(checked.diags.is_empty(), "{:?}", checked.diags);
            assert_eq!(
                checked.lower_bounds,
                Some(expected.into_iter().map(BigUint::from).collect())
            );
        }
    }

    #[test]
    fn computed_type_declaration_errors_survive_edits_and_restore() {
        let valid = "recursive type fn Tree<T, const O: usize, const N: usize>() -> (*) { match N { 0 => T, _ => Tree<T, O, {N - 1}> } }";
        let cases = [
            (
                valid.replace("const N: usize", "N"),
                "final const recursion subject",
            ),
            (
                valid.replace("const N: usize", "const N: bool"),
                "type `usize`",
            ),
            (valid.replace("match N", "match O"), "match must inspect"),
            (valid.replace("-> (*)", ""), "explicit return kind"),
            (
                valid.replace("-> (*)", "-> (Constraint)"),
                "cannot return kind `Constraint`",
            ),
            (
                valid.replace("0 => T", "0 => T, 0 => T"),
                "duplicate literal",
            ),
            (
                valid.replace("0 => T", "_ => T"),
                "wildcard must be the final",
            ),
            (
                valid.replace("_ => Tree<T, O, {N - 1}>", ""),
                "final wildcard arm is required",
            ),
        ];
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("declaration.fe"), valid);
        for (invalid, message) in cases {
            for (source, expected) in [
                (valid, None),
                (invalid.as_str(), Some(message)),
                (valid, None),
            ] {
                file.set_text(&mut db).to(source.to_owned());
                let (top, _) = db.top_mod(file);
                let actual = format_diagnostics(&db, &db.run_on_top_mod(top));
                if let Some(expected) = expected {
                    assert!(actual.contains(expected), "{expected}: {actual}");
                    assert!(
                        declaration_shape(&db, definition(&db, file))
                            .lower_bounds
                            .is_none()
                    );
                } else {
                    assert!(actual.is_empty(), "{actual}");
                }
                let mut fresh = HirAnalysisTestDb::default();
                let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("declaration.fe"), source);
                let (fresh_top, _) = fresh.top_mod(fresh_file);
                assert_eq!(
                    actual,
                    format_diagnostics(&fresh, &fresh.run_on_top_mod(fresh_top))
                );
            }
        }
    }

    #[test]
    fn computed_type_recursion_checks_all_arms_and_qualified_parents() {
        let template = r#"
trait Output { type Out }
struct Wrapper<T> {}
impl<T> Output for Wrapper<T> { type Out = T }
recursive type fn Tree<T, const O: usize, const N: usize>() -> (*) {
    match N {
        BASE
        _ => STEP
    }
}
"#;
        for (base, step, accepted) in [
            ("0 => T", "Tree<T, O, {N - 1}>", true),
            ("0 => T", "Tree<T, O, {N / 2}>", true),
            ("0 => T", "Tree<T, O, {0}>", true),
            ("0 => T, 1 => T", "Tree<T, O, {N - 2}>", true),
            ("0 => T, 2 => T", "Tree<T, O, {N - 2}>", false),
            ("", "Tree<T, O, {N / 2}>", false),
            ("0 => T", "Tree<T, O, {N / 1}>", false),
            ("0 => T", "Tree<T, O, {N - 0}>", false),
            ("0 => T", "Tree<T, O, {N + 1}>", false),
            ("0 => T", "Tree<T, O, {O - 1}>", false),
            ("0 => T", "Tree<bool, O, {N - 1}>", false),
            ("0 => T", "Tree<T, {O + 1}, {N - 1}>", false),
            ("0 => T", "Tree<T, N, {N - 1}>", false),
            (
                "0 => T",
                "<Wrapper<Tree<T, O, {N - 1}>> as Output>::Out",
                true,
            ),
            (
                "0 => T",
                "<Wrapper<Tree<T, O, {N + 1}>> as Output>::Out",
                false,
            ),
            ("0 => Tree<T, O, {N - 1}>", "Tree<T, O, {N - 1}>", false),
        ] {
            let mut db = HirAnalysisTestDb::default();
            let source = template.replace("BASE", base).replace("STEP", step);
            let file = db.new_stand_alone(Utf8PathBuf::from("recursion.fe"), &source);
            let diags = recurrence_diagnostics(&db, definition(&db, file));
            assert_eq!(diags.is_empty(), accepted, "{source}\n{diags:?}");
        }
    }

    #[test]
    fn computed_type_body_checks_payloads_kinds_and_preserved_recursion() {
        let template = r#"
struct Pair<A, B> {}
struct Term<const I: usize> {}
const fn payload(_ i: usize) -> usize { i + 1 }
fn runtime(_ i: usize) -> usize { i + 1 }
type Keep<X> = X
type Mask<X, T, const O: usize, const N: usize> = Tree<T, O, N>
recursive type fn Tree<T, const O: usize, const N: usize>() -> (*) {
    match N {
        0 => BASE
        _ => STEP
    }
}
"#;
        for (base, step, accepted) in [
            ("T", "Tree<T, O, {N - 1}>", true),
            ("T", "Keep<Tree<T, O, {N - 1}>>", true),
            (
                "T",
                "Pair<Term<{payload(N - 1)}>, Tree<T, O, {N - 1}>>",
                true,
            ),
            ("Missing", "Tree<T, O, {N - 1}>", false),
            ("Pair", "Tree<T, O, {N - 1}>", false),
            ("Term<{true}>", "Tree<T, O, {N - 1}>", false),
            (
                "T",
                "Pair<Term<{runtime(N - 1)}>, Tree<T, O, {N - 1}>>",
                false,
            ),
            (
                "T",
                "Pair<Term<{if false { true } else { 1 }}>, Tree<T, O, {N - 1}>>",
                false,
            ),
            ("T", "Mask<Tree<T, O, {N - 1}>, T, O, N>", false),
        ] {
            let mut db = HirAnalysisTestDb::default();
            let source = template.replace("BASE", base).replace("STEP", step);
            let file = db.new_stand_alone(Utf8PathBuf::from("body_check.fe"), &source);
            let checked = check_definition_body(&db, definition(&db, file));
            assert_eq!(
                checked.arms.is_some(),
                accepted,
                "{source}\n{:?}",
                checked.diags
            );
            assert_eq!(
                checked.diags.is_empty(),
                accepted,
                "{source}\n{:?}",
                checked.diags
            );
        }
    }

    #[test]
    fn computed_type_normalizes_structure_and_const_payloads_in_ordinary_signatures() {
        let template = r#"
struct Pair<A, B> {}
struct Term<const I: usize> {}
const fn payload(_ i: usize, _ o: usize) -> usize { i * 3 + o }
recursive type fn Tree<T, const O: usize, const N: usize>() -> (*) {
    match N {
        0 => T
        _ => Pair<Term<{payload(N - 1, O)}>, Tree<T, O, {N - 1}>>
    }
}
type Expected<T> = Pair<Term<FIRST>, Pair<Term<10>, Pair<Term<7>, T>>>
fn forward(x: Tree<bool, 7, 3>) -> Expected<bool> { x }
fn backward(x: Expected<bool>) -> Tree<bool, 7, 3> { x }
fn generic<T>(x: Tree<T, 7, 3>) -> Expected<T> { x }
"#;
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("normalized.fe"), "");
        for (first, accepted) in [("13", true), ("14", false), ("13", true)] {
            let source = template.replace("FIRST", first);
            file.set_text(&mut db).to(source.clone());
            let (top, _) = db.top_mod(file);
            let actual = format_diagnostics(&db, &db.run_on_top_mod(top));
            assert_eq!(actual.is_empty(), accepted, "{actual}");
            let mut fresh = HirAnalysisTestDb::default();
            let fresh_file = fresh.new_stand_alone(Utf8PathBuf::from("normalized.fe"), &source);
            let (fresh_top, _) = fresh.top_mod(fresh_file);
            assert_eq!(
                actual,
                format_diagnostics(&fresh, &fresh.run_on_top_mod(fresh_top))
            );
        }
    }
}
