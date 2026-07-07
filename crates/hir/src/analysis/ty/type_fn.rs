//! Definition-site well-formedness for `recursive type fn` items and the
//! distilled arm representation that gates unfolding (spec sec 1.1 / 1.2 / 3,
//! slice S1.4).
//!
//! This module is the S1.5 gate. [`type_fn_wf`] performs the full definition
//! checklist purely syntactically (spec sec 1.1 grammar, sec 1.2 self-call-only,
//! sec 3 termination) and, ONLY on success, produces a [`TypeFnWfData`]: the
//! distilled arms with each self-call reduced to a [`SubjectStep`]. That
//! distilled type is the ONLY input the S1.5 unfold queries accept, so
//! "well-formedness gates unfolding" is a structural fact, not a convention.
//!
//! The check NEVER routes a body block through the const evaluator: subject
//! arguments are destructured syntactically (`Bin(Sub|Div)(Path(subject),
//! IntLit)` / an integer literal), never evaluated. `evaluate_const_ty`'s
//! `NotIntExpr` fall-through into the CTFE machine therefore stays unreachable
//! from type-fn bodies (Fable steering finding 2).

use num_bigint::BigUint;

use crate::analysis::HirAnalysisDb;
use crate::analysis::name_resolution::{NameDomain, resolve_ident_to_bucket};
use crate::analysis::ty::diagnostics::{TyLowerDiag, TypeFnWfError};
use crate::analysis::ty::trait_resolution::PredicateListId;
use crate::analysis::ty::ty_def::{
    PrimTy, TyBase, TyData, find_unsaturated_type_fn, kind_mentions_constraint,
};
use crate::analysis::ty::ty_lower::{lower_hir_ty, lower_kind};
use crate::core::hir_def::scope_graph::ScopeId;
use crate::core::hir_def::{
    ArithBinOp, BinOp, Body, ConstGenericArgValue, Expr, GenericArg, GenericParam, IdentId,
    IntegerId, ItemKind, LitKind, Partial, PathId, Stmt, TypeFnDef, TypeFnPat, TypeId, TypeKind,
};

/// A single step the recursion subject takes at a self-call (spec sec 3.3),
/// distilled from the whitelisted syntactic subject form. This enum is the ONLY
/// subject representation the S1.5 unfold queries consume; it is produced only
/// after well-formedness succeeds, so "WF gates unfolding" is structural.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub enum SubjectStep<'db> {
    /// `{N - k}`, with `k >= 1` and the arm's lower bound `L >= k`.
    Sub(IntegerId<'db>),
    /// `{N / k}`, with `k >= 2` and the arm's lower bound `L >= 1`.
    Div(IntegerId<'db>),
    /// A literal subject `m` (only inside a `_` arm, with `m < L`).
    Lit(IntegerId<'db>),
}

/// A distilled, well-formed `recursive type fn` arm (spec sec 1.1). Produced by
/// [`type_fn_wf`] only on WF success.
#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct TypeFnArmData<'db> {
    /// The arm pattern (`LIT` or `_`).
    pub pat: TypeFnPat<'db>,
    /// The arm RHS type, retained intensionally as HIR (spec sec 6.1); S1.5
    /// substitutes into and lowers it.
    pub rhs_ty: TypeId<'db>,
    /// The distilled subject step of every self-call in this arm, in source
    /// order. Empty for a base arm.
    pub self_calls: Vec<SubjectStep<'db>>,
}

/// The validated body of a `recursive type fn` (slice S1.4): the distilled arm
/// data plus the resolved subject-parameter index. This is the SOLE input type
/// the S1.5 unfold queries accept.
#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct TypeFnWfData<'db> {
    pub def: TypeFnDef<'db>,
    /// Index of the `const N: usize` subject in the generic-param list (always
    /// the last param on WF success).
    pub subject_idx: usize,
    /// The distilled arms, in source order.
    pub arms: Vec<TypeFnArmData<'db>>,
}

/// Result of the `recursive type fn` well-formedness check.
#[derive(Debug, Clone, PartialEq, Eq, Hash, salsa::Update)]
pub struct TypeFnWfResult<'db> {
    /// The distilled arm data, present ONLY when the definition is well-formed
    /// (`diags` empty). S1.5 consumes exactly this.
    pub data: Option<TypeFnWfData<'db>>,
    /// Definition well-formedness diagnostics.
    pub diags: Vec<TyLowerDiag<'db>>,
}

/// Runs the definition-site well-formedness check for a `recursive type fn`
/// (spec slice S1.4). Memoized: the distilled output is stable HIR-derived data.
#[salsa::tracked(return_ref)]
pub fn type_fn_wf<'db>(db: &'db dyn HirAnalysisDb, def: TypeFnDef<'db>) -> TypeFnWfResult<'db> {
    Checker::new(db, def).run()
}

/// How an arm-RHS type path classifies for the self-call / no-chaining rules.
enum PathClass {
    /// An application of the type fn being defined (spec sec 1.2 self-loop).
    SelfCall,
    /// An application of another type fn (constraint D violation).
    ForeignTypeFn,
    /// An associated-type projection (`G::Assoc`), forbidden in bodies.
    AssocProj,
    /// Anything else: a type parameter, a concrete type/alias, a prim, a module
    /// path. Recurse into its generic arguments.
    Other,
}

struct Checker<'db> {
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
    diags: Vec<TyLowerDiag<'db>>,
    /// The declared subject parameter name (`N`), if a unique last const param
    /// was found. Body checks that need it are skipped when it is `None`.
    subject_name: Option<IdentId<'db>>,
    /// The names of the type params (all params before the subject), in order.
    type_param_names: Vec<Option<IdentId<'db>>>,
    /// Whether any arm contained a syntactic self-call, well-formed or not. The
    /// "at least one self-call" rule (spec sec 1.1 rule 5) is about presence, so
    /// an ill-formed self-call still satisfies it (and reports its own error).
    saw_self_call: bool,
}

impl<'db> Checker<'db> {
    fn new(db: &'db dyn HirAnalysisDb, def: TypeFnDef<'db>) -> Self {
        Self {
            db,
            def,
            diags: vec![],
            subject_name: None,
            type_param_names: vec![],
            saw_self_call: false,
        }
    }

    fn emit(&mut self, primary: crate::span::DynLazySpan<'db>, error: TypeFnWfError<'db>) {
        self.diags.push(TyLowerDiag::TypeFnIllFormed { primary, error });
    }

    fn arm_ty_span(&self, arm_idx: usize) -> crate::span::DynLazySpan<'db> {
        self.def
            .span()
            .body()
            .match_()
            .arms()
            .arm(arm_idx)
            .ty()
            .into()
    }

    fn run(mut self) -> TypeFnWfResult<'db> {
        let db = self.db;
        let def = self.def;

        let subject_idx = self.check_subject_param();
        self.check_return_kind();
        self.check_where_clause();
        self.check_scrutinee();

        let (arms, arms_complete) = self.check_arms(subject_idx);

        let data = if self.diags.is_empty() && arms_complete && subject_idx.is_some() {
            Some(TypeFnWfData {
                def,
                subject_idx: subject_idx.unwrap(),
                arms,
            })
        } else {
            None
        };

        // Only when everything above is well-formed do we lower the arm RHS
        // types; this exercises the S1.3 saturation walk on the definition
        // itself (catching an unsaturated self- or nested occurrence) and is
        // safe because the whitelisted subjects have already been validated
        // syntactically, so no body block reaches the const evaluator.
        if let Some(ref data) = data {
            let assumptions = PredicateListId::empty_list(db);
            let scope = def.scope();
            for (arm_idx, arm) in data.arms.iter().enumerate() {
                let lowered = lower_hir_ty(db, arm.rhs_ty, scope, assumptions);
                if let Some((_, expected, given)) = find_unsaturated_type_fn(db, lowered) {
                    self.diags.push(TyLowerDiag::TypeFnNotSaturated {
                        span: self.arm_ty_span(arm_idx),
                        expected,
                        given,
                    });
                }
            }
        }

        // If lowering surfaced a residual saturation error, the definition is no
        // longer well-formed: withhold the distilled data.
        let data = if self.diags.is_empty() { data } else { None };

        TypeFnWfResult {
            data,
            diags: self.diags,
        }
    }

    /// Spec sec 1.1 rule 1: exactly one `const _: usize` subject, declared last.
    /// Returns the subject index when a single last const param is present
    /// (regardless of its declared type), so downstream checks can proceed.
    fn check_subject_param(&mut self) -> Option<usize> {
        let db = self.db;
        let params = self.def.hir_generic_params(db).data(db);
        let params_span = self.def.span().generic_params().into();

        let const_indices: Vec<usize> = params
            .iter()
            .enumerate()
            .filter(|(_, p)| matches!(p, GenericParam::Const(_)))
            .map(|(i, _)| i)
            .collect();

        if const_indices.is_empty() {
            self.emit(params_span, TypeFnWfError::MissingSubject);
            return None;
        }
        if const_indices.len() > 1 {
            self.emit(
                params_span,
                TypeFnWfError::MultipleSubjects {
                    count: const_indices.len(),
                },
            );
        }

        let subject_idx = *const_indices.last().unwrap();
        if subject_idx != params.len() - 1 {
            self.emit(
                self.def.span().generic_params().into(),
                TypeFnWfError::SubjectNotLast,
            );
        }

        let GenericParam::Const(subject) = &params[subject_idx] else {
            unreachable!("subject_idx points at a const param");
        };
        self.subject_name = subject.name.to_opt();

        // Type params are every param before the subject.
        self.type_param_names = params[..subject_idx].iter().map(|p| p.name().to_opt()).collect();

        // The subject's declared type must be `usize`.
        let is_usize = subject.ty.to_opt().is_some_and(|ty| {
            let lowered = lower_hir_ty(db, ty, self.def.scope(), PredicateListId::empty_list(db));
            matches!(lowered.data(db), TyData::TyBase(TyBase::Prim(PrimTy::Usize)))
        });
        if !is_usize {
            self.emit(
                self.def.span().generic_params().into(),
                TypeFnWfError::SubjectNotUsize,
            );
        }

        Some(subject_idx)
    }

    /// Spec sec 1.1 rule 6: the `where` clause may bound only type parameters.
    fn check_where_clause(&mut self) {
        let db = self.db;
        let wc = self.def.hir_where_clause(db);
        let wc_span: crate::span::DynLazySpan<'db> = self.def.span().where_clause().into();

        if !wc.const_predicates(db).is_empty() {
            self.emit(wc_span.clone(), TypeFnWfError::WhereNotTypeParamBound);
        }

        let type_params: Vec<IdentId> = self.type_param_names.iter().flatten().copied().collect();
        for pred in wc.data(db) {
            let bounds_type_param = pred.ty.to_opt().is_some_and(|ty| {
                bare_path_ident(db, ty).is_some_and(|id| type_params.contains(&id))
            });
            if !bounds_type_param {
                self.emit(wc_span.clone(), TypeFnWfError::WhereNotTypeParamBound);
            }
        }
    }

    /// Spec sec 2.1 / 2.3: a return kind is required and may not be `Constraint`
    /// in v1 (the reused parser accepts both an absent kind and `Constraint`).
    fn check_return_kind(&mut self) {
        let db = self.db;
        match self.def.ret_kind_bound(db).to_opt() {
            None => self.emit(
                self.def.span().name().into(),
                TypeFnWfError::MissingReturnKind,
            ),
            Some(kind) => {
                if kind_mentions_constraint(&lower_kind(&kind)) {
                    self.diags.push(TyLowerDiag::TypeFnConstraintRetKind {
                        span: self.def.span().ret_kind().into(),
                    });
                }
            }
        }
    }

    /// Spec sec 1.1 / 3.1: the `match` scrutinee must be the subject parameter.
    fn check_scrutinee(&mut self) {
        let db = self.db;
        let (Some(subject), Some(scrutinee)) =
            (self.subject_name, self.def.match_subject_ident(db).to_opt())
        else {
            return;
        };
        if subject != scrutinee {
            self.emit(
                self.def.span().body().match_().subject().into(),
                TypeFnWfError::ScrutineeMismatch { subject },
            );
        }
    }

    /// Spec sec 1.1 rule 4 (exhaustiveness, duplicates, arms-after-`_`) plus the
    /// per-arm body walk (self-call-only, termination, grammar). Returns the
    /// distilled arms and whether every arm was structurally complete.
    fn check_arms(&mut self, subject_idx: Option<usize>) -> (Vec<TypeFnArmData<'db>>, bool) {
        let db = self.db;
        let hir_arms = self.def.hir_arms(db);

        // Exhaustiveness: a mandatory final `_` arm, no arms after it.
        let wild_positions: Vec<usize> = hir_arms
            .iter()
            .enumerate()
            .filter(|(_, a)| matches!(a.pat.to_opt(), Some(TypeFnPat::Wild)))
            .map(|(i, _)| i)
            .collect();
        let last_is_wild = matches!(
            hir_arms.last().and_then(|a| a.pat.to_opt()),
            Some(TypeFnPat::Wild)
        );
        if wild_positions.is_empty() || !last_is_wild {
            self.emit(
                self.def.span().body().match_().arms().into(),
                TypeFnWfError::MissingWildcardArm,
            );
        }
        for &pos in wild_positions.iter().filter(|&&p| p + 1 < hir_arms.len()) {
            self.emit(
                self.arm_ty_span(pos + 1),
                TypeFnWfError::ArmAfterWildcard,
            );
        }

        // Duplicate literal arms, and the set of matched literals (for `L`).
        let mut seen: Vec<BigUint> = vec![];
        for (idx, arm) in hir_arms.iter().enumerate() {
            if let Some(TypeFnPat::Lit(value)) = arm.pat.to_opt() {
                let v = value.data(db).clone();
                if seen.contains(&v) {
                    self.emit(
                        self.arm_ty_span(idx),
                        TypeFnWfError::DuplicateArmLit { value },
                    );
                } else {
                    seen.push(v);
                }
            }
        }

        // Per-arm body walk.
        let mut distilled = vec![];
        let mut complete = subject_idx.is_some();
        for (idx, arm) in hir_arms.iter().enumerate() {
            let (Some(pat), Some(rhs_ty)) = (arm.pat.to_opt(), arm.ty.to_opt()) else {
                complete = false;
                continue;
            };

            let l = self.arm_lower_bound(&pat, &seen);
            let mut self_calls = vec![];
            if self.subject_name.is_some() {
                self.walk_arm_ty(idx, rhs_ty, &l, &mut self_calls);
            }
            distilled.push(TypeFnArmData {
                pat,
                rhs_ty,
                self_calls,
            });
        }

        // Spec sec 1.1 rule 5: at least one self-call somewhere.
        if !self.saw_self_call {
            self.emit(self.def.span().name().into(), TypeFnWfError::NoSelfCall);
        }

        (distilled, complete)
    }

    /// The arm's lower bound `L` (spec sec 3.2): the literal for a literal arm,
    /// or the smallest `usize` not matched by any preceding literal for `_`.
    fn arm_lower_bound(&self, pat: &TypeFnPat<'db>, matched: &[BigUint]) -> BigUint {
        match pat {
            TypeFnPat::Lit(value) => value.data(self.db).clone(),
            TypeFnPat::Wild => {
                let one = BigUint::from(1u32);
                let mut candidate = BigUint::from(0u32);
                while matched.contains(&candidate) {
                    candidate += &one;
                }
                candidate
            }
        }
    }

    /// Recursively walks an arm-RHS type, classifying every path and distilling
    /// self-call subjects. Emits a diagnostic for each violation encountered.
    fn walk_arm_ty(
        &mut self,
        arm_idx: usize,
        ty: TypeId<'db>,
        l: &BigUint,
        calls: &mut Vec<SubjectStep<'db>>,
    ) {
        let db = self.db;
        match ty.data(db) {
            TypeKind::Path(Partial::Present(path)) => match self.classify_path(*path) {
                PathClass::SelfCall => self.handle_self_call(arm_idx, *path, l, calls),
                PathClass::ForeignTypeFn => {
                    self.emit(self.arm_ty_span(arm_idx), TypeFnWfError::ForeignTypeFnCall)
                }
                PathClass::AssocProj => {
                    self.emit(self.arm_ty_span(arm_idx), TypeFnWfError::AssocProjInArm)
                }
                PathClass::Other => {
                    for arg in path.generic_args(db).data(db) {
                        match arg {
                            GenericArg::Type(t) => {
                                if let Partial::Present(ty) = t.ty {
                                    self.walk_arm_ty(arm_idx, ty, l, calls);
                                }
                            }
                            GenericArg::AssocType(a) => {
                                if let Partial::Present(ty) = a.ty {
                                    self.walk_arm_ty(arm_idx, ty, l, calls);
                                }
                            }
                            GenericArg::Const(_) => {}
                        }
                    }
                }
            },
            TypeKind::Path(Partial::Absent) => {}
            TypeKind::Tuple(_)
            | TypeKind::Array(..)
            | TypeKind::Ptr(_)
            | TypeKind::Mode(..)
            | TypeKind::Never => {
                self.emit(self.arm_ty_span(arm_idx), TypeFnWfError::DisallowedArmType)
            }
        }
    }

    /// Classifies a path in an arm RHS. Self-call detection is by `DefId` via the
    /// early name resolver, which never lowers generic arguments (so no body
    /// block reaches the const evaluator). Self-calls must be single-segment
    /// (the syntactic self identifier); multi-segment references are inspected
    /// only for an associated-type projection at the root.
    fn classify_path(&self, path: PathId<'db>) -> PathClass {
        let db = self.db;
        let scope = self.def.scope();
        if path.parent(db).is_none() {
            match resolve_leaf_scope(db, path, scope) {
                Some(ScopeId::Item(ItemKind::TypeFn(d))) => {
                    if d == self.def {
                        PathClass::SelfCall
                    } else {
                        PathClass::ForeignTypeFn
                    }
                }
                _ => PathClass::Other,
            }
        } else {
            let root = path.segment(db, 0).unwrap();
            match resolve_leaf_scope(db, root, scope) {
                Some(ScopeId::GenericParam(..)) => PathClass::AssocProj,
                _ => PathClass::Other,
            }
        }
    }

    /// Validates a self-call (spec sec 3.3, sec 5.3 verbatim narrowing) and
    /// distills its subject step. Type arguments must be the definition's own
    /// type params forwarded verbatim, in order; the final argument is the
    /// subject.
    fn handle_self_call(
        &mut self,
        arm_idx: usize,
        path: PathId<'db>,
        l: &BigUint,
        calls: &mut Vec<SubjectStep<'db>>,
    ) {
        let db = self.db;
        self.saw_self_call = true;
        let args = path.generic_args(db).data(db);
        let n_type_params = self.type_param_names.len();

        if args.len() != n_type_params + 1 {
            self.emit(
                self.arm_ty_span(arm_idx),
                TypeFnWfError::SelfCallArgsNotVerbatim,
            );
            return;
        }

        for (i, arg) in args[..n_type_params].iter().enumerate() {
            let ok = match (arg, self.type_param_names[i]) {
                (GenericArg::Type(t), Some(expected)) => t
                    .ty
                    .to_opt()
                    .and_then(|ty| bare_path_ident(db, ty))
                    .is_some_and(|id| id == expected),
                _ => false,
            };
            if !ok {
                self.emit(
                    self.arm_ty_span(arm_idx),
                    TypeFnWfError::SelfCallArgsNotVerbatim,
                );
                return;
            }
        }

        match self.distill_subject(&args[n_type_params], l) {
            Ok(step) => calls.push(step),
            Err(error) => self.emit(self.arm_ty_span(arm_idx), error),
        }
    }

    /// Distills a self-call subject argument into a [`SubjectStep`] (spec
    /// sec 3.3), or returns the specific termination/grammar violation. Purely
    /// syntactic: the subject body is destructured, never evaluated.
    fn distill_subject(
        &self,
        arg: &GenericArg<'db>,
        l: &BigUint,
    ) -> Result<SubjectStep<'db>, TypeFnWfError<'db>> {
        let db = self.db;
        let subject = self.subject_name.expect("checked by caller");

        let GenericArg::Const(const_arg) = arg else {
            return Err(TypeFnWfError::SubjectNotDecreasing);
        };
        let ConstGenericArgValue::Expr(Partial::Present(body)) = const_arg.value else {
            return Err(TypeFnWfError::SubjectNotDecreasing);
        };
        let Some(root) = body_root_expr(db, body) else {
            return Err(TypeFnWfError::SubjectNotDecreasing);
        };

        match root {
            Expr::Bin(lhs, rhs, BinOp::Arith(op @ (ArithBinOp::Sub | ArithBinOp::Div))) => {
                if !expr_is_ident(db, body, lhs, subject) {
                    return Err(TypeFnWfError::SubjectNotDecreasing);
                }
                let Some(k) = expr_int_lit(db, body, rhs) else {
                    return Err(TypeFnWfError::SubjectNotDecreasing);
                };
                let kv = k.data(db);
                match op {
                    ArithBinOp::Sub => {
                        if *kv < BigUint::from(1u32) {
                            return Err(TypeFnWfError::SubjectNotDecreasing);
                        }
                        if l < kv {
                            return Err(TypeFnWfError::SubjectMayUnderflow);
                        }
                        Ok(SubjectStep::Sub(k))
                    }
                    ArithBinOp::Div => {
                        if *kv < BigUint::from(2u32) {
                            return Err(TypeFnWfError::SubjectNotDecreasing);
                        }
                        if *l < BigUint::from(1u32) {
                            return Err(TypeFnWfError::SubjectDivZeroFixpoint);
                        }
                        Ok(SubjectStep::Div(k))
                    }
                    _ => unreachable!(),
                }
            }
            Expr::Lit(LitKind::Int(m)) => {
                if m.data(db) < l {
                    Ok(SubjectStep::Lit(m))
                } else {
                    Err(TypeFnWfError::LiteralSubjectNotSmaller)
                }
            }
            _ => Err(TypeFnWfError::SubjectNotDecreasing),
        }
    }
}

/// Resolves a single-segment path's leaf name to a scope via the early name
/// resolver (no generic-argument lowering).
fn resolve_leaf_scope<'db>(
    db: &'db dyn HirAnalysisDb,
    path: PathId<'db>,
    scope: ScopeId<'db>,
) -> Option<ScopeId<'db>> {
    let bucket = resolve_ident_to_bucket(db, path, scope);
    let nameres = bucket.pick(NameDomain::TYPE).as_ref().ok()?;
    nameres.scope()
}

/// The root expression of a const-argument body, unwrapping a single-statement
/// block (the parser wraps a braced subject `{N - 1}` in a block).
fn body_root_expr<'db>(db: &'db dyn HirAnalysisDb, body: Body<'db>) -> Option<Expr<'db>> {
    let root_id = body.expr(db);
    let root = body.exprs(db)[root_id].clone().to_opt();
    if let Some(Expr::Block(stmts)) = &root
        && stmts.len() == 1
        && let Some(Stmt::Expr(inner)) = body.stmts(db)[stmts[0]].clone().to_opt()
    {
        return body.exprs(db)[inner].clone().to_opt();
    }
    root
}

/// `true` if the expression is a bare path equal to `name`.
fn expr_is_ident<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    expr: crate::core::hir_def::ExprId,
    name: IdentId<'db>,
) -> bool {
    matches!(
        body.exprs(db)[expr].clone().to_opt(),
        Some(Expr::Path(Partial::Present(p))) if p.as_ident(db) == Some(name)
    )
}

/// The integer literal an expression is, if any.
fn expr_int_lit<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
    expr: crate::core::hir_def::ExprId,
) -> Option<IntegerId<'db>> {
    match body.exprs(db)[expr].clone().to_opt() {
        Some(Expr::Lit(LitKind::Int(i))) => Some(i),
        _ => None,
    }
}

/// The bare identifier a type is, if it is a single-segment path with no args.
fn bare_path_ident<'db>(db: &'db dyn HirAnalysisDb, ty: TypeId<'db>) -> Option<IdentId<'db>> {
    match ty.data(db) {
        TypeKind::Path(Partial::Present(path)) => path.as_ident(db),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;

    use super::{SubjectStep, TypeFnWfResult, type_fn_wf};
    use crate::analysis::ty::diagnostics::{TyLowerDiag, TypeFnWfError};
    use crate::hir_def::{TopLevelMod, TypeFnDef};
    use crate::test_db::HirAnalysisTestDb;

    fn find_tf<'db>(
        db: &'db HirAnalysisTestDb,
        top_mod: TopLevelMod<'db>,
        name: &str,
    ) -> TypeFnDef<'db> {
        *top_mod
            .all_type_fns(db)
            .iter()
            .find(|tf| tf.name(db).to_opt().is_some_and(|i| i.data(db) == name))
            .unwrap_or_else(|| panic!("missing `{name}` type fn"))
    }

    fn has_err(res: &TypeFnWfResult, f: impl Fn(&TypeFnWfError) -> bool) -> bool {
        res.diags.iter().any(|d| {
            matches!(d, TyLowerDiag::TypeFnIllFormed { error, .. } if f(error))
        })
    }

    /// Checks the `Bad` type fn in `src` raises a WF error matching `f` and
    /// withholds the distilled data.
    fn assert_bad(src: &str, f: impl Fn(&TypeFnWfError) -> bool) {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_wf.fe"), src);
        let (top_mod, _) = db.top_mod(file);
        let bad = find_tf(&db, top_mod, "Bad");
        let res = type_fn_wf(&db, bad);
        assert!(
            has_err(res, f),
            "expected a specific WF error, got: {:?}",
            res.diags
        );
        assert!(res.data.is_none(), "ill-formed def must not produce data");
    }

    #[test]
    fn rejects_two_const_subjects() {
        assert_bad(
            r#"
recursive type fn Bad<const M: usize, const N: usize>() -> (*) {
    match N {
        0 => u8
        _ => u16
    }
}
"#,
            |e| matches!(e, TypeFnWfError::MultipleSubjects { count: 2 }),
        );
    }

    #[test]
    fn rejects_subject_not_last() {
        assert_bad(
            r#"
recursive type fn Bad<const N: usize, F>() -> (*) {
    match N {
        0 => u8
        _ => u16
    }
}
"#,
            |e| matches!(e, TypeFnWfError::SubjectNotLast),
        );
    }

    #[test]
    fn rejects_missing_wildcard() {
        assert_bad(
            r#"
recursive type fn Bad<const N: usize>() -> (*) {
    match N {
        0 => u8
        1 => Bad<{N - 1}>
    }
}
"#,
            |e| matches!(e, TypeFnWfError::MissingWildcardArm),
        );
    }

    #[test]
    fn rejects_duplicate_arm_lit() {
        assert_bad(
            r#"
recursive type fn Bad<const N: usize>() -> (*) {
    match N {
        0 => u8
        0 => u16
        _ => Bad<{N - 1}>
    }
}
"#,
            |e| matches!(e, TypeFnWfError::DuplicateArmLit { .. }),
        );
    }

    #[test]
    fn rejects_self_call_in_zero_arm() {
        assert_bad(
            r#"
recursive type fn Bad<const N: usize>() -> (*) {
    match N {
        0 => Bad<{N - 1}>
        _ => u8
    }
}
"#,
            |e| matches!(e, TypeFnWfError::SubjectMayUnderflow),
        );
    }

    #[test]
    fn rejects_subject_plus_one() {
        assert_bad(
            r#"
recursive type fn Bad<const N: usize>() -> (*) {
    match N {
        0 => u8
        _ => Bad<{N + 1}>
    }
}
"#,
            |e| matches!(e, TypeFnWfError::SubjectNotDecreasing),
        );
    }

    #[test]
    fn rejects_composed_subject() {
        assert_bad(
            r#"
recursive type fn Bad<const N: usize>() -> (*) {
    match N {
        0 => u8
        _ => Bad<{N - 1 + 1}>
    }
}
"#,
            |e| matches!(e, TypeFnWfError::SubjectNotDecreasing),
        );
    }

    #[test]
    fn rejects_no_self_call() {
        assert_bad(
            r#"
recursive type fn Bad<const N: usize>() -> (*) {
    match N {
        0 => u8
        _ => u16
    }
}
"#,
            |e| matches!(e, TypeFnWfError::NoSelfCall),
        );
    }

    #[test]
    fn rejects_assoc_proj_in_arm() {
        assert_bad(
            r#"
recursive type fn Bad<F, const N: usize>() -> (*)
    where F: * -> *
{
    match N {
        0 => F::Assoc
        _ => Bad<F, {N - 1}>
    }
}
"#,
            |e| matches!(e, TypeFnWfError::AssocProjInArm),
        );
    }

    /// A `Bush`-style multi-self-call definition is well-formed: both self-calls
    /// distill to `Sub(1)` and the distilled data is produced.
    #[test]
    fn accepts_bush_multi_self_call() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            Utf8PathBuf::from("type_fn_bush.fe"),
            r#"
struct Pair {}
struct Comp<F, G> {}

recursive type fn Bush<const N: usize>() -> (* -> *) {
    match N {
        0 => Pair
        _ => Comp<Bush<{N - 1}>, Bush<{N - 1}>>
    }
}
"#,
        );
        let (top_mod, _) = db.top_mod(file);
        let bush = find_tf(&db, top_mod, "Bush");
        let res = type_fn_wf(&db, bush);

        assert!(res.diags.is_empty(), "unexpected diags: {:?}", res.diags);
        let data = res.data.as_ref().expect("Bush is well-formed");
        assert_eq!(data.subject_idx, 0);
        assert_eq!(data.arms.len(), 2);
        // Base arm `0 => Pair`: no self-call.
        assert!(data.arms[0].self_calls.is_empty());
        // `_ => Comp<Bush<{N-1}>, Bush<{N-1}>>`: two `Sub(1)` self-calls.
        assert_eq!(data.arms[1].self_calls.len(), 2);
        for step in &data.arms[1].self_calls {
            match step {
                SubjectStep::Sub(k) => assert_eq!(k.data(&db), &num_bigint::BigUint::from(1u32)),
                other => panic!("expected Sub(1), got {other:?}"),
            }
        }
    }
}
