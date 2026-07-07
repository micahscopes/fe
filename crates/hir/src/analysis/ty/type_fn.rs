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
use crate::analysis::ty::const_expr::ConstExpr;
use crate::analysis::ty::const_ty::{ConstTyData, ConstTyId, EvaluatedConstTy};
use crate::analysis::ty::diagnostics::{TyLowerDiag, TypeFnWfError};
use crate::analysis::ty::fold::{TyFoldable, TyFolder};
use crate::analysis::ty::trait_resolution::PredicateListId;
use crate::analysis::ty::ty_def::{
    InvalidCause, PrimTy, TyBase, TyData, TyId, find_unsaturated_type_fn,
    kind_mentions_constraint, type_fn_sig,
};
use crate::analysis::ty::ty_lower::{lower_hir_ty, lower_kind};
use crate::core::hir_def::scope_graph::ScopeId;
use crate::core::hir_def::{
    ArithBinOp, BinOp, Body, ConstGenericArgValue, Expr, GenericArg, GenericParam, IdentId,
    ImplTrait, IntegerId, ItemKind, LitKind, Partial, PathId, Stmt, TypeFnDef, TypeFnPat, TypeId,
    TypeKind,
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

                // Hole 2 (Fable steering finding 2): every `TyBase::TypeFn` head
                // in the lowered RHS must be THIS def, and the number of
                // self-referential heads must equal the syntactic self-call
                // count. This closes qualified-path / alias / foreign routes that
                // bypass the single-segment `classify_path` self-detection and
                // could otherwise form a `normalize(F) -> normalize(G) ->
                // normalize(F)` salsa cycle before normalization ever runs.
                // The cross-check reads type-fn HEADS off the lowered spine; a
                // kind error in the RHS collapses a spine into `Invalid` and
                // would drop a head, so only run it on a valid lowering (a
                // kind-ill-typed RHS is reported at its use sites already).
                if !lowered.has_invalid(db) {
                    let heads = collect_type_fn_heads(db, lowered);
                    if heads.iter().any(|d| *d != def) || heads.len() != arm.self_calls.len() {
                        self.diags.push(TyLowerDiag::TypeFnIllFormed {
                            primary: self.arm_ty_span(arm_idx),
                            error: TypeFnWfError::ForeignTypeFnRefInArm,
                        });
                    }
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
                            GenericArg::Const(c) => self.check_arm_const_arg(arm_idx, c),
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

    /// Restricts an arm-RHS `const` generic argument that appears on a
    /// non-self-call path (spec sec 1.5; Fable steering finding 1a). Only an
    /// integer literal or the bare subject `N` is allowed. Anything richer would
    /// lower to a `ConstTyData::UnEvaluated` whose later forcing re-enters the
    /// CTFE machine (`evaluate_const_ty`'s `NotIntExpr` fall-through), reopening
    /// the type-fn -> CTFE edge with no termination cover.
    fn check_arm_const_arg(&mut self, arm_idx: usize, arg: &crate::core::hir_def::ConstGenericArg<'db>) {
        let db = self.db;
        let subject = match self.subject_name {
            Some(s) => s,
            None => return,
        };
        let ConstGenericArgValue::Expr(Partial::Present(body)) = arg.value else {
            self.emit(self.arm_ty_span(arm_idx), TypeFnWfError::DisallowedArmConstArg);
            return;
        };
        let ok = match body_root_expr(db, body) {
            Some(Expr::Lit(LitKind::Int(_))) => true,
            Some(Expr::Path(Partial::Present(p))) => p.as_ident(db) == Some(subject),
            _ => false,
        };
        if !ok {
            self.emit(self.arm_ty_span(arm_idx), TypeFnWfError::DisallowedArmConstArg);
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
pub(super) fn resolve_leaf_scope<'db>(
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
pub(super) fn body_root_expr<'db>(db: &'db dyn HirAnalysisDb, body: Body<'db>) -> Option<Expr<'db>> {
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
pub(super) fn bare_path_ident<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TypeId<'db>,
) -> Option<IdentId<'db>> {
    match ty.data(db) {
        TypeKind::Path(Partial::Present(path)) => path.as_ident(db),
        _ => None,
    }
}

/// Collects every `TyBase::TypeFn` head occurring anywhere in a lowered type
/// (Fable steering finding 2 cross-check). Because type-fn occurrences are
/// always in head position of a saturated `TyApp` spine, this is exactly the set
/// of type-fn references in the type.
pub(super) fn collect_type_fn_heads<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: crate::analysis::ty::ty_def::TyId<'db>,
) -> Vec<TypeFnDef<'db>> {
    use crate::analysis::ty::visitor::{TyVisitable, TyVisitor};

    struct Collector<'db> {
        db: &'db dyn HirAnalysisDb,
        heads: Vec<TypeFnDef<'db>>,
    }
    impl<'db> TyVisitor<'db> for Collector<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_type_fn(&mut self, type_fn: TypeFnDef<'db>) {
            self.heads.push(type_fn);
        }
    }

    let mut collector = Collector { db, heads: vec![] };
    ty.visit_with(&mut collector);
    collector.heads
}

// ---------------------------------------------------------------------------
// S1.5 ground normalization (spec sec 4.1 / 4.2).
//
// Ground reduction is the ONLY reduction (spec sec 5.3 / Slice 2 preview):
// symbolic subjects stay opaque and are rejected outside type-fn bodies. The
// unfolder consumes ONLY [`TypeFnWfData`], applies each [`SubjectStep`] as a
// direct `BigUint` op (never the CTFE machine), and represents self-calls as new
// smaller SATURATED ground application nodes. Fuel is a ROOTED local counter in
// [`normalize_type_fn_app`], never part of a salsa memo key (so a subterm and a
// root reduce identically: no memo poisoning, confluence preserved).
// ---------------------------------------------------------------------------

/// The unfold-step ceiling for a single rooted application (spec sec 4.2, hard
/// compiler ceiling 4096). It is deliberately NOT part of any salsa memo key: it
/// lives only in [`normalize_type_fn_app`]'s local counter.
const TYPE_FN_UNFOLD_CEILING: usize = 4096;

/// Decomposes a saturated GROUND `recursive type fn` application into its def,
/// forwarded arguments, the ground subject value, and the subject's const type.
/// Returns `None` unless the head is a `TyBase::TypeFn`, the spine is saturated,
/// and the subject slot is an `Evaluated(LitInt)` const.
fn ground_type_fn_app<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
) -> Option<(TypeFnDef<'db>, &'db [TyId<'db>], BigUint, TyId<'db>)> {
    let (base, args) = ty.decompose_ty_app(db);
    let TyData::TyBase(TyBase::TypeFn(def)) = base.data(db) else {
        return None;
    };
    if args.len() != type_fn_sig(db, *def).arity {
        return None;
    }
    let TyData::ConstTy(cid) = args.last()?.data(db) else {
        return None;
    };
    let ConstTyData::Evaluated(EvaluatedConstTy::LitInt(value), subj_ty) = cid.data(db) else {
        return None;
    };
    Some((*def, args, value.data(db).clone(), *subj_ty))
}

/// `true` if `ty` is a saturated ground type-fn application ready to unfold.
pub(crate) fn type_fn_app_subject_is_ground<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
) -> bool {
    ground_type_fn_app(db, ty).is_some()
}

/// `true` if `scope` is (nested in) a `recursive type fn` body. Ground expansion
/// at the path-lowering site is suppressed here: inside a body a literal-subject
/// self-call (`F<{2}>`) is ground, but expanding it eagerly would re-enter
/// `type_fn_wf` (which lowers arm RHSs) and cycle. The unfolder owns self-calls.
pub(crate) fn scope_in_type_fn_body<'db>(db: &'db dyn HirAnalysisDb, scope: ScopeId<'db>) -> bool {
    let mut cur = Some(scope);
    while let Some(s) = cur {
        if matches!(
            s,
            ScopeId::Item(ItemKind::TypeFn(_)) | ScopeId::GenericParam(ItemKind::TypeFn(_), _)
        ) {
            return true;
        }
        cur = s.parent(db);
    }
    false
}

/// S2.1 position-awareness for the symbolic gate (spec sec 5, ladder S2.1).
///
/// `true` if `scope` is a STORED ADT type position: a struct/enum/contract
/// field, or a where-clause / generic-param bound / default of such an item
/// (all of which are lowered under the ADT item's own scope). A symbolic
/// `recursive type fn` application must STILL be rejected here in S2.1: a stored
/// field is never routed through the ground normalizer, and an unresolved
/// symbolic app has no defined layout. Everything else (fn signatures, where
/// clauses on fns/traits/impls, method bodies, type aliases) is NOT stored, so
/// the gate propagates the symbolic app OPAQUELY there and the obligation flows
/// to the solver.
///
/// This is scope-granular: an ADT method's signature/body is an
/// `ItemKind::Func` scope (its nearest item), so it is correctly NOT stored;
/// only the ADT's own field/where/bound positions resolve their nearest item to
/// the `Struct`/`Enum`/`Contract`. The (rare, conservatively-rejected) ADT
/// where-clause case is deliberately folded in with fields to keep the check a
/// single nearest-item test; the S2.1 discharge workload lives on fn/impl/trait
/// where clauses, which are not stored.
pub(crate) fn symbolic_type_fn_position_is_stored<'db>(scope: ScopeId<'db>) -> bool {
    matches!(
        scope.item(),
        ItemKind::Struct(_) | ItemKind::Enum(_) | ItemKind::Contract(_)
    )
}

/// The `recursive type fn` def at the head of a saturated application `ty`,
/// regardless of whether the subject is ground or symbolic. Returns `None` for
/// any other head. Used by the S2.1 soundness tripwire in the trait solver to
/// recognise an OPAQUE type-fn goal (a symbolic subject leaves the head a
/// `TyBase::TypeFn`, so `ground_type_fn_app` would decline).
pub(crate) fn type_fn_app_head<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TyId<'db>,
) -> Option<TypeFnDef<'db>> {
    let (base, _args) = ty.decompose_ty_app(db);
    match base.data(db) {
        TyData::TyBase(TyBase::TypeFn(def)) => Some(*def),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// S2.0 (a): explicit spec sec 5.1 / sec 9.9 impl-target ban.
//
// A `recursive type fn` application may not appear anywhere in an impl header:
// neither AS the impl target (self type) nor IN it (nested in the self type),
// nor in a trait-ref argument. This is banned SYMBOLIC OR GROUND, independently
// of the S1.5 `SymbolicTypeFnUnsupported` reject.
//
// Why an independent, structural check: a type-fn application is transparent
// (`RPow<F, 1>` IS `Comp<Par, F>` after one unfold step), so an impl on one
// would make coherence depend on arithmetic. For a symbolic header the S1.5
// reject already refuses lowering, but Slice 2.1 lifts that reject; for a
// GROUND header S1.5 eager-expands the self type to its normal form BEFORE any
// check sees it, which would silently accept `impl Tr for RPow<Pair, 1>` as an
// impl on `Comp<Par, Pair>` (accept-by-expansion, spec-forbidden). The ban must
// therefore run STRUCTURALLY on the impl's HIR types, before expansion, so it
// catches both and cannot be un-done by a later gate lift.
//
// v1 narrowing (documented, mirrors the S1.4 single-segment self-detection):
// only single-segment type-fn heads are recognized; a qualified/aliased head
// (`impl Tr for m::RPow<..>`) is not caught here (its symbolic form is still
// rejected by the S1.5 gate; the ground qualified form is a v1 gap tracked with
// the other qualified-path cases).
// ---------------------------------------------------------------------------

/// Where in an impl header a `recursive type fn` application was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImplHeaderTypeFnSite {
    /// In the impl's self type (`impl .. for RPow<..>` or nested within it).
    SelfType,
    /// In a trait-ref argument (`impl Tr<RPow<..>> for ..`).
    TraitRef,
}

/// Returns the site of a `recursive type fn` application in `impl_trait`'s
/// header, if any (spec sec 5.1 / sec 9.9). Structural over the HIR types, run
/// before S1.5 eager expansion, so it recognizes symbolic AND ground
/// applications. The self type is checked first.
pub(crate) fn impl_header_type_fn_site<'db>(
    db: &'db dyn HirAnalysisDb,
    impl_trait: ImplTrait<'db>,
) -> Option<ImplHeaderTypeFnSite> {
    let scope = impl_trait.scope();

    if let Some(ty) = impl_trait.type_ref(db).to_opt()
        && hir_ty_mentions_type_fn(db, ty, scope)
    {
        return Some(ImplHeaderTypeFnSite::SelfType);
    }

    if let Some(trait_ref) = impl_trait.hir_trait_ref(db).to_opt()
        && let Some(args) = trait_ref.generic_args(db)
        && generic_args_mention_type_fn(db, args, scope)
    {
        return Some(ImplHeaderTypeFnSite::TraitRef);
    }

    None
}

/// `true` if `ty` is, or structurally contains, a single-segment `recursive
/// type fn` head, resolved in `scope`.
fn hir_ty_mentions_type_fn<'db>(
    db: &'db dyn HirAnalysisDb,
    ty: TypeId<'db>,
    scope: ScopeId<'db>,
) -> bool {
    match ty.data(db) {
        TypeKind::Path(Partial::Present(path)) => {
            path_head_is_type_fn(db, *path, scope)
                || generic_args_mention_type_fn(db, path.generic_args(db), scope)
        }
        TypeKind::Ptr(Partial::Present(inner)) | TypeKind::Mode(_, Partial::Present(inner)) => {
            hir_ty_mentions_type_fn(db, *inner, scope)
        }
        TypeKind::Tuple(elems) => elems
            .data(db)
            .iter()
            .filter_map(|e| e.to_opt())
            .any(|e| hir_ty_mentions_type_fn(db, e, scope)),
        TypeKind::Array(Partial::Present(elem), _) => {
            hir_ty_mentions_type_fn(db, *elem, scope)
        }
        _ => false,
    }
}

/// `true` if any type argument in `args` is, or contains, a type-fn head.
fn generic_args_mention_type_fn<'db>(
    db: &'db dyn HirAnalysisDb,
    args: crate::core::hir_def::GenericArgListId<'db>,
    scope: ScopeId<'db>,
) -> bool {
    args.data(db).iter().any(|arg| match arg {
        GenericArg::Type(t) => t
            .ty
            .to_opt()
            .is_some_and(|ty| hir_ty_mentions_type_fn(db, ty, scope)),
        // Const args are integer subjects and assoc-type args are projections;
        // neither is a type-fn head position for the ban.
        GenericArg::Const(_) | GenericArg::AssocType(_) => false,
    })
}

/// `true` if the leaf of `path` (single-segment only, v1) resolves to a
/// `recursive type fn` item.
fn path_head_is_type_fn<'db>(
    db: &'db dyn HirAnalysisDb,
    path: PathId<'db>,
    scope: ScopeId<'db>,
) -> bool {
    // `resolve_ident_to_bucket` (via `resolve_leaf_scope`) only accepts a root
    // (single-segment) path; a qualified head is the documented v1 gap.
    if path.parent(db).is_some() {
        return false;
    }
    matches!(
        resolve_leaf_scope(db, path, scope),
        Some(ScopeId::Item(ItemKind::TypeFn(_)))
    )
}

/// Interns a ground subject `value: usize` as the canonical
/// `Evaluated(LitInt)` const, reusing the root subject's integral type so the
/// unfolder-built subject shares a `ConstTyId`/`TyId` with call-site lowering
/// (spec sec 4.1 canonical-interning requirement).
fn make_subject_ty<'db>(
    db: &'db dyn HirAnalysisDb,
    value: BigUint,
    subject_ty: TyId<'db>,
) -> TyId<'db> {
    let cid = ConstTyId::new(
        db,
        ConstTyData::Evaluated(EvaluatedConstTy::LitInt(IntegerId::new(db, value)), subject_ty),
    );
    TyId::const_ty(db, cid)
}

/// Re-distills the [`SubjectStep`] of a self-call subject argument at unfold time
/// (Fable steering: occurrence-local re-distillation, not positional matching),
/// reusing the same syntactic destructuring as [`Checker::distill_subject`]. The
/// subject body is READ, never evaluated.
fn subject_step_from_body<'db>(
    db: &'db dyn HirAnalysisDb,
    body: Body<'db>,
) -> Option<SubjectStep<'db>> {
    match body_root_expr(db, body)? {
        Expr::Bin(_, rhs, BinOp::Arith(ArithBinOp::Sub)) => {
            Some(SubjectStep::Sub(expr_int_lit(db, body, rhs)?))
        }
        Expr::Bin(_, rhs, BinOp::Arith(ArithBinOp::Div)) => {
            Some(SubjectStep::Div(expr_int_lit(db, body, rhs)?))
        }
        Expr::Lit(LitKind::Int(m)) => Some(SubjectStep::Lit(m)),
        _ => None,
    }
}

/// Applies a distilled [`SubjectStep`] to a ground subject as a direct `BigUint`
/// operation (spec sec 3.3 / 4.1). NEVER the CTFE machine.
fn apply_subject_step<'db>(
    db: &'db dyn HirAnalysisDb,
    step: SubjectStep<'db>,
    subject: &BigUint,
) -> BigUint {
    match step {
        SubjectStep::Sub(k) => {
            let k = k.data(db);
            debug_assert!(subject >= k, "WF guarantees the subject does not underflow");
            subject - k
        }
        SubjectStep::Div(k) => subject / k.data(db),
        SubjectStep::Lit(m) => m.data(db).clone(),
    }
}

/// One-step weak-head unfold of a ground type-fn application (spec sec 4.1):
/// select the arm by the ground subject, substitute type args + subject into the
/// arm RHS, and rebuild each self-call as a new smaller ground application node.
/// Pure (no fuel), so it is safe to memoize; the DAG sharing this gives is what
/// makes `Bush<8>` a handful of entries rather than an exponential tree.
#[salsa::tracked]
pub(crate) fn unfold_type_fn_step<'db>(
    db: &'db dyn HirAnalysisDb,
    app: TyId<'db>,
) -> TyId<'db> {
    let Some((def, args, subject, subject_ty)) = ground_type_fn_app(db, app) else {
        return TyId::invalid(db, InvalidCause::Other);
    };
    let result = type_fn_wf(db, def);
    let Some(data) = result.data.as_ref() else {
        return TyId::invalid(db, InvalidCause::Other);
    };

    // Arm selection: the unique literal arm for this subject, else the mandatory
    // final `_` arm (exhaustiveness is a WF invariant).
    let arm = data
        .arms
        .iter()
        .find(|a| matches!(a.pat, TypeFnPat::Lit(m) if m.data(db) == &subject))
        .or_else(|| data.arms.iter().find(|a| matches!(a.pat, TypeFnPat::Wild)));
    let Some(arm) = arm else {
        return TyId::invalid(db, InvalidCause::Other);
    };

    // Lower the arm RHS in the def's scope. Because that scope is inside a type
    // fn body, the path-lowering site leaves self-calls opaque (no eager
    // expansion, no `type_fn_wf` re-entry).
    let body_ty = lower_hir_ty(db, arm.rhs_ty, def.scope(), PredicateListId::empty_list(db));

    let mut unfolder = Unfolder {
        db,
        def,
        subst_args: args,
        subject,
        subject_ty,
        subject_idx: data.subject_idx,
    };
    body_ty.fold_with(db, &mut unfolder)
}

/// Fully normalizes a ground type-fn application to its concrete normal form
/// (spec sec 4.1). The rooted step counter (scheme A: single memo entry,
/// iterative head reduction + structural child recursion, fresh constant budget)
/// keeps fuel OUT of the memo key.
#[salsa::tracked]
pub(crate) fn normalize_type_fn_app<'db>(
    db: &'db dyn HirAnalysisDb,
    app: TyId<'db>,
) -> TyId<'db> {
    let mut steps = 0usize;
    normalize_all(db, app, &mut steps)
}

/// Reduces `ty`'s head to a non-type-fn constructor, then normalizes its
/// children, threading a single shared step counter (never reset across the
/// recursion, so confluence holds).
fn normalize_all<'db>(db: &'db dyn HirAnalysisDb, ty: TyId<'db>, steps: &mut usize) -> TyId<'db> {
    let mut cur = ty;
    while ground_type_fn_app(db, cur).is_some() {
        *steps += 1;
        if *steps > TYPE_FN_UNFOLD_CEILING {
            return TyId::invalid(
                db,
                InvalidCause::TypeFnRecursionLimit {
                    limit: TYPE_FN_UNFOLD_CEILING,
                },
            );
        }
        let stepped = unfold_type_fn_step(db, cur);
        if stepped == cur {
            break; // no progress (unreachable given strict decrease); avoid a spin
        }
        cur = stepped;
    }

    let (base, args) = cur.decompose_ty_app(db);
    if args.is_empty() {
        return cur;
    }
    let new_args: Vec<TyId<'db>> = args
        .iter()
        .map(|a| normalize_all(db, *a, steps))
        .collect();
    TyId::foldl(db, base, &new_args)
}

/// One-step substitution folder: substitutes the def's type params and subject
/// into an arm RHS, and rebuilds each self-call spine as a new smaller ground
/// application. Never folds a self-call's `UnEvaluated` subject (that would be
/// the CTFE leak); instead it re-distills the step and reinterns the result.
struct Unfolder<'db, 'a> {
    db: &'db dyn HirAnalysisDb,
    def: TypeFnDef<'db>,
    /// The root application's arguments, indexed by original param index (type
    /// params first, subject literal last).
    subst_args: &'a [TyId<'db>],
    subject: BigUint,
    subject_ty: TyId<'db>,
    subject_idx: usize,
}

impl<'db> Unfolder<'db, '_> {
    /// The smaller ground subject a self-call steps to, or `None` if the subject
    /// arg is not a recognized (WF-validated) form. The subject arithmetic is a
    /// direct `BigUint` op read off the lowered const expression; the CTFE
    /// machine is never entered.
    fn self_call_subject(&self, s: TyId<'db>) -> Option<TyId<'db>> {
        let TyData::ConstTy(cid) = s.data(self.db) else {
            return None;
        };
        match cid.data(self.db) {
            // A literal-subject self-call `F<{2}>` is already the smaller ground
            // subject.
            ConstTyData::Evaluated(EvaluatedConstTy::LitInt(_), _) => Some(s),
            // The usual `{N - k}` / `{N / k}` lowers to an abstract arith node.
            ConstTyData::Abstract(expr, _) => {
                let ConstExpr::ArithBinOp { op, rhs, .. } = expr.data(self.db) else {
                    return None;
                };
                let k = self.const_lit(*rhs)?;
                let value = match op {
                    ArithBinOp::Sub => {
                        debug_assert!(self.subject >= k, "WF guarantees no underflow");
                        &self.subject - &k
                    }
                    ArithBinOp::Div => &self.subject / &k,
                    _ => return None,
                };
                Some(make_subject_ty(self.db, value, self.subject_ty))
            }
            // Some lowerings keep the subject `UnEvaluated`; re-distill its body.
            ConstTyData::UnEvaluated { body, .. } => {
                let step = subject_step_from_body(self.db, *body)?;
                let value = apply_subject_step(self.db, step, &self.subject);
                Some(make_subject_ty(self.db, value, self.subject_ty))
            }
            _ => None,
        }
    }

    /// The `BigUint` a `ConstTy(Evaluated(LitInt))` type carries, if any.
    fn const_lit(&self, ty: TyId<'db>) -> Option<BigUint> {
        let TyData::ConstTy(cid) = ty.data(self.db) else {
            return None;
        };
        let ConstTyData::Evaluated(EvaluatedConstTy::LitInt(v), _) = cid.data(self.db) else {
            return None;
        };
        Some(v.data(self.db).clone())
    }
}

impl<'db> TyFolder<'db> for Unfolder<'db, '_> {
    fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        // Intercept a self-call spine BEFORE its subject can be folded/evaluated.
        let (base, args) = ty.decompose_ty_app(db);
        if let TyData::TyBase(TyBase::TypeFn(d)) = base.data(db) {
            if *d != self.def {
                // Tripwire: the WF cross-check forbids foreign heads.
                return TyId::invalid(db, InvalidCause::Other);
            }
            let n = args.len().saturating_sub(1);
            let mut new_args: Vec<TyId<'db>> =
                args[..n].iter().map(|a| a.fold_with(db, self)).collect();
            let Some(new_subject) = args.get(n).copied().and_then(|s| self.self_call_subject(s))
            else {
                return TyId::invalid(db, InvalidCause::Other);
            };
            new_args.push(new_subject);
            return TyId::foldl(db, base, &new_args);
        }

        match ty.data(db) {
            TyData::TyParam(p) if !p.is_effect() && p.idx < self.subst_args.len() => {
                self.subst_args[p.idx]
            }
            TyData::ConstTy(cid) => match cid.data(db) {
                // The bare subject `N` used as a whitelisted const arg: reuse the
                // root's interned subject literal (canonical interning).
                ConstTyData::TyParam(p, _) if p.idx == self.subject_idx => {
                    self.subst_args[self.subject_idx]
                }
                ConstTyData::UnEvaluated { body, .. } => match body_root_expr(db, *body) {
                    Some(Expr::Path(Partial::Present(_))) => self.subst_args[self.subject_idx],
                    Some(Expr::Lit(LitKind::Int(m))) => {
                        make_subject_ty(db, m.data(db).clone(), cid.ty(db))
                    }
                    _ => ty,
                },
                _ => ty,
            },
            _ => ty.super_fold_with(db, self),
        }
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

    /// Hole 1 (Fable steering finding 1a): a non-literal, non-`N` `const` arg on
    /// a non-self-call path in an arm RHS is rejected, so no `UnEvaluated` const
    /// can later force through the CTFE machine.
    #[test]
    fn rejects_disallowed_arm_const_arg() {
        assert_bad(
            r#"
struct Wrapper<F, const K: usize> {}

recursive type fn Bad<const N: usize>() -> (*) {
    match N {
        0 => u8
        _ => Wrapper<Bad<{N - 1}>, {N + N}>
    }
}
"#,
            |e| matches!(e, TypeFnWfError::DisallowedArmConstArg),
        );
    }

    /// Hole 1: even the bare subject inside a self-call is fine, but a helper-ish
    /// call `{f(N)}` as a plain const arg is rejected.
    #[test]
    fn rejects_helper_call_const_arg() {
        assert_bad(
            r#"
struct Wrapper<F, const K: usize> {}

recursive type fn Bad<const N: usize>() -> (*) {
    match N {
        0 => u8
        _ => Wrapper<Bad<{N - 1}>, {N - 1}>
    }
}
"#,
            |e| matches!(e, TypeFnWfError::DisallowedArmConstArg),
        );
    }

    /// Hole 2 (Fable steering finding 2): a foreign type-fn reference reached via
    /// a qualified path bypasses the single-segment `classify_path` self-detection
    /// but is caught by the lowered-RHS cross-check, so a mutual-reference cycle
    /// never reaches normalization.
    #[test]
    fn rejects_foreign_type_fn_via_qualified_path() {
        assert_bad(
            r#"
mod m {
    pub recursive type fn G<const N: usize>() -> (*) {
        match N {
            0 => u8
            _ => G<{N - 1}>
        }
    }
}

recursive type fn Bad<const N: usize>() -> (*) {
    match N {
        0 => u8
        1 => Bad<{N - 1}>
        _ => m::G<3>
    }
}
"#,
            |e| matches!(e, TypeFnWfError::ForeignTypeFnRefInArm),
        );
    }

    /// A direct mutual reference between two type fns is rejected at WF (here by
    /// the single-segment `ForeignTypeFnCall` check), so the pair never forms a
    /// `normalize(F) -> normalize(G) -> normalize(F)` cycle.
    #[test]
    fn rejects_direct_mutual_recursion() {
        assert_bad(
            r#"
recursive type fn Other<const N: usize>() -> (*) {
    match N {
        0 => u8
        _ => Bad<{N - 1}>
    }
}

recursive type fn Bad<const N: usize>() -> (*) {
    match N {
        0 => u8
        _ => Other<{N - 1}>
    }
}
"#,
            |e| matches!(e, TypeFnWfError::ForeignTypeFnCall),
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

recursive type fn Bush<const N: usize>() -> (*) {
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

    // --- S1.5 ground normalization positive tests ---

    /// The shared combinator + type-fn fixtures used by the normalization tests.
    /// All combinators are kind `*` so the arm RHSs type-check without kind
    /// bounds; the reduction shapes match spec sec 4.1.
    const NORM_FIXTURES: &str = r#"
struct Par {}
struct Pair {}
struct Comp<F, G> {}

recursive type fn RPow<F, const N: usize>() -> (*) {
    match N {
        0 => Par
        _ => Comp<RPow<F, {N - 1}>, F>
    }
}

recursive type fn LPow<F, const N: usize>() -> (*) {
    match N {
        0 => Par
        _ => Comp<F, LPow<F, {N - 1}>>
    }
}

recursive type fn Half<const N: usize>() -> (*) {
    match N {
        0 => Par
        _ => Comp<Half<{N / 2}>, Pair>
    }
}

recursive type fn Bush<const N: usize>() -> (*) {
    match N {
        0 => Pair
        _ => Comp<Bush<{N - 1}>, Bush<{N - 1}>>
    }
}
"#;

    /// Lowers the single field type of a no-generic-param probe struct. Because
    /// the field's scope is not inside a recursive type fn body, the ground
    /// type-fn application in it is eager-expanded at path lowering, so this
    /// returns the concrete normal form.
    fn probe_field_pretty(src: &str, struct_name: &str) -> String {
        use super::collect_type_fn_heads;
        use crate::analysis::ty::adt_def::AdtRef;
        use crate::analysis::ty::ty_def::TyId;
        use crate::hir_def::ItemKind;

        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_norm.fe"), src);
        let (top_mod, _) = db.top_mod(file);
        let structs = top_mod.all_structs(&db);
        let s = structs
            .iter()
            .copied()
            .find(|s| s.name(&db).to_opt().is_some_and(|i| i.data(&db) == struct_name))
            .unwrap_or_else(|| panic!("missing probe struct `{struct_name}`"));
        let adt = AdtRef::try_from_item(ItemKind::Struct(s)).unwrap().as_adt(&db);
        let ty = TyId::adt(&db, adt);
        let field = ty.field_types(&db)[0];
        // No `TyBase::TypeFn` may survive normalization.
        assert!(
            collect_type_fn_heads(&db, field).is_empty(),
            "normal form still contains a type fn: {}",
            field.pretty_print(&db)
        );
        field.pretty_print(&db).to_string()
    }

    #[test]
    fn normalizes_rpow_pair_3() {
        let src = format!("{NORM_FIXTURES}\nstruct Probe {{ p: RPow<Pair, 3> }}\n");
        assert_eq!(
            probe_field_pretty(&src, "Probe"),
            "Comp<Comp<Comp<Par, Pair>, Pair>, Pair>"
        );
    }

    #[test]
    fn normalizes_lpow_pair_2() {
        let src = format!("{NORM_FIXTURES}\nstruct Probe {{ p: LPow<Pair, 2> }}\n");
        assert_eq!(
            probe_field_pretty(&src, "Probe"),
            "Comp<Pair, Comp<Pair, Par>>"
        );
    }

    #[test]
    fn normalizes_half_div_4() {
        let src = format!("{NORM_FIXTURES}\nstruct Probe {{ p: Half<4> }}\n");
        assert_eq!(
            probe_field_pretty(&src, "Probe"),
            "Comp<Comp<Comp<Par, Pair>, Pair>, Pair>"
        );
    }

    #[test]
    fn normalizes_bush_multi_self_call_2() {
        let src = format!("{NORM_FIXTURES}\nstruct Probe {{ p: Bush<2> }}\n");
        assert_eq!(
            probe_field_pretty(&src, "Probe"),
            "Comp<Comp<Pair, Pair>, Comp<Pair, Pair>>"
        );
    }

    /// A symbolic type-fn application in an ADT FIELD position (here a struct
    /// field mentioning the struct's own const param) stays rejected under S2.1:
    /// the gate is position-aware and keeps rejecting stored positions (a stored
    /// field is never routed through the normalizer, and an unresolved symbolic
    /// app has no defined layout). Fn-signature / where-clause / body positions
    /// are lifted to opaque propagation elsewhere (see the S2.1 tests below). We
    /// assert the diagnostic surfaces from the full analysis pass.
    #[test]
    fn rejects_symbolic_type_fn_outside_body() {
        use crate::test_db::format_diagnostics;

        let src = format!(
            "{NORM_FIXTURES}\nstruct Wrap<const M: usize> {{ p: RPow<Pair, M> }}\n"
        );
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_symbolic.fe"), &src);
        let (top_mod, _) = db.top_mod(file);
        let diags = db.run_on_top_mod(top_mod);
        let rendered = format_diagnostics(&db, &diags);
        assert!(
            rendered.contains("symbolic type-fn application not yet supported"),
            "expected a symbolic-type-fn diagnostic, got:\n{rendered}"
        );
    }

    // --- S2.0 (a): impl-target ban (spec sec 5.1 / sec 9.9) ---

    /// A generic `impl` whose self type is a SYMBOLIC `recursive type fn`
    /// application (`RPow<F, N>`) is rejected with the named ban diagnostic. This
    /// is the headline S2.0 check: it is independent of the S1.5 symbolic reject
    /// (which Slice 2.1 lifts), so lifting that reject cannot un-ban the impl.
    #[test]
    fn rejects_impl_on_symbolic_type_fn_application() {
        use crate::test_db::format_diagnostics;

        let src = format!(
            "{NORM_FIXTURES}\ntrait LScan {{}}\n\
             impl<F, const N: usize> LScan for RPow<F, N> {{}}\n"
        );
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_impl_ban_sym.fe"), &src);
        let (top_mod, _) = db.top_mod(file);
        let diags = db.run_on_top_mod(top_mod);
        let rendered = format_diagnostics(&db, &diags);
        assert!(
            rendered.contains("cannot implement a trait for a `recursive type fn` application"),
            "expected the impl-target ban diagnostic, got:\n{rendered}"
        );
    }

    /// The GROUND-header case is banned too (spec's expand-vs-reject decision:
    /// REJECT). Without the structural ban, `impl LScan for RPow<Pair, 1>` would
    /// eager-expand at lowering and silently register as an impl on the normal
    /// form `Comp<Par, Pair>` (accept-by-expansion), making impl-header meaning
    /// depend on normalization timing.
    #[test]
    fn rejects_impl_on_ground_type_fn_application() {
        use crate::test_db::format_diagnostics;

        let src = format!(
            "{NORM_FIXTURES}\ntrait LScan {{}}\nimpl LScan for RPow<Pair, 1> {{}}\n"
        );
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_impl_ban_ground.fe"), &src);
        let (top_mod, _) = db.top_mod(file);
        let diags = db.run_on_top_mod(top_mod);
        let rendered = format_diagnostics(&db, &diags);
        assert!(
            rendered.contains("cannot implement a trait for a `recursive type fn` application"),
            "expected the impl-target ban diagnostic on a ground header, got:\n{rendered}"
        );
    }

    /// A type-fn application NESTED in the self type (`Wrapper<RPow<F, N>>`) is
    /// also banned ("in impl headers anywhere", spec sec 9.9).
    #[test]
    fn rejects_impl_on_nested_type_fn_application() {
        use crate::test_db::format_diagnostics;

        let src = format!(
            "{NORM_FIXTURES}\nstruct Wrapper<G> {{}}\ntrait LScan {{}}\n\
             impl<F, const N: usize> LScan for Wrapper<RPow<F, N>> {{}}\n"
        );
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_impl_ban_nested.fe"), &src);
        let (top_mod, _) = db.top_mod(file);
        let diags = db.run_on_top_mod(top_mod);
        let rendered = format_diagnostics(&db, &diags);
        assert!(
            rendered.contains("cannot implement a trait for a `recursive type fn` application"),
            "expected the impl-target ban diagnostic on a nested header, got:\n{rendered}"
        );
    }

    /// S2.0 (c): mono-time normalization. Exercises the SECOND S1.5 containment
    /// line (`TypeNormalizer::fold_ty`, `normalize.rs`), which is exactly the
    /// hook MIR instance substitution relies on: every instance type is routed
    /// through `normalize_ty` (`instantiate_normalized_ty` /
    /// `RuntimeInstance::normalized_ty` / `runtime/lower/type_info.rs`) AFTER
    /// generic-arg substitution, so a symbolic app that becomes ground at
    /// instantiation reduces to its normal form before reaching MIR (the
    /// `stable_key.rs` `TyBase::TypeFn` tripwire stays the backstop). Here the
    /// post-substitution ground app is HAND-BUILT (bypassing path lowering,
    /// which would eager-expand it at the first containment line) and fed to
    /// `normalize_ty`, proving that path reduces it.
    #[test]
    fn normalize_ty_reduces_ground_type_fn_app() {
        use super::{collect_type_fn_heads, make_subject_ty};
        use crate::analysis::ty::adt_def::AdtRef;
        use crate::analysis::ty::normalize::normalize_ty;
        use crate::analysis::ty::trait_resolution::PredicateListId;
        use crate::analysis::ty::ty_def::{PrimTy, TyBase, TyData, TyId};
        use crate::hir_def::ItemKind;

        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_mono.fe"), NORM_FIXTURES);
        let (top_mod, _) = db.top_mod(file);

        let rpow = find_tf(&db, top_mod, "RPow");
        let pair_struct = top_mod
            .all_structs(&db)
            .iter()
            .copied()
            .find(|s| s.name(&db).to_opt().is_some_and(|i| i.data(&db) == "Pair"))
            .expect("missing Pair");
        let pair_ty = TyId::adt(
            &db,
            AdtRef::try_from_item(ItemKind::Struct(pair_struct))
                .unwrap()
                .as_adt(&db),
        );
        let usize_ty = TyId::new(&db, TyData::TyBase(TyBase::Prim(PrimTy::Usize)));
        let subject = make_subject_ty(&db, num_bigint::BigUint::from(3u32), usize_ty);

        // Assemble the saturated ground application `RPow<Pair, 3>` directly.
        let base = TyId::type_fn(&db, rpow);
        let app = TyId::foldl(&db, base, &[pair_ty, subject]);
        // It is a live, unexpanded `TyBase::TypeFn` head.
        assert!(
            !collect_type_fn_heads(&db, app).is_empty(),
            "hand-built app should carry a live type-fn head"
        );

        // The MIR-consumed `normalize_ty` reduces it to the concrete normal form.
        let normal = normalize_ty(
            &db,
            app,
            top_mod.scope(),
            PredicateListId::empty_list(&db),
        );
        assert!(
            collect_type_fn_heads(&db, normal).is_empty(),
            "normalized type must not carry a type-fn head: {}",
            normal.pretty_print(&db)
        );
        assert_eq!(
            normal.pretty_print(&db),
            "Comp<Comp<Comp<Par, Pair>, Pair>, Pair>"
        );
    }

    /// S2.0 (b): the `recursive type fn`'s `where` clause is its application
    /// precondition (spec sec 2.4), carried as a first-class WF constraint of any
    /// SURVIVING type-fn application. `ty_constraints` on a hand-built
    /// `RPow<Pair, 3>` (whose def declares `where F: Marker`) yields the
    /// precondition instantiated at the arg (`Pair: Marker`), which the ordinary
    /// `check_ty_wf` machinery then discharges. This is the SSOT S2.1's symbolic
    /// obligations consume; it is a no-op at reachable S2.0 positions (a ground
    /// application eager-expands before this is consulted; a symbolic one is
    /// rejected). A `where`-less type fn yields the empty list, pinning that the
    /// constraint comes from the clause, not the head.
    #[test]
    fn ty_constraints_carries_type_fn_where_clause() {
        use super::make_subject_ty;
        use crate::analysis::ty::adt_def::AdtRef;
        use crate::analysis::ty::trait_resolution::constraint::ty_constraints;
        use crate::analysis::ty::ty_def::{PrimTy, TyBase, TyData, TyId};
        use crate::hir_def::ItemKind;

        let src = r#"
trait Marker {}
struct Par {}
struct Pair {}
struct Comp<F, G> {}

recursive type fn RPow<F, const N: usize>() -> (*) where F: Marker {
    match N {
        0 => Par
        _ => Comp<RPow<F, {N - 1}>, F>
    }
}

recursive type fn Bare<F, const N: usize>() -> (*) {
    match N {
        0 => Par
        _ => Comp<Bare<F, {N - 1}>, F>
    }
}
"#;
        // Assembles the saturated ground app `<tf><Pair, 3>` directly (bypassing
        // path lowering, which would eager-expand it).
        fn build_app<'db>(
            db: &'db HirAnalysisTestDb,
            top_mod: TopLevelMod<'db>,
            tf_name: &str,
        ) -> TyId<'db> {
            let tf = find_tf(db, top_mod, tf_name);
            assert!(
                type_fn_wf(db, tf).data.is_some(),
                "`{tf_name}` should be well-formed"
            );
            let pair_struct = top_mod
                .all_structs(db)
                .iter()
                .copied()
                .find(|s| s.name(db).to_opt().is_some_and(|i| i.data(db) == "Pair"))
                .expect("missing Pair");
            let pair_ty = TyId::adt(
                db,
                AdtRef::try_from_item(ItemKind::Struct(pair_struct))
                    .unwrap()
                    .as_adt(db),
            );
            let usize_ty = TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::Usize)));
            let subject = make_subject_ty(db, num_bigint::BigUint::from(3u32), usize_ty);
            TyId::foldl(db, TyId::type_fn(db, tf), &[pair_ty, subject])
        }

        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_precond.fe"), src);
        let (top_mod, _) = db.top_mod(file);

        // The `where F: Marker` fn yields the instantiated precondition.
        let preds = ty_constraints(&db, build_app(&db, top_mod, "RPow"));
        let rendered = preds.pretty_print(&db);
        assert!(
            !preds.is_empty(&db) && rendered.contains("Marker") && rendered.contains("Pair"),
            "expected the instantiated precondition `Pair: Marker`, got: {rendered}"
        );

        // The `where`-less fn yields nothing (the constraint is the clause).
        assert!(
            ty_constraints(&db, build_app(&db, top_mod, "Bare")).is_empty(&db),
            "a `where`-less type fn must carry no precondition"
        );
    }

    // --- S2.1: symbolic propagation + assumption-only discharge ---

    /// Shared S2.1 fixture. `RPow` is a `*`-returning recursion; `Marker` has
    /// impls on every combinator (`Par`/`Pair`/`Comp`), so every ground normal
    /// form of `RPow<_, n>` is `Marker`. `Requires<T> where T: Marker` is a
    /// wrapper whose USE in a signature FORCES the obligation `T: Marker` to be
    /// discharged (an empty struct is enough: its where clause is a WF
    /// precondition of the applied type). The gate is exercised in a fn-signature
    /// position, so the symbolic `RPow<F, M>` propagates opaquely.
    const S21_FIXTURES: &str = r#"
struct Par {}
struct Pair {}
struct Comp<F, G> {}

trait Marker {}
impl Marker for Par {}
impl Marker for Pair {}
impl<F, G> Marker for Comp<F, G> {}

struct Requires<T> where T: Marker {}

recursive type fn RPow<F, const N: usize>() -> (*) {
    match N {
        0 => Par
        _ => Comp<RPow<F, {N - 1}>, F>
    }
}
"#;

    fn s21_diags(tail: &str) -> String {
        use crate::test_db::format_diagnostics;
        let src = format!("{S21_FIXTURES}\n{tail}\n");
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_s21.fe"), &src);
        let (top_mod, _) = db.top_mod(file);
        let diags = db.run_on_top_mod(top_mod);
        format_diagnostics(&db, &diags)
    }

    /// POSITIVE: a generic fn whose signature forces `RPow<F, M>: Marker` (via
    /// the `Requires` wrapper on a parameter) type-checks when the caller-written
    /// `where RPow<F, M>: Marker` assumption is present. The symbolic obligation
    /// reaches the solver and is discharged from that assumption; zero new proof
    /// power.
    #[test]
    fn s21_symbolic_obligation_discharged_by_assumption() {
        let rendered = s21_diags(
            "fn use_it<F, const M: usize>(x: Requires<RPow<F, M>>) where RPow<F, M>: Marker {}",
        );
        assert!(
            rendered.is_empty(),
            "the symbolic obligation should be discharged by the assumption, but got:\n{rendered}"
        );
    }

    /// NEGATIVE (missing assumption): the SAME fn WITHOUT the `where RPow<F, M>:
    /// Marker` bound fails. The opaque type-fn head has no impl candidate (the
    /// S2.0 ban) and no assumption, so the obligation is UnSat and the ordinary
    /// trait-bound diagnostic fires. This pins the conservatism direction: the
    /// assumption is load-bearing, not decorative.
    #[test]
    fn s21_symbolic_obligation_fails_without_assumption() {
        let rendered = s21_diags("fn use_it<F, const M: usize>(x: Requires<RPow<F, M>>) {}");
        assert!(
            rendered.contains("is not satisfied") || rendered.contains("doesn't implement"),
            "expected a trait-bound-not-satisfied diagnostic without the assumption, got:\n{rendered}"
        );
    }

    /// AUDIT: an opaque symbolic type-fn application flows through a fn parameter
    /// type, a return type, and a trivial body without ICE, and with no spurious
    /// diagnostic (RPow here has no where clause, so its use carries no
    /// precondition). Exercises the passes the steering flagged as potentially
    /// touching a symbolic head now that it reaches signatures/bodies (ty_check,
    /// signature WF, print). MIR is not reached: a generic definition is not
    /// monomorphized by the analysis pass.
    #[test]
    fn s21_opaque_head_flows_through_signature_no_ice() {
        let rendered = s21_diags(
            "fn id_opaque<F, const M: usize>(x: RPow<F, M>) -> RPow<F, M> { return x }",
        );
        assert!(
            rendered.is_empty(),
            "an opaque type-fn app in a signature/body must type-check cleanly, got:\n{rendered}"
        );
    }

    /// CROSS-CHECK (the core S2.1 "gate, don't select" soundness test). Two legs.
    ///
    /// GATE leg (symbolic): the opaque obligation `Marker(RPow<F, N>)` (F, N are
    /// RPow's own rigid params) is Satisfied ONLY from the assumption
    /// `RPow<F, N>: Marker`, and its witness carries
    /// `ImplementorOrigin::Assumption` (never an impl: the S2.0 ban leaves no
    /// impl candidate whose self-type head is the opaque type fn, and there is no
    /// induction engine yet). Drop the assumption and it is UnSat. This is the
    /// only S2.1 discharge route, and the permanent user escape hatch.
    ///
    /// SELECT leg (ground): for n in {0, 1, 2, 4, 7}, ground resolution of the
    /// UN-normalized `Marker(RPow<Pair, n>)` and of the pre-normalized
    /// `Marker(NF_n)` yield the IDENTICAL unique implementor. Equal `ImplementorId`
    /// means equal `(ImplementorId, ImplementorOrigin, SelDiscriminator)` (the
    /// latter two are pure functions of the implementor), and `select_impl`
    /// returns `Selection::Unique` of it on BOTH forms. Since the instantiated
    /// assumption becomes exactly `RPow<Pair, n>: Marker` at a call site, the
    /// gate can never diverge from what ground selection picks on the normal form.
    #[test]
    fn s21_cross_check_gate_matches_ground_select() {
        use super::{make_subject_ty, normalize_type_fn_app};
        use crate::analysis::ty::adt_def::AdtRef;
        use crate::analysis::ty::trait_def::{ImplementorOrigin, TraitInstId};
        use crate::analysis::ty::trait_resolution::{
            GoalSatisfiability, PredicateListId, Selection, TraitSolveCx, is_goal_satisfiable,
        };
        use crate::analysis::ty::ty_def::{PrimTy, TyBase, TyData, TyId};
        use crate::analysis::ty::ty_lower::collect_generic_params;
        use crate::hir_def::{GenericParamOwner, ItemKind};

        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_s21_xcheck.fe"), S21_FIXTURES);
        let (top_mod, _) = db.top_mod(file);

        let rpow = find_tf(&db, top_mod, "RPow");
        let marker = *top_mod
            .all_traits(&db)
            .iter()
            .find(|t| t.name(&db).to_opt().is_some_and(|i| i.data(&db) == "Marker"))
            .expect("missing Marker trait");
        let adt_ty = |name: &str| {
            let s = top_mod
                .all_structs(&db)
                .iter()
                .copied()
                .find(|s| s.name(&db).to_opt().is_some_and(|i| i.data(&db) == name))
                .unwrap_or_else(|| panic!("missing struct `{name}`"));
            TyId::adt(&db, AdtRef::try_from_item(ItemKind::Struct(s)).unwrap().as_adt(&db))
        };
        let pair_ty = adt_ty("Pair");

        // --- GATE leg: the symbolic obligation is dischargeable only by the
        // assumption, with Assumption provenance. ---
        let params = collect_generic_params(&db, GenericParamOwner::TypeFn(rpow));
        let f_param = params.param_by_original_idx(&db, 0).expect("RPow.F");
        let n_param = params.param_by_original_idx(&db, 1).expect("RPow.N");
        let sym_app = TyId::foldl(&db, TyId::type_fn(&db, rpow), &[f_param, n_param]);
        // The head stays a live, opaque `TyBase::TypeFn` application.
        assert!(
            super::type_fn_app_head(&db, sym_app).is_some(),
            "symbolic app must keep a live type-fn head"
        );
        let sym_goal = TraitInstId::new_simple(&db, marker, vec![sym_app]);

        let cx_no_assume = TraitSolveCx::new(&db, rpow.scope());
        assert!(
            matches!(
                is_goal_satisfiable(&db, cx_no_assume, sym_goal),
                GoalSatisfiability::UnSat(_)
            ),
            "without the assumption the opaque obligation must be UnSat (no impl candidate)"
        );

        let assumptions = PredicateListId::new(&db, vec![sym_goal]);
        let cx_assume = TraitSolveCx::new(&db, rpow.scope()).with_assumptions(assumptions);
        match is_goal_satisfiable(&db, cx_assume, sym_goal) {
            GoalSatisfiability::Satisfied(sol) => assert!(
                matches!(sol.value.implementor.origin(&db), ImplementorOrigin::Assumption),
                "the symbolic discharge must come from the assumption, not an impl"
            ),
            other => panic!("expected Satisfied-from-assumption, got {other:?}"),
        }

        // --- SELECT leg: ground resolution at each n agrees on the normal form. ---
        let usize_ty = TyId::new(&db, TyData::TyBase(TyBase::Prim(PrimTy::Usize)));
        let cx = TraitSolveCx::new(&db, top_mod.scope());
        for n in [0u32, 1, 2, 4, 7] {
            let subject = make_subject_ty(&db, num_bigint::BigUint::from(n), usize_ty);
            let app = TyId::foldl(&db, TyId::type_fn(&db, rpow), &[pair_ty, subject]);
            let nf = normalize_type_fn_app(&db, app);
            assert!(
                super::collect_type_fn_heads(&db, nf).is_empty(),
                "NF_{n} still carries a type-fn head: {}",
                nf.pretty_print(&db)
            );

            let goal_unnorm = TraitInstId::new_simple(&db, marker, vec![app]);
            let goal_norm = TraitInstId::new_simple(&db, marker, vec![nf]);

            let impl_unnorm = match is_goal_satisfiable(&db, cx, goal_unnorm) {
                GoalSatisfiability::Satisfied(sol) => sol.value.implementor,
                other => panic!("Marker(RPow<Pair, {n}>) must be Satisfied ground, got {other:?}"),
            };
            let impl_norm = match is_goal_satisfiable(&db, cx, goal_norm) {
                GoalSatisfiability::Satisfied(sol) => sol.value.implementor,
                other => panic!("Marker(NF_{n}) must be Satisfied ground, got {other:?}"),
            };

            // Equal ImplementorId => equal origin + SelDiscriminator.
            assert_eq!(
                impl_unnorm, impl_norm,
                "gate/select divergence at n={n}: the un-normalized ground app and the \
                 normal form selected different implementors"
            );
            assert!(
                matches!(impl_norm.origin(&db), ImplementorOrigin::Hir(_)),
                "ground selection at n={n} must pin a real impl, not an assumption"
            );

            // `select_impl` returns Unique(that impl) on BOTH forms.
            for (label, goal) in [("normal form", goal_norm), ("un-normalized", goal_unnorm)] {
                match cx.select_impl(&db, goal) {
                    Selection::Unique(sel) => assert_eq!(
                        sel, impl_norm,
                        "select_impl on the {label} at n={n} picked a different impl"
                    ),
                    other => panic!("select_impl on the {label} at n={n} was not Unique: {other:?}"),
                }
            }
        }
    }

    /// Guard against over-firing: an ordinary impl on a plain ADT whose fields
    /// happen to mention nothing type-fn-related is NOT banned. (`Comp` is a
    /// combinator; impls on it are exactly what the spec directs users to.)
    #[test]
    fn allows_impl_on_combinator() {
        use crate::test_db::format_diagnostics;

        let src = format!(
            "{NORM_FIXTURES}\ntrait LScan {{}}\nimpl<F, G> LScan for Comp<F, G> {{}}\n"
        );
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(Utf8PathBuf::from("type_fn_impl_ok.fe"), &src);
        let (top_mod, _) = db.top_mod(file);
        let diags = db.run_on_top_mod(top_mod);
        let rendered = format_diagnostics(&db, &diags);
        assert!(
            !rendered.contains("cannot implement a trait for a `recursive type fn` application"),
            "impl on a plain combinator must not trip the ban, got:\n{rendered}"
        );
    }
}
