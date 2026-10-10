use crate::core::semantic::Provision;
use common::ingot::IngotKind;
use either::Either;
use num_bigint::{BigInt, BigUint, Sign};
use num_traits::ToPrimitive;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec1::SmallVec;

use crate::core::hir_def::{
    ArithBinOp, BinOp, CallArg as HirCallArg, CallableDef, Cond, CondId, Expr, ExprId, FieldIndex,
    IdentId, IntegerId, LitKind, LogicalBinOp, Partial, PatId, PathId, PathKind, Stmt, StmtId,
    UnOp, VariantKind, WithBinding,
};
use crate::span::DynLazySpan;

use super::{
    CodeRegionIntrinsicKind, ConstIntrinsicKind, ConstRef, EffectArgLayoutView,
    PathResolutionScopes, PatternLayoutContext, RecordLike, Typeable, ValuePathRef,
    effect_env::{
        FamilyKeyedEntry, FrameLookupResult, MatchedForwarder, MatchedKeyedEntry, MatchedWitness,
    },
    env::{
        EffectOrigin, EffectParamSite, ExprProp, LocalBinding, ParamSite, PendingPrimitiveOp,
        ProvidedEffect, TraitObligation, TraitObligationOrigin, TyCheckEnv,
    },
    path::ResolvedPathInBody,
    ty_may_be_code_region_token,
};
use crate::analysis::place::{Place, PlaceBase, PlaceProjection};
use crate::analysis::semantic::{RuntimeSizeError, runtime_size_bytes};
use crate::analysis::ty::{
    adt_def::{
        AdtRef, instantiate_adt_field_shape, instantiate_adt_field_source_for_concrete_demand,
    },
    assoc_const::{AssocConstUse, InherentConstUse},
    canonical::{Canonicalized, Solution},
    const_ty::{BodyHoleSite, HoleAnchor, LoweringContext, instantiate_inherent_const_decl_ty},
    corelib::{
        is_core_evidence_ty, resolve_core_range_types, resolve_core_trait, resolve_lib_func_path,
        resolve_lib_trait_path, resolve_lib_type_path,
    },
    diagnostics::{BodyDiag, FuncBodyDiag, MustUseSubject},
    effects::{
        BarrierReason, EffectBarrier, EffectFamily, EffectKeyKind, EffectPatternKey, EffectQuery,
        EffectQueryMode, EffectRequirementKey, EffectWitness, ForwardedEffectKey, PatternSlotKind,
        PatternSlots, StoredEffectKey, StoredTraitKey, StoredTypeKey, TraitPatternKey,
        TypePatternKey, WitnessTransport,
        elaborate::{
            build_barrier_pattern_for_with_key,
            build_conservative_same_family_barrier_pattern_in_scope, build_effect_query_for_call,
            contains_projection_or_invalid_query_state, effect_requirement_decls_for_callable,
            finalize_stored_effect_key, query_contains_unresolved_inference,
        },
        match_::{
            KeyMatchCommit, apply_key_match_commit, instantiate_trait_pattern_in,
            instantiate_trait_pattern_in_with_bindings, query_matches_forwarder,
            query_matches_witness,
        },
        place_effect_provider_param_index_map, stored_value_contains_implicit_layout_params,
        stored_value_contains_out_of_scope_params,
    },
    fold::{TyFoldable as _, TyFolder},
    normalize::normalize_with_trait_evidence,
    provider::{
        ProviderLayoutEvidence, ProviderTransport, provider_semantics,
        provider_semantics_for_specialized_call,
    },
    trait_def::{TraitInstId, named_impls_for_member},
    trait_resolution::{GoalSatisfiability, PredicateListId, TraitGoalSolution},
    ty_check::callable::{Callable, EffectProviderProvenance, EffectProviderSpecialization},
    ty_def::{CapabilityKind, PrimTy, TyBase, TyData, prim_int_bits},
    unify::UnificationTable,
};
use crate::analysis::{
    HirAnalysisDb, Spanned,
    name_resolution::{
        EarlyNameQueryId, ExpectedPathKind, NameDomain, NameResBucket, NameResolutionError,
        PathRes, PathResErrorKind, QueryDirective,
        diagnostics::PathResDiag,
        is_scope_visible_from,
        method_selection::{
            MethodCandidate, MethodSelectionError, select_method_candidate,
            select_trait_method_candidates,
        },
        resolve_name_res_with_minter, resolve_query,
    },
    place::resolve_place_field,
    semantic::{
        ConstRepr, EvalOutcome, SemConstScalar, SemConstValue, SemOrigin, eval_const_ref,
        instance::resolve_semantic_const_ref, int_const,
    },
    ty::{
        LayoutBundlePathStep,
        const_expr::ConstExpr,
        const_ty::{ConstTyData, ConstTyId, const_ty_from_sem_const, try_eval_const_int_expr},
        normalize::normalize_ty,
        ty_check::{BodyOwner, RecordInitLowering, TyChecker, path::RecordInitChecker},
        ty_def::{InvalidCause, TyId},
        ty_lower::{
            callable_input_carrier_projected_layout_ty, callable_input_layout_origin_ty,
            callable_input_layout_projection_paths, callable_input_projected_layout_ty,
            instantiate_callable_effect_layout_args, instantiate_callable_projection_layout_args,
        },
    },
};
use crate::hir_def::{FieldParent, ItemKind, scope_graph::ScopeId};
use crate::semantic::{
    FieldStorageLayout, LayoutProjection, LayoutViewError, LayoutViewKind, ProviderBinding,
};
use common::indexmap::IndexMap;

#[derive(Debug, Clone, Copy)]
pub(super) enum TypeEffectBindingMatch<'db> {
    Direct {
        given: TyId<'db>,
    },
    Provider {
        resolution: ProviderTargetResolution<'db>,
    },
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ProviderTargetResolution<'db> {
    target_ty: TyId<'db>,
    target_seed_ty: TyId<'db>,
    handle_proof: Option<(TraitInstId<'db>, Solution<TraitGoalSolution<'db>>)>,
    effect_ref_proof: Option<(TraitInstId<'db>, Solution<TraitGoalSolution<'db>>)>,
    effect_ref_mut_proof: Option<(TraitInstId<'db>, Solution<TraitGoalSolution<'db>>)>,
}

fn layout_projections_from_callable_path(
    path: &[LayoutBundlePathStep],
) -> Option<Vec<LayoutProjection>> {
    let mut projections = Vec::new();
    let mut steps = path.iter();
    while let Some(step) = steps.next() {
        match *step {
            LayoutBundlePathStep::Field(field) => {
                projections.push(LayoutProjection::Field(field));
            }
            LayoutBundlePathStep::Variant(variant) => {
                let LayoutBundlePathStep::Field(field) = *steps.next()? else {
                    return None;
                };
                projections.push(LayoutProjection::VariantField { variant, field });
            }
            LayoutBundlePathStep::Index => {
                projections.push(LayoutProjection::Index);
            }
            LayoutBundlePathStep::ConstParam(param) => {
                projections.push(LayoutProjection::ConstParam(param));
            }
        }
    }
    Some(projections)
}

impl<'db> ProviderTargetResolution<'db> {
    fn direct(target_ty: TyId<'db>) -> Self {
        Self {
            target_ty,
            target_seed_ty: target_ty,
            handle_proof: None,
            effect_ref_proof: None,
            effect_ref_mut_proof: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct KeyedWitnessBuildScope<'db> {
    pub(super) scope: ScopeId<'db>,
    pub(super) assumptions: PredicateListId<'db>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct KeyedWitnessBuildOptions<'db> {
    pub(super) scope: KeyedWitnessBuildScope<'db>,
    pub(super) emit_diag: bool,
    pub(super) mode: WitnessBuildMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WitnessBuildMode {
    ExplicitKeyedWith,
    SeededRequirement,
}

#[derive(Debug, Clone)]
enum EffectEvidence<'db> {
    Keyed {
        provider: ProvidedEffect<'db>,
        key_kind: EffectKeyKind,
        target_ty: Option<TyId<'db>>,
        commit: EffectCommitPlan<'db>,
        arg_style: EffectArgStyle,
        layout_view: EffectArgLayoutView,
    },
    UnkeyedType {
        provider: ProvidedEffect<'db>,
        commit: EffectCommitPlan<'db>,
        arg_style: EffectArgStyle,
        layout_view: EffectArgLayoutView,
    },
    UnkeyedTrait {
        provider: ProvidedEffect<'db>,
        commit: EffectCommitPlan<'db>,
        arg_style: EffectArgStyle,
        layout_view: EffectArgLayoutView,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectArgStyle {
    Place,
    TempPlace,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssignLhsStatus {
    Assignable,
    Immutable,
    NonAssignable,
}

struct MutableIndexTarget<'db> {
    prop: ExprProp<'db>,
    target_ty: TyId<'db>,
    trait_lowered: bool,
}

#[derive(Debug, Clone, Default)]
pub(super) struct EffectCommitPlan<'db> {
    key_match: Option<KeyMatchCommit<'db>>,
    trait_solutions: SmallVec<[(TraitInstId<'db>, Solution<TraitGoalSolution<'db>>); 2]>,
    provider_resolution: Option<ProviderTargetResolution<'db>>,
    extra_unifications: SmallVec<[(TyId<'db>, TyId<'db>); 4]>,
}

#[derive(Debug, Clone)]
enum EffectResolution<'db> {
    Chosen(Box<EffectEvidence<'db>>),
    BlockedByBarrier,
    Missing,
    Ambiguous,
}

fn evidence_provider<'db>(evidence: &EffectEvidence<'db>) -> ProvidedEffect<'db> {
    match evidence {
        EffectEvidence::Keyed { provider, .. }
        | EffectEvidence::UnkeyedType { provider, .. }
        | EffectEvidence::UnkeyedTrait { provider, .. } => *provider,
    }
}

pub(super) enum PendingPrimitiveOpResolution {
    Pending,
    Resolved,
    Done,
}

impl<'db> TyChecker<'db> {
    fn code_region_intrinsic_kind(
        &self,
        callable_def: CallableDef<'db>,
    ) -> Option<CodeRegionIntrinsicKind> {
        let CallableDef::Func(func) = callable_def else {
            return None;
        };
        let offset = resolve_lib_func_path(
            self.db,
            self.env.scope(),
            "std::evm::intrinsic::code_region_offset",
        );
        let len = resolve_lib_func_path(
            self.db,
            self.env.scope(),
            "std::evm::intrinsic::code_region_len",
        );
        Some(if Some(func) == offset {
            CodeRegionIntrinsicKind::Offset
        } else if Some(func) == len {
            CodeRegionIntrinsicKind::Len
        } else {
            return None;
        })
    }

    pub(super) fn code_region_method_kind(
        &self,
        receiver_ty: TyId<'db>,
        method_name: IdentId<'db>,
    ) -> Option<CodeRegionIntrinsicKind> {
        let evm_ty = resolve_lib_type_path(self.db, self.env.scope(), "std::evm::Evm")?;
        if receiver_ty != evm_ty {
            return None;
        }
        let name = method_name.data(self.db);
        Some(if name == "code_region_offset" {
            CodeRegionIntrinsicKind::Offset
        } else if name == "code_region_len" {
            CodeRegionIntrinsicKind::Len
        } else {
            return None;
        })
    }

    fn check_obligation_introduction(&mut self, expr: ExprId, callable: &Callable<'db>) -> bool {
        let CallableDef::Func(func) = callable.callable_def() else {
            return true;
        };
        if func.top_mod(self.db).ingot(self.db).kind(self.db) != IngotKind::Core
            || !crate::analysis::ty::corelib::lib_func_matches(
                self.db,
                func,
                "core::obligations::prove",
            )
        {
            return true;
        }
        let reason = match callable.generic_args() {
            [term] => {
                let term = self.normalize_ty(*term);
                let context = self.env.solver_context();
                crate::analysis::ty::trait_def::concrete_obligation_goal(self.db, term)
                    .and_then(|goal| match self.env.provision_for_goal(goal) {
                        Some(Provision::Named(selection)) => selection
                            .revalidate(self.db, context, goal)
                            .map(|_| ())
                            .map_err(|_| "selected implementation prerequisites were not proved"),
                        Some(Provision::EvidenceLocal(..) | Provision::EvidenceParameter(..)) => {
                            // The call carries this operand; CTFE validates its actual witness.
                            Ok(())
                        }
                        _ => crate::analysis::ty::trait_def::introduce_obligation(
                            self.db, context, term,
                        )
                        .map(|_| ()),
                    })
                    .err()
            }
            _ => Some("expected one concrete trait goal"),
        };
        if let Some(reason) = reason {
            self.push_diag(BodyDiag::ObligationIntroduction {
                primary: expr.span(self.body()).into(),
                reason,
            });
            return false;
        }
        true
    }

    fn const_intrinsic_kind(&self, callable_def: CallableDef<'db>) -> Option<ConstIntrinsicKind> {
        let CallableDef::Func(func) = callable_def else {
            return None;
        };
        (resolve_lib_func_path(self.db, self.env.scope(), "core::size_of") == Some(func))
            .then_some(ConstIntrinsicKind::SizeOf)
    }

    fn check_and_register_const_intrinsic(
        &mut self,
        expr: ExprId,
        callable: Callable<'db>,
        kind: ConstIntrinsicKind,
    ) -> bool {
        let ty = match kind {
            ConstIntrinsicKind::SizeOf => callable.generic_args().first().copied(),
        };
        if let Some(ty) = ty {
            let ty = self.normalize_ty(ty);
            if matches!(
                runtime_size_bytes(self.db, ty),
                Err(RuntimeSizeError::Overflow)
            ) {
                self.push_diag(BodyDiag::TypeSizeOverflow {
                    primary: expr.span(self.body()).into(),
                    ty,
                });
                return false;
            }
        }
        self.env.register_const_intrinsic(expr, callable, kind);
        true
    }

    pub(super) fn check_expr(&mut self, expr: ExprId, expected: TyId<'db>) -> ExprProp<'db> {
        self.check_expr_with_result_context(expr, expected, false)
    }

    pub(super) fn check_expr_unknown(&mut self, expr: ExprId) -> ExprProp<'db> {
        let t = self.fresh_ty();
        self.check_expr(expr, t)
    }

    pub(super) fn check_expr_with_discarded_result(
        &mut self,
        expr: ExprId,
        expected: TyId<'db>,
    ) -> ExprProp<'db> {
        let prop = self.check_expr_with_result_context(expr, expected, true);
        if !self.expr_propagates_discarded_result(expr) {
            self.check_unused_must_use(expr, prop.clone());
        }
        prop
    }

    fn check_expr_with_result_context(
        &mut self,
        expr: ExprId,
        expected: TyId<'db>,
        result_discarded: bool,
    ) -> ExprProp<'db> {
        let Partial::Present(expr_data) = self.env.expr_data(expr) else {
            let typed = ExprProp::invalid(self.db);
            self.env.type_expr(expr, typed.clone());
            return typed;
        };

        let expected = normalize_ty(
            self.db,
            expected,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                self.env.scope(),
                self.env.assumptions(),
            ),
        );

        self.env.enter_expr(expr);
        let mut actual = match expr_data {
            Expr::Lit(LitKind::String(string_id)) => {
                ExprProp::new(self.string_literal_ty(*string_id, expected), true)
            }
            Expr::Lit(LitKind::Int(integer)) if integer.data(self.db).bits() > 256 => {
                // No primitive integer type can represent more than one EVM word.
                // Reject before lowering can silently discard the high bits, even
                // when the literal's contextual type has not been inferred yet.
                self.push_diag(BodyDiag::IntLiteralOutOfRange {
                    primary: expr.span(self.body()).into(),
                    literal: integer.data(self.db).to_string(),
                    ty: TyId::u256(self.db),
                });
                ExprProp::invalid(self.db)
            }
            Expr::Lit(lit) => ExprProp::new(self.lit_ty_for_expected(lit, expected), true),
            Expr::Block(..) => self.check_block(expr, expr_data, expected, result_discarded),
            Expr::Un(..) => self.check_unary(expr, expr_data),
            Expr::Cast(inner, ty) => self.check_cast(expr, *inner, *ty),
            Expr::Bin(lhs, rhs, op) => self.check_binary(expr, *lhs, *rhs, *op),
            Expr::Call(..) => self.check_call(expr, expr_data),
            Expr::Assert(args) => self.check_assert(expr, args),
            Expr::UnsupportedMacroCall => {
                self.push_diag(BodyDiag::UnsupportedMacroCall(
                    expr.span(self.body()).into(),
                ));
                ExprProp::invalid(self.db)
            }
            Expr::Quote(quote) => self.check_quote(expr, quote),
            // Holes stand only in templates, which are not checked in place.
            Expr::Hole(_) => ExprProp::invalid(self.db),
            Expr::MethodCall(..) => self.check_method_call(expr, expr_data),
            Expr::Path(..) => self.check_path(expr, expr_data),
            Expr::RecordInit(..) => self.check_record_init(expr, expr_data, expected),
            Expr::Field(..) => self.check_field(expr, expr_data),
            Expr::Tuple(..) => self.check_tuple(expr, expr_data, expected),
            Expr::Array(..) => self.check_array(expr, expr_data, expected),
            Expr::ArrayRep(..) => self.check_array_rep(expr, expr_data, expected),
            Expr::If(..) => self.check_if(expr, expr_data, expected, result_discarded),
            Expr::Match(..) => self.check_match(expr, expr_data, expected, result_discarded),
            Expr::Assign(..) => self.check_assign(expr, expr_data),
            Expr::AugAssign(..) => self.check_aug_assign(expr, expr_data),
            Expr::With(bindings, body) => {
                self.check_with(expr, bindings, *body, expected, result_discarded)
            }
        };
        self.env.leave_expr();

        actual.ty = normalize_ty(
            self.db,
            actual.ty,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                self.env.scope(),
                self.env.assumptions(),
            ),
        );
        if let Some(coerced) =
            self.try_coerce_capability_for_expr_to_expected(expr, actual.ty, expected)
        {
            actual.ty = coerced;
        }
        let typeable = Typeable::Expr(expr, actual.clone());
        actual.ty = self.unify_ty(typeable, actual.ty, expected);
        if !actual.ty.has_invalid(self.db)
            && !self.in_const_context()
            && self.is_code_type(actual.ty)
        {
            self.push_diag(BodyDiag::CodeValueAtRuntime(expr.span(self.body()).into()));
        }
        actual
    }

    fn expr_propagates_discarded_result(&self, expr: ExprId) -> bool {
        matches!(
            self.env.expr_data(expr),
            Partial::Present(Expr::Block(..) | Expr::If(..) | Expr::Match(..) | Expr::With(..))
        )
    }

    fn lit_ty_for_expected(&mut self, lit: &LitKind<'db>, expected: TyId<'db>) -> TyId<'db> {
        match lit {
            LitKind::String(_) if expected.is_core_dyn_string(self.db) => expected,
            _ => self.lit_ty(lit),
        }
    }

    fn check_block(
        &mut self,
        expr: ExprId,
        expr_data: &Expr<'db>,
        expected: TyId<'db>,
        result_discarded: bool,
    ) -> ExprProp<'db> {
        let Expr::Block(stmts) = expr_data else {
            unreachable!()
        };

        if stmts.is_empty() {
            ExprProp::new(TyId::unit(self.db), true)
        } else {
            self.env.enter_scope(expr);
            for &stmt in stmts[..stmts.len() - 1].iter() {
                self.check_discarded_stmt(stmt);
            }

            let last_stmt = stmts[stmts.len() - 1];
            let res = if result_discarded {
                let expected = if expected == TyId::unit(self.db) {
                    self.fresh_ty()
                } else {
                    expected
                };
                ExprProp::new(
                    self.check_discarded_stmt_with_expected(last_stmt, expected),
                    true,
                )
            } else if expected == TyId::unit(self.db) {
                self.check_discarded_stmt(last_stmt);
                ExprProp::new(TyId::unit(self.db), true)
            } else {
                match self.env.stmt_data(last_stmt) {
                    Partial::Present(Stmt::Expr(expr)) => self.check_expr(*expr, expected),
                    Partial::Present(_) => {
                        ExprProp::new(self.check_stmt(last_stmt, expected), true)
                    }
                    Partial::Absent => ExprProp::invalid(self.db),
                }
            };
            self.env.leave_scope();
            res
        }
    }

    fn check_discarded_stmt(&mut self, stmt: StmtId) {
        let ty = self.fresh_ty();
        self.check_discarded_stmt_with_expected(stmt, ty);
    }

    fn check_discarded_stmt_with_expected(
        &mut self,
        stmt: StmtId,
        expected: TyId<'db>,
    ) -> TyId<'db> {
        match self.env.stmt_data(stmt) {
            Partial::Present(Stmt::Expr(expr)) => {
                self.check_expr_with_discarded_result(*expr, expected).ty
            }
            Partial::Present(_) => self.check_stmt(stmt, expected),
            Partial::Absent => TyId::invalid(self.db, InvalidCause::ParseError),
        }
    }

    fn check_unused_must_use(&mut self, expr: ExprId, prop: ExprProp<'db>) {
        if prop.ty.has_invalid(self.db) {
            return;
        }

        if let Some(adt_ref) = prop.ty.adt_ref(self.db)
            && adt_ref.is_must_use(self.db)
        {
            self.push_diag(BodyDiag::UnusedMustUse {
                primary: expr.span(self.body()).into(),
                subject: MustUseSubject::Type(prop.ty),
            });
            return;
        }

        let Partial::Present(expr_data) = self.env.expr_data(expr) else {
            return;
        };
        if matches!(expr_data, Expr::Call(..) | Expr::MethodCall(..))
            && let Some(callable) = self.env.callable_expr(expr)
            && callable.callable_def().is_must_use(self.db)
        {
            self.push_diag(BodyDiag::UnusedMustUse {
                primary: expr.span(self.body()).into(),
                subject: MustUseSubject::Function(callable.callable_def()),
            });
        }
    }

    fn check_unary(&mut self, expr: ExprId, expr_data: &Expr<'db>) -> ExprProp<'db> {
        let Expr::Un(lhs, op) = expr_data else {
            unreachable!()
        };
        let prop = self.check_expr_unknown(*lhs);
        if prop.ty.has_invalid(self.db) {
            return ExprProp::invalid(self.db);
        }

        if *op == UnOp::Deref {
            let ptr_ty = prop
                .ty
                .as_capability(self.db)
                .map_or(prop.ty, |(_, inner)| inner);
            if let Some(pointee) = ptr_ty.as_ptr(self.db) {
                return ExprProp::new(pointee, true);
            }

            let expected = TyId::ptr_to(self.db, self.fresh_ty());
            self.push_diag(BodyDiag::TypeMismatch {
                span: lhs.span(self.body()).into(),
                expected,
                given: prop.ty,
            });
            return ExprProp::invalid(self.db);
        }

        if prop.ty.is_integral_var(self.db) && matches!(op, UnOp::Plus | UnOp::Minus | UnOp::BitNot)
        {
            if matches!(op, UnOp::Minus) {
                self.env
                    .register_pending_primitive_op(PendingPrimitiveOp::Unary {
                        expr,
                        inner: *lhs,
                        op: *op,
                    });
            }
            return prop;
        }

        if matches!(op, UnOp::Mut | UnOp::Ref) {
            if !self.env.is_place_expr(*lhs) {
                self.push_diag(BodyDiag::BorrowFromNonPlace {
                    primary: expr.span(self.body()).into(),
                });
                return ExprProp::invalid(self.db);
            }

            let place_ty = prop
                .ty
                .as_capability(self.db)
                .map_or(prop.ty, |(_, inner)| inner);
            let borrow_provider = self
                .env
                .expr_place(*lhs)
                .and_then(|place| self.concrete_borrow_provider_for_place(&place));
            let borrow_provider =
                if borrow_provider.is_none() && self.env.is_pointer_place_expr(*lhs) {
                    Some(super::ProviderAddressSpace::Memory)
                } else {
                    borrow_provider
                };

            return match op {
                UnOp::Ref => ExprProp {
                    ty: TyId::borrow_ref_of(self.db, place_ty),
                    is_mut: false,
                    binding: None,
                    borrow_provider,
                    path_read_semantics: None,
                },
                UnOp::Mut => {
                    if !prop.is_mut {
                        self.report_cannot_borrow_mut(*lhs, expr.span(self.body()).into());
                        return ExprProp::invalid(self.db);
                    }
                    ExprProp {
                        ty: TyId::borrow_mut_of(self.db, place_ty),
                        is_mut: true,
                        binding: None,
                        borrow_provider,
                        path_read_semantics: None,
                    }
                }
                _ => unreachable!(),
            };
        }

        let base_ty = prop.ty.base_ty(self.db);
        if base_ty.is_ty_var(self.db) {
            let diag = BodyDiag::TypeMustBeKnown(lhs.span(self.body()).into());
            self.push_diag(diag);
            return ExprProp::invalid(self.db);
        }

        if *op == UnOp::Plus {
            if prop.ty.is_integral(self.db) {
                return prop;
            }
            let diag = BodyDiag::UnsupportedUnaryPlus(expr.span(self.body()).into());
            self.push_diag(diag);
            return ExprProp::invalid(self.db);
        }

        let lhs_ty = self.copy_inner_from_borrow(prop.ty).unwrap_or(prop.ty);
        if lhs_ty != prop.ty {
            self.unify_ty(Typeable::Expr(*lhs, prop.clone()), lhs_ty, lhs_ty);
        }

        self.check_ops_trait(expr, lhs_ty, op, None)
    }

    fn check_cast(
        &mut self,
        expr: ExprId,
        inner_expr: ExprId,
        target_ty: Partial<crate::hir_def::TypeId<'db>>,
    ) -> ExprProp<'db> {
        let inner_prop = self.check_expr_unknown(inner_expr);
        if inner_prop.ty.has_invalid(self.db) {
            return ExprProp::invalid(self.db);
        }

        let Some(hir_target_ty) = target_ty.to_opt() else {
            return ExprProp::invalid(self.db);
        };

        let span = expr.span(self.body()).into_cast_expr().ty();
        let target_ty = self.lower_ty(hir_target_ty, span, true);
        if target_ty.has_invalid(self.db) {
            return ExprProp::invalid(self.db);
        }

        let mut from = normalize_ty(
            self.db,
            inner_prop.ty,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                self.env.scope(),
                self.env.assumptions(),
            ),
        );
        let to = normalize_ty(
            self.db,
            target_ty,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                self.env.scope(),
                self.env.assumptions(),
            ),
        );

        // Casts operate on values, so for Copy capabilities treat the source as
        // the inner value type. This allows widening/narrowing checks such as
        // `(selector as u256)` when `selector` comes from a view parameter.
        if let Some((_, inner)) = from.as_capability(self.db)
            && self.ty_is_copy(inner)
        {
            from = inner;
        }

        if from == to {
            return ExprProp::new(to, true);
        }

        if let Partial::Present(Expr::Lit(LitKind::Int(int_id))) =
            inner_expr.data(self.db, self.body())
        {
            let value = int_id.data(self.db);
            let leaf = self.peel_transparent_newtypes(to);
            if to.is_string(self.db) && self.int_literal_fits_in_ty(value, TyId::u256(self.db)) {
                let _ = self.table.unify(from, TyId::u256(self.db));
                return ExprProp::new(to, true);
            }

            if self.int_literal_fits_in_ty(value, to) {
                // Unify the literal's type variable with the target leaf type
                // so it doesn't remain unresolved.
                let _ = self.table.unify(from, leaf);
                return ExprProp::new(to, true);
            }

            // Unify to prevent a spurious "type annotation needed" error.
            let _ = self.table.unify(from, leaf);
            let diag = BodyDiag::InvalidCast {
                primary: expr.span(self.body()).into(),
                from,
                to,
                hint: Some(format!(
                    "integer literal `{}` is not representable in `{}`",
                    value,
                    leaf.pretty_print(self.db),
                )),
            };
            self.push_diag(diag);
            return ExprProp::invalid(self.db);
        }

        // Fail if the source type is unknown.
        if from.base_ty(self.db).is_ty_var(self.db) {
            let diag = BodyDiag::TypeMustBeKnown(inner_expr.span(self.body()).into());
            self.push_diag(diag);
            return ExprProp::invalid(self.db);
        }

        if self.is_lossless_cast(from, to)
            || self.is_provably_lossless_cast_expr(inner_expr, from, to)
        {
            return ExprProp::new(to, true);
        }

        // Check if the cast failed due to invisible struct fields.
        let hint = if self.is_single_field_struct_with_invisible_field(from)
            || self.is_single_field_struct_with_invisible_field(to)
        {
            Some("cast is not allowed because the struct field is not `pub`".to_string())
        } else {
            None
        };

        let diag = BodyDiag::InvalidCast {
            primary: expr.span(self.body()).into(),
            from,
            to,
            hint,
        };
        self.push_diag(diag);
        ExprProp::invalid(self.db)
    }

    // Allow eg `(some_u256 >> 224) as u32`
    fn is_provably_lossless_cast_expr(&self, expr: ExprId, from: TyId<'db>, to: TyId<'db>) -> bool {
        let body = self.body();
        if let Some((false, from_bits)) =
            self.prim_int_signed_bits(self.peel_transparent_newtypes(from))
            && let Some((false, to_bits)) =
                self.prim_int_signed_bits(self.peel_transparent_newtypes(to))
            && let Partial::Present(Expr::Bin(_, rhs, BinOp::Arith(ArithBinOp::RShift))) =
                expr.data(self.db, body)
            && let Partial::Present(Expr::Lit(LitKind::Int(shift_int))) = rhs.data(self.db, body)
            && let Some(shift) = shift_int.data(self.db).to_usize()
        {
            to_bits >= from_bits.saturating_sub(shift)
        } else {
            false
        }
    }

    fn is_lossless_cast(&self, from: TyId<'db>, to: TyId<'db>) -> bool {
        if from == to {
            return true;
        }

        let from_leaf = self.peel_transparent_newtypes(from);
        let to_leaf = self.peel_transparent_newtypes(to);

        if from_leaf == to_leaf {
            return true;
        }

        if from_leaf.is_bool(self.db) {
            return self.prim_int_signed_bits(to_leaf).is_some();
        }

        if to_leaf.is_bool(self.db) {
            return false;
        }

        if self.is_string_word_cast(from_leaf, to_leaf) {
            return true;
        }

        if self.is_pointer_cast(from_leaf, to_leaf) {
            return true;
        }

        self.is_lossless_int_cast(from_leaf, to_leaf)
    }

    fn is_string_word_cast(&self, from: TyId<'db>, to: TyId<'db>) -> bool {
        (from.is_string(self.db) && self.is_plain_u256(to))
            || (self.is_plain_u256(from) && to.is_string(self.db))
    }

    fn is_pointer_cast(&self, from: TyId<'db>, to: TyId<'db>) -> bool {
        from.as_ptr(self.db).is_some() && to.as_ptr(self.db).is_some()
    }

    fn is_plain_u256(&self, ty: TyId<'db>) -> bool {
        matches!(
            ty.base_ty(self.db).data(self.db),
            TyData::TyBase(TyBase::Prim(PrimTy::U256))
        )
    }

    fn transparent_newtype_field_ty(&self, ty: TyId<'db>) -> Option<TyId<'db>> {
        if ty.is_tuple(self.db) {
            let field_tys = ty.field_types(self.db);
            return (field_tys.len() == 1).then(|| field_tys[0]);
        }

        if ty.is_struct(self.db) {
            let field_tys = ty.field_types(self.db);
            if field_tys.len() != 1 {
                return None;
            }

            // Reject cast if the struct field is not visible from the current scope.
            if self.is_single_field_struct_with_invisible_field(ty) {
                return None;
            }

            return Some(field_tys[0]);
        }

        None
    }

    /// Returns `true` if `ty` is a single-field struct whose field is not visible
    /// from the current scope.
    fn is_single_field_struct_with_invisible_field(&self, ty: TyId<'db>) -> bool {
        if !ty.is_struct(self.db) {
            return false;
        }
        let field_tys = ty.field_types(self.db);
        if field_tys.len() != 1 {
            return false;
        }
        let Some(adt_def) = ty.adt_def(self.db) else {
            return false;
        };
        let AdtRef::Struct(s) = adt_def.adt_ref(self.db) else {
            return false;
        };
        let field_scope = ScopeId::Field(FieldParent::Struct(s), 0);
        !is_scope_visible_from(self.db, field_scope, self.env.scope())
    }

    fn peel_transparent_newtypes(&self, mut ty: TyId<'db>) -> TyId<'db> {
        while let Some(inner) = self.transparent_newtype_field_ty(ty) {
            ty = inner;
        }
        ty
    }

    fn prim_int_signed_bits(&self, ty: TyId<'db>) -> Option<(bool, usize)> {
        let base = ty.base_ty(self.db);
        let TyData::TyBase(TyBase::Prim(prim)) = base.data(self.db) else {
            return None;
        };
        let bits = prim_int_bits(*prim)?;
        let signed = matches!(
            prim,
            PrimTy::I8
                | PrimTy::I16
                | PrimTy::I32
                | PrimTy::I64
                | PrimTy::I128
                | PrimTy::I256
                | PrimTy::Isize
        );
        Some((signed, bits))
    }

    fn is_lossless_int_cast(&self, from: TyId<'db>, to: TyId<'db>) -> bool {
        let Some((from_signed, from_bits)) = self.prim_int_signed_bits(from) else {
            return false;
        };
        let Some((to_signed, to_bits)) = self.prim_int_signed_bits(to) else {
            return false;
        };

        match (from_signed, to_signed) {
            (false, false) => to_bits >= from_bits,
            (true, true) => to_bits >= from_bits,
            (false, true) => to_bits > from_bits,
            (true, false) => false,
        }
    }

    fn int_literal_fits_in_ty(&self, value: &BigUint, target_ty: TyId<'db>) -> bool {
        let leaf = self.peel_transparent_newtypes(target_ty);
        let Some((signed, bits)) = self.prim_int_signed_bits(leaf) else {
            return false;
        };

        if signed {
            let max = (BigUint::from(1u8) << (bits - 1)) - BigUint::from(1u8);
            value <= &max
        } else {
            let max = (BigUint::from(1u8) << bits) - BigUint::from(1u8);
            value <= &max
        }
    }

    fn negated_int_literal_fits_in_ty(&self, value: &BigUint, target_ty: TyId<'db>) -> bool {
        let leaf = self.peel_transparent_newtypes(target_ty);
        let Some((signed, bits)) = self.prim_int_signed_bits(leaf) else {
            return false;
        };
        if !signed {
            return false;
        }

        let max = BigUint::from(1u8) << (bits - 1);
        value <= &max
    }

    fn check_binary(
        &mut self,
        expr: ExprId,
        lhs_expr: ExprId,
        rhs_expr: ExprId,
        op: BinOp,
    ) -> ExprProp<'db> {
        // Logical operands must be bools
        if let BinOp::Logical(_) = op {
            let bool = TyId::bool(self.db);
            let lhs = self.check_expr(lhs_expr, bool);
            let rhs = self.check_expr(rhs_expr, bool);
            return if lhs.ty.is_bool(self.db) && rhs.ty.is_bool(self.db) {
                ExprProp::new(bool, true)
            } else {
                ExprProp::invalid(self.db)
            };
        }

        // Range expressions construct Range types directly
        if matches!(op, BinOp::Arith(ArithBinOp::Range)) {
            return self.check_range_expr(expr, lhs_expr, rhs_expr);
        }

        let lhs = self.check_expr_unknown(lhs_expr);
        if lhs.ty.has_invalid(self.db) {
            return ExprProp::invalid(self.db);
        }

        let lhs_place_ty = lhs
            .ty
            .as_capability(self.db)
            .map_or(lhs.ty, |(_, inner)| inner);
        if matches!(op, BinOp::Index) && lhs_place_ty.is_array(self.db) {
            // Built-in array indexing (TODO: move to trait impl)
            let args = lhs_place_ty.generic_args(self.db);
            let elem_ty = args[0];
            let index_ty = args[1].const_ty_ty(self.db).unwrap();
            self.check_expr(rhs_expr, index_ty);
            // Check the index for static array
            if let Some(index) = self.try_eval_static_int(rhs_expr, index_ty)
                && let Some(len) = lhs_place_ty.array_len(self.db)
                && index.data(self.db) >= &BigUint::from(len)
            {
                self.push_diag(BodyDiag::ArrayIndexOutOfBounds {
                    primary: rhs_expr.span(self.body()).into(),
                    index,
                    len,
                })
            }
            if let Some(projected) = self.contract_field_projected_index_ty(lhs_expr) {
                return ExprProp::new(self.table.fold_ty(self.db, projected), lhs.is_mut);
            }
            return ExprProp::new(elem_ty, lhs.is_mut);
        }

        if lhs.ty.is_integral_var(self.db) {
            // Avoid 'type must be known' diagnostics when lhs is an unknown integer ty.
            // For unknown integer types, the result type depends on the operator:
            // - arithmetic: same integer type
            // - comparison: bool
            self.check_expr(rhs_expr, lhs.ty);
            if matches!(
                op,
                BinOp::Arith(
                    ArithBinOp::Add
                        | ArithBinOp::Sub
                        | ArithBinOp::Mul
                        | ArithBinOp::Div
                        | ArithBinOp::Rem
                        | ArithBinOp::Pow
                ) | BinOp::Comp(..)
            ) {
                self.env
                    .register_pending_primitive_op(PendingPrimitiveOp::Binary {
                        expr,
                        lhs: lhs_expr,
                        rhs: rhs_expr,
                        op,
                    });
            }

            if matches!(op, BinOp::Comp(_)) {
                return ExprProp::new(TyId::bool(self.db), true);
            }

            return lhs;
        }

        // Fail if lhs ty is unknown
        if lhs.ty.base_ty(self.db).is_ty_var(self.db) {
            self.check_expr_unknown(rhs_expr);
            let diag = BodyDiag::TypeMustBeKnown(lhs_expr.span(self.body()).into());
            self.push_diag(diag);
            return ExprProp::invalid(self.db);
        }

        let lhs_ty = self.copy_inner_from_borrow(lhs.ty).unwrap_or(lhs.ty);
        if lhs_ty != lhs.ty {
            self.unify_ty(Typeable::Expr(lhs_expr, lhs.clone()), lhs_ty, lhs_ty);
        }

        self.check_ops_trait(expr, lhs_ty, &op, Some(rhs_expr))
    }

    pub(super) fn resolve_pending_primitive_op(
        &mut self,
        pending: &PendingPrimitiveOp,
    ) -> PendingPrimitiveOpResolution {
        if self.env.callable_expr(pending.expr()).is_some() {
            return PendingPrimitiveOpResolution::Done;
        }

        let Some(expr_prop) = self.env.typed_expr(pending.expr()) else {
            return PendingPrimitiveOpResolution::Done;
        };
        let expr_ty = {
            let mut prober = super::env::Prober::new(&mut self.table, self.env.scope());
            expr_prop.ty.fold_with(self.db, &mut prober)
        };
        if expr_ty.has_invalid(self.db) {
            return PendingPrimitiveOpResolution::Done;
        }

        let resolved = match pending {
            PendingPrimitiveOp::Unary { expr, inner, op } => {
                let Some(inner_prop) = self.env.typed_expr(*inner) else {
                    return PendingPrimitiveOpResolution::Done;
                };
                let operand_ty = {
                    let mut prober = super::env::Prober::new(&mut self.table, self.env.scope());
                    inner_prop.ty.fold_with(self.db, &mut prober)
                };
                let operand_ty = operand_ty
                    .as_capability(self.db)
                    .map_or(operand_ty, |(_, inner)| inner);
                let operand_ty = self.normalize_ty(operand_ty);
                if operand_ty.has_invalid(self.db) {
                    return PendingPrimitiveOpResolution::Done;
                }
                if operand_ty.is_integral_var(self.db)
                    || operand_ty.base_ty(self.db).is_ty_var(self.db)
                {
                    return PendingPrimitiveOpResolution::Pending;
                }
                if matches!(op, UnOp::Minus)
                    && let Some(int_id) = self.try_get_literal_int(*inner)
                {
                    let literal = int_id.data(self.db);
                    if self.negated_int_literal_fits_in_ty(literal, operand_ty) {
                        return PendingPrimitiveOpResolution::Done;
                    }
                    if self
                        .peel_transparent_newtypes(operand_ty)
                        .base_ty(self.db)
                        .is_prim(self.db)
                    {
                        self.push_diag(BodyDiag::IntLiteralOutOfRange {
                            primary: expr.span(self.body()).into(),
                            literal: format!("-{literal}"),
                            ty: operand_ty,
                        });
                        return PendingPrimitiveOpResolution::Done;
                    }
                }
                self.check_ops_trait(*expr, operand_ty, op, None)
            }
            PendingPrimitiveOp::Binary { expr, lhs, rhs, op } => {
                let Some(lhs_prop) = self.env.typed_expr(*lhs) else {
                    return PendingPrimitiveOpResolution::Done;
                };
                let Some(rhs_prop) = self.env.typed_expr(*rhs) else {
                    return PendingPrimitiveOpResolution::Done;
                };
                let lhs_ty = {
                    let mut prober = super::env::Prober::new(&mut self.table, self.env.scope());
                    lhs_prop.ty.fold_with(self.db, &mut prober)
                };
                let rhs_ty = {
                    let mut prober = super::env::Prober::new(&mut self.table, self.env.scope());
                    rhs_prop.ty.fold_with(self.db, &mut prober)
                };
                let lhs_ty = lhs_ty
                    .as_capability(self.db)
                    .map_or(lhs_ty, |(_, inner)| inner);
                let lhs_ty = self.normalize_ty(lhs_ty);
                let rhs_ty = rhs_ty
                    .as_capability(self.db)
                    .map_or(rhs_ty, |(_, inner)| inner);
                let rhs_ty = self.normalize_ty(rhs_ty);
                if lhs_ty.has_invalid(self.db) || rhs_ty.has_invalid(self.db) {
                    return PendingPrimitiveOpResolution::Done;
                }
                if lhs_ty.is_integral_var(self.db)
                    || lhs_ty.base_ty(self.db).is_ty_var(self.db)
                    || rhs_ty.is_integral_var(self.db)
                    || rhs_ty.base_ty(self.db).is_ty_var(self.db)
                {
                    return PendingPrimitiveOpResolution::Pending;
                }
                self.check_ops_trait(*expr, lhs_ty, op, Some(*rhs))
            }
        };

        if resolved.ty.has_invalid(self.db) {
            return PendingPrimitiveOpResolution::Done;
        }
        self.table.unify(expr_ty, resolved.ty).ok();
        PendingPrimitiveOpResolution::Resolved
    }

    fn check_let_condition(&mut self, pat: PatId, scrutinee: ExprId) -> ExprProp<'db> {
        let scrutinee_ty = self.fresh_ty();
        let scrutinee_prop = self.check_expr(scrutinee, scrutinee_ty);
        let (pat_expected, mode) = self.destructure_source_mode(scrutinee_prop.ty);
        let layout = self.pattern_layout_context(scrutinee);
        self.check_pat_with_layout(pat, pat_expected, layout.as_ref());
        if let super::PatternDestructureMode::Borrow(kind) = mode {
            self.retype_pattern_bindings_for_borrow(pat, kind);
        }

        ExprProp::new(TyId::bool(self.db), true)
    }

    pub(super) fn check_cond(&mut self, cond: CondId) -> ExprProp<'db> {
        let Partial::Present(cond_data) = cond.data(self.db, self.body()) else {
            return ExprProp::invalid(self.db);
        };

        match cond_data {
            Cond::Expr(expr) => self.check_expr(*expr, TyId::bool(self.db)),
            Cond::Let(pat, scrutinee) => self.check_let_condition(*pat, *scrutinee),
            Cond::Bin(lhs, rhs, op) => {
                let lhs = self.check_cond(*lhs);
                match op {
                    LogicalBinOp::And => self.env.flush_pending_bindings(),
                    LogicalBinOp::Or => self.env.clear_pending_bindings(),
                }
                let rhs = self.check_cond(*rhs);
                if lhs.ty.is_bool(self.db) && rhs.ty.is_bool(self.db) {
                    ExprProp::new(TyId::bool(self.db), true)
                } else {
                    ExprProp::invalid(self.db)
                }
            }
        }
    }

    /// Check a range expression `start..end` and return the Range type.
    ///
    /// Both operands must be `usize`. The result type depends on whether bounds
    /// are compile-time constants:
    /// - `Range<Known<S>, Known<E>>` when both are literals (0 words)
    /// - `Range<Known<S>, Unknown>` when only start is literal (1 word)
    /// - `Range<Unknown, Known<E>>` when only end is literal (1 word)
    /// - `Range<Unknown, Unknown>` when neither is literal (2 words)
    fn check_range_expr(
        &mut self,
        _expr: ExprId,
        start_expr: ExprId,
        end_expr: ExprId,
    ) -> ExprProp<'db> {
        let usize_ty = TyId::new(self.db, TyData::TyBase(TyBase::Prim(PrimTy::Usize)));

        // Check that both operands are usize
        self.check_expr(start_expr, usize_ty);
        self.check_expr(end_expr, usize_ty);

        // Try to detect if bounds are literal integers
        let start_lit = self.try_get_literal_int(start_expr);
        let end_lit = self.try_get_literal_int(end_expr);

        // Resolve Range types from core library
        match resolve_core_range_types(self.db, self.env.scope()) {
            Some(types) => {
                // Construct appropriate bound types based on constness
                let start_bound =
                    self.make_range_bound(start_lit, types.known, types.unknown, usize_ty);
                let end_bound =
                    self.make_range_bound(end_lit, types.known, types.unknown, usize_ty);

                // Construct Range<StartBound, EndBound>
                let range_s = TyId::app(self.db, types.range, start_bound);
                let range_full = TyId::app(self.db, range_s, end_bound);
                ExprProp::new(range_full, true)
            }
            _ => {
                // Fallback: if Range/Known/Unknown isn't found, return invalid
                // This shouldn't happen in normal usage
                ExprProp::invalid(self.db)
            }
        }
    }

    /// Try to extract a literal integer value from an expression.
    /// Returns `Some(IntegerId)` if the expression is a literal integer, `None` otherwise.
    fn try_get_literal_int(&self, expr: ExprId) -> Option<IntegerId<'db>> {
        let Partial::Present(expr_data) = self.env.expr_data(expr) else {
            return None;
        };

        match expr_data {
            Expr::Lit(LitKind::Int(int_id)) => Some(*int_id),
            _ => None,
        }
    }

    fn try_eval_static_int(&self, expr: ExprId, expected: TyId<'db>) -> Option<IntegerId<'db>> {
        if let Some(value) = self.try_get_literal_int(expr) {
            return Some(value);
        }
        if let Some(const_ref) = self.env.expr_const_ref(expr)
            && let Some(const_ref) =
                resolve_semantic_const_ref(self.db, const_ref, expected, SemOrigin::Expr(expr))
            && let EvalOutcome::Ready(value) = eval_const_ref(self.db, const_ref)
            && let SemConstValue::Scalar {
                value: SemConstScalar::Int { value },
                ..
            } = value.value(self.db)
        {
            let (sign, bytes) = value.to_bytes_be();
            if sign != Sign::Minus {
                return Some(IntegerId::new(self.db, BigUint::from_bytes_be(&bytes)));
            }
        }
        let value = try_eval_const_int_expr(self.db, self.body(), expr, expected)?;
        let (sign, bytes) = value.to_bytes_be();
        (sign != Sign::Minus).then(|| IntegerId::new(self.db, BigUint::from_bytes_be(&bytes)))
    }

    /// Create a range bound type: either `Known<N>` for a literal or `Unknown`.
    fn make_range_bound(
        &self,
        lit: Option<IntegerId<'db>>,
        known_base: TyId<'db>,
        unknown_ty: TyId<'db>,
        usize_ty: TyId<'db>,
    ) -> TyId<'db> {
        match lit {
            Some(int_id) => {
                // Create Known<N> where N is the literal value
                let const_ty = const_ty_from_sem_const(
                    self.db,
                    int_const(
                        self.db,
                        usize_ty,
                        BigInt::from(int_id.data(self.db).clone()),
                    ),
                );
                let const_ty_id = TyId::const_ty(self.db, const_ty);
                TyId::app(self.db, known_base, const_ty_id)
            }
            None => unknown_ty,
        }
    }

    fn static_selection(
        &mut self,
        expr: ExprId,
    ) -> Option<Result<crate::analysis::semantic::SelectedApplication<'db>, BodyDiag<'db>>> {
        use crate::analysis::ty::trait_def::{
            named_impl_aliases, named_impl_candidates, unnamed_impl_candidates_for_goal,
        };
        let Partial::Present(Expr::Path(Partial::Present(path))) = expr.data(self.db, self.body())
        else {
            return None;
        };
        let minter = LoweringContext::new(HoleAnchor::BodySyntax {
            body: self.body(),
            site: BodyHoleSite::Expr(expr),
        });
        let primary: DynLazySpan<'db> = expr.span(self.body()).into();
        if matches!(path.kind(self.db), PathKind::QualifiedType { .. }) {
            let path_span = expr.span(self.body()).into_path_expr().path();
            let PathRes::Ty(ty) = self
                .resolve_path(*path, false, path_span, primary.clone(), &minter)
                .ok()?
            else {
                return None;
            };
            let TyData::QualifiedTy(goal) = ty.data(self.db) else {
                return None;
            };
            let goal = *goal;
            let subject = goal.self_ty(self.db);
            if subject.has_var(self.db) || subject.has_invalid(self.db) {
                return Some(Err(BodyDiag::GoalSelectionFailed {
                    primary,
                    goal,
                    candidates: Vec::new(),
                    reason: "goal selection requires a known subject type",
                    unproved: None,
                }));
            }
            let candidates = unnamed_impl_candidates_for_goal(self.db, self.env.scope(), goal);
            let [candidate] = candidates.as_slice() else {
                let reason = if candidates.is_empty() {
                    "no unnamed implementation matches this goal"
                } else {
                    "more than one unnamed implementation matches this goal"
                };
                return Some(Err(BodyDiag::GoalSelectionFailed {
                    primary,
                    goal,
                    candidates,
                    reason,
                    unproved: None,
                }));
            };
            return Some(self.prove_static_selection(*candidate, goal).map_err(
                |(reason, unproved)| BodyDiag::GoalSelectionFailed {
                    primary,
                    goal,
                    candidates,
                    reason,
                    unproved,
                },
            ));
        }
        if !path.is_bare_ident(self.db) {
            return None;
        }
        let resolved = resolve_ident_expr(
            self.db,
            &self.env,
            *path,
            expr.span(self.body()).into(),
            &minter,
        );
        let name = path.ident(self.db).to_opt()?;
        let candidates = named_impl_candidates(self.db, self.env.scope(), name);
        let value_span = match &resolved {
            ResolvedPathInBody::Binding(binding) => Some(binding.def_span(&self.env)),
            ResolvedPathInBody::Reso(value) => {
                let is_value = match value {
                    PathRes::Ty(ty) | PathRes::TyAlias(_, ty, _) => {
                        ty.const_ty_ty(self.db).is_some()
                    }
                    PathRes::EnumVariant(variant) => {
                        matches!(
                            variant.kind(self.db),
                            VariantKind::Unit | VariantKind::Tuple(_)
                        )
                    }
                    PathRes::Func(_)
                    | PathRes::TraitMethod(..)
                    | PathRes::Const(..)
                    | PathRes::Method(..)
                    | PathRes::TraitConst(..)
                    | PathRes::InherentConst(..) => true,
                    PathRes::Mod(_) | PathRes::Trait(_) | PathRes::FuncParam(..) => false,
                };
                is_value.then(|| value.name_span(self.db)).flatten()
            }
            _ => None,
        };
        if let Some(value_span) = value_span
            && !candidates.is_empty()
        {
            return Some(Err(BodyDiag::ValueAndNamedSelection {
                primary,
                name,
                value_span,
                candidates,
            }));
        }
        if !matches!(resolved, ResolvedPathInBody::NewBinding(_)) {
            return None;
        }
        if candidates.is_empty() {
            let available = named_impl_aliases(self.db, self.env.scope());
            return (!available.is_empty()).then_some(Err(BodyDiag::UnknownNamedSelection {
                primary,
                name,
                available,
            }));
        }
        let [candidate] = candidates.as_slice() else {
            return Some(Err(BodyDiag::AmbiguousNamedSelection {
                primary,
                name,
                candidates,
            }));
        };
        let failure = |reason, unproved| BodyDiag::NamedSelectionFailed {
            primary: primary.clone(),
            name,
            candidate: *candidate,
            reason,
            unproved,
        };
        let goal = match candidate.trait_inst(self.db) {
            Some(goal) => goal,
            None => return Some(Err(failure("invalid implementation application", None))),
        };
        if goal
            .args(self.db)
            .iter()
            .any(|ty| ty.has_param(self.db) || ty.has_var(self.db))
        {
            return Some(Err(failure(
                "named selection currently requires a concrete implementation application",
                None,
            )));
        }
        Some(
            self.prove_static_selection(*candidate, goal)
                .map_err(|(reason, unproved)| failure(reason, unproved)),
        )
    }

    pub(super) fn prove_static_selection(
        &self,
        candidate: crate::hir_def::ImplTrait<'db>,
        goal: TraitInstId<'db>,
    ) -> Result<
        crate::analysis::semantic::SelectedApplication<'db>,
        (&'static str, Option<TraitInstId<'db>>),
    > {
        use crate::analysis::{
            semantic::SelectedApplication,
            ty::trait_def::{prove_selected_hir_impl_evidence, unproved_named_impl_requirement},
        };
        let context = self.env.solver_context();
        let proof =
            prove_selected_hir_impl_evidence(self.db, context, candidate, goal).map_err(|_| {
                (
                    "selected implementation prerequisites could not be proved",
                    unproved_named_impl_requirement(self.db, context, candidate, goal),
                )
            })?;
        let goal = proof
            .goal_with_associated_bindings()
            .map_err(|_| ("selected associated outputs must be concrete", None))?;
        let proof =
            prove_selected_hir_impl_evidence(self.db, context, candidate, goal).map_err(|_| {
                (
                    "selected associated outputs could not be proved",
                    unproved_named_impl_requirement(self.db, context, candidate, goal),
                )
            })?;
        Ok(SelectedApplication::from_evidence(&proof))
    }

    fn insert_with_provision(&mut self, value: ExprId, provision: Provision<'db>) -> bool {
        if self.env.insert_provision(provision).is_ok() {
            return true;
        }
        let diagnostic = BodyDiag::ObligationOpening {
            primary: value.span(self.body()).into(),
            reason: "one frame cannot provide the same trait goal twice",
        };
        self.env.record_with_choice(value, Err(diagnostic.clone()));
        self.push_diag(diagnostic);
        false
    }

    fn check_with(
        &mut self,
        site: ExprId,
        bindings: &[WithBinding<'db>],
        body_expr: ExprId,
        expected: TyId<'db>,
        result_discarded: bool,
    ) -> ExprProp<'db> {
        self.env.enter_context_frame(site);

        for binding in bindings {
            if binding.key_path.is_none()
                && let Some(selection) = self.static_selection(binding.value)
            {
                match selection {
                    Ok(selection) => {
                        if !self.insert_with_provision(binding.value, Provision::named(selection)) {
                            continue;
                        }
                        self.env
                            .type_expr(binding.value, ExprProp::new(TyId::unit(self.db), false));
                        self.env.static_selection_exprs.insert(binding.value);
                        self.env
                            .record_with_choice(binding.value, Ok(Provision::named(selection)));
                    }
                    Err(diag) => {
                        self.env
                            .record_with_choice(binding.value, Err(diag.clone()));
                        self.push_diag(diag);
                    }
                }
                continue;
            }
            let value_prop = self.check_expr_unknown(binding.value);

            if binding.key_path.is_none() {
                let ty = self.normalize_ty(value_prop.ty);
                let (base, args) = ty.decompose_ty_app(self.db);
                let core_record = base.adt_def(self.db).is_some_and(|adt| {
                    matches!(adt.adt_ref(self.db), AdtRef::Struct(record)
                        if record.top_mod(self.db).ingot(self.db).kind(self.db) == IngotKind::Core)
                });
                if core_record && is_core_evidence_ty(self.db, self.env.scope(), ty) {
                    if let [term] = args
                        && let TyData::ConstraintTerm(goal) = term.data(self.db)
                    {
                        // Reopening one immutable evidence binding retains its operand
                        // identity; a second read is not a different witness.
                        let provision = self
                            .env
                            .provision_for_goal(*goal)
                            .filter(|previous| {
                                let Some((operand, _)) = previous.evidence_operand() else {
                                    return false;
                                };
                                value_prop.binding.is_some()
                                    && self.env.typed_expr(operand).and_then(|prop| prop.binding)
                                        == value_prop.binding
                            })
                            .unwrap_or_else(|| {
                                Provision::evidence(binding.value, *goal, value_prop.binding)
                            });
                        if !self.insert_with_provision(binding.value, provision) {
                            continue;
                        }
                        self.env.record_with_choice(binding.value, Ok(provision));
                        continue;
                    }
                    self.push_diag(BodyDiag::ObligationOpening {
                        primary: binding.value.span(self.body()).into(),
                        reason: "opening requires evidence with an explicit trait goal",
                    });
                    continue;
                }
            }

            let is_mut = value_prop.binding.map_or(value_prop.is_mut, |b| b.is_mut());

            let provided = ProvidedEffect {
                origin: EffectOrigin::With {
                    value_expr: binding.value,
                },
                ty: self.table.fold_ty(self.db, value_prop.ty),
                is_mut,
                binding: value_prop.binding,
            };

            match binding.key_path {
                Some(key_path) => {
                    if let Some(key_path) = key_path.to_opt() {
                        let folded_provider_ty = self.table.fold_ty(self.db, provided.ty);
                        match self.validate_keyed_with(
                            key_path,
                            ProvidedEffect {
                                ty: folded_provider_ty,
                                ..provided
                            },
                            binding.value.span(self.body()).into(),
                        ) {
                            Ok((witness, commit)) => {
                                let committed = self.apply_effect_commit_plan(commit);
                                debug_assert!(
                                    committed,
                                    "validated keyed `with` binding commit failed"
                                );
                                if !committed {
                                    let barrier = EffectBarrier {
                                        pattern: build_barrier_pattern_for_with_key(self, key_path)
                                            .expect("validated keyed binding should have a barrier pattern"),
                                        reason: self.barrier_reason_for_pattern(
                                            key_path,
                                            witness.key.clone(),
                                            binding.value.span(self.body()).into(),
                                        ),
                                    };
                                    self.insert_effect_barrier(barrier);
                                    continue;
                                }
                                self.env.effect_env_mut().insert_witness(self.db, witness);
                            }
                            Err(barrier) => {
                                self.insert_effect_barrier(*barrier);
                            }
                        }
                    }
                }
                None => {
                    self.env.effect_env_mut().insert_unkeyed(provided);
                }
            }
        }

        let result = if result_discarded {
            self.check_expr_with_discarded_result(body_expr, expected)
        } else {
            self.check_expr(body_expr, expected)
        };
        let boundary = self
            .env
            .selection_boundaries(None)
            .into_iter()
            .find(|boundary| boundary.site == site);
        let escaped =
            boundary.is_some_and(|boundary| self.reject_selection_type_escape(result.ty, boundary));
        self.env.leave_context_frame();
        if escaped {
            ExprProp::invalid(self.db)
        } else {
            result
        }
    }

    pub(super) fn reject_selection_type_escape(
        &mut self,
        ty: TyId<'db>,
        boundary: super::effect_env::SelectionBoundary<'db>,
    ) -> bool {
        let inside = self.env.selection_context();
        let outside_chain = self.env.recorded_position_chain(boundary.site);
        let differs = self.selection_type_layout_differs(ty, |tc, source| {
            let inner = inside.contextualize_const_types_with(tc.db, source, |goal| {
                tc.env.provision_for_goal(goal)
            });
            let inner = tc.ground_selection_type(inner, inside);
            let outer = boundary
                .outside
                .contextualize_const_types_with(tc.db, source, |goal| {
                    outside_chain.lookup(tc.db, goal)
                });
            let outer = tc.ground_selection_type(outer, boundary.outside);
            (inner, outer)
        });
        if differs {
            self.env.mark_selection_boundary_reported(boundary.site);
            self.push_diag(BodyDiag::SelectedConstantTypeEscape(
                boundary.site.span(self.body()).into(),
            ));
        }
        differs
    }

    pub(super) fn reject_call_selection_type_escape(
        &mut self,
        callable: &Callable<'db>,
        expr: ExprId,
        ty: TyId<'db>,
        span: DynLazySpan<'db>,
    ) -> bool {
        if self.env.selection_boundaries(None).is_empty() {
            return false;
        }
        let mut origin = expr;
        let mut seen = FxHashSet::default();
        while seen.insert(origin) {
            match origin.data(self.db, self.body()) {
                Partial::Present(Expr::Un(value, _) | Expr::Field(value, _)) => origin = *value,
                Partial::Present(Expr::Path(_)) => {
                    let Some(binding) = self.env.typed_expr(origin).and_then(|prop| prop.binding)
                    else {
                        break;
                    };
                    if matches!(binding, LocalBinding::Param { .. }) {
                        // A parameter arrives in the body's root context, before
                        // any lexical selection at this argument expression.
                        origin = self.body().expr(self.db);
                        break;
                    }
                    let Some(source) = self.env.binding_source(binding) else {
                        break;
                    };
                    origin = source.init_expr;
                }
                _ => break,
            }
        }
        let chain = self.env.recorded_position_chain(origin);
        let inside = chain.context(self.db, self.env.scope());
        let differs = self.selection_type_layout_differs(ty, |tc, source| {
            let inner = inside
                .contextualize_const_types_with(tc.db, source, |goal| chain.lookup(tc.db, goal));
            let inner = tc.ground_selection_type(inner, inside);
            let current = tc.env.selection_context();
            let outer = tc.normalize_callable_type(callable, source);
            let outer = tc.ground_selection_type(outer, current);
            // Nominal parameter equality does not compare selected field layouts.
            // Compare the value's origin with the callee in either direction.
            (inner, outer)
        });
        if differs {
            self.push_diag(BodyDiag::SelectedConstantArgumentLayoutMismatch(span));
        }
        differs
    }

    fn ground_selection_type(
        &self,
        ty: TyId<'db>,
        context: crate::analysis::semantic::ImplEnv<'db>,
    ) -> TyId<'db> {
        use crate::analysis::ty::const_ty::{ConstCanonEnv, concretize_const_ty_if_ground};
        struct Ground<'db>(crate::analysis::semantic::ImplEnv<'db>);
        impl<'db> TyFolder<'db> for Ground<'db> {
            fn fold_ty(
                &mut self,
                db: &'db dyn crate::analysis::HirAnalysisDb,
                ty: TyId<'db>,
            ) -> TyId<'db> {
                let ty = ty.super_fold_with(db, self);
                if let TyData::ConstTy(value) = ty.data(db) {
                    let env = ConstCanonEnv::new(
                        self.0.normalization_scope(db),
                        self.0.assumptions(db),
                        None,
                    );
                    if let Some(value) = concretize_const_ty_if_ground(db, *value, env) {
                        return TyId::const_ty(db, value);
                    }
                }
                ty
            }
        }
        ty.fold_with(self.db, &mut Ground(context))
    }

    fn selection_type_layout_differs(
        &mut self,
        ty: TyId<'db>,
        mut contextualize: impl FnMut(&mut Self, TyId<'db>) -> (TyId<'db>, TyId<'db>),
    ) -> bool {
        let mut pending = vec![ty.fold_with(self.db, &mut self.table)];
        let mut visited = FxHashSet::default();
        while let Some(source) = pending.pop() {
            if !visited.insert(source) || source.has_invalid(self.db) {
                continue;
            }
            let (inner, outer) = contextualize(self, source);
            if inner != outer && !inner.has_invalid(self.db) && !outer.has_invalid(self.db) {
                return true;
            }
            pending.extend(source.generic_args(self.db).iter().copied());
            if let Some(adt) = source.adt_def(self.db) {
                for (variant, fields) in adt.fields(self.db).iter().enumerate() {
                    for field in 0..fields.num_types() {
                        pending.push(instantiate_adt_field_source_for_concrete_demand(
                            self.db,
                            adt,
                            variant,
                            field,
                            source.generic_args(self.db),
                        ));
                    }
                }
            }
        }
        false
    }

    /// A callee's signature with its generic arguments filled in can contain a
    /// const that faults only now (`S<{N - 5}>` at `N = 2`). Report the fault
    /// at the call; the invalid type would otherwise match any type.
    fn report_instantiated_const_fault(
        &mut self,
        callable: &Callable<'db>,
        ty: TyId<'db>,
        span: DynLazySpan<'db>,
    ) {
        use crate::analysis::ty::ty_error::{
            diag_from_invalid_cause, first_invalid_ty_cause, is_const_fault,
        };
        let Some(cause) = first_invalid_ty_cause(self.db, ty) else {
            return;
        };
        if is_const_fault(&cause)
            && let Some(diag) = diag_from_invalid_cause(span.clone(), &cause)
        {
            self.push_diag(diag);
            return;
        }
        // The fault can lose its cause on the way (a computed type's subject,
        // `Tree<{N - 5}>`). It still came from these generic arguments if the
        // declared return type and the arguments are valid.
        let declared = callable
            .callable_def()
            .ret_ty(self.db)
            .instantiate_identity();
        if matches!(cause, InvalidCause::Other)
            && !declared.has_invalid(self.db)
            && !callable
                .generic_args()
                .iter()
                .any(|arg| arg.has_invalid(self.db))
        {
            self.push_diag(BodyDiag::ReturnTypeNotComputable(span));
        }
    }

    pub(super) fn normalize_callable_type(
        &mut self,
        callable: &Callable<'db>,
        ty: TyId<'db>,
    ) -> TyId<'db> {
        let requirements = callable
            .selection_requirements(self.db)
            .fold_with(self.db, &mut self.table);
        let provisions = requirements
            .list(self.db)
            .iter()
            .filter_map(|&goal| self.env.provision_for_goal(goal))
            .collect::<Vec<_>>();
        let assumptions = crate::semantic::lookup_assumptions(
            self.db,
            self.env.assumptions(),
            provisions
                .iter()
                .filter_map(|p| p.evidence_operand().map(|(_, goal)| goal)),
            provisions.iter().filter_map(|p| match p {
                crate::semantic::Provision::Named(selection) => Some(*selection),
                _ => None,
            }),
        );
        let context = self.env.selection_context();
        let ty = ty.fold_with(self.db, &mut self.table);
        // A callee receives the provisions required by its signature, rather than
        // every choice visible at the call site.
        let ty = context.contextualize_const_types_with(self.db, ty, |goal| {
            crate::semantic::lookup_trait_goal(
                self.db,
                goal,
                provisions.iter().copied(),
                |provision| provision.goal(),
            )
        });
        normalize_ty(
            self.db,
            ty,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                self.env.scope(),
                assumptions,
            ),
        )
    }

    fn normalize_callable_return(&mut self, callable: &Callable<'db>) -> TyId<'db> {
        self.normalize_callable_type(callable, callable.ret_ty(self.db))
    }

    fn check_call(&mut self, expr: ExprId, expr_data: &Expr<'db>) -> ExprProp<'db> {
        let Expr::Call(callee, args) = expr_data else {
            unreachable!()
        };
        let callee_prop = self.check_expr_unknown(*callee);
        if callee_prop.ty.has_invalid(self.db) {
            return ExprProp::invalid(self.db);
        }

        let mut callable = if matches!(
            callee.data(self.db, self.body()),
            Partial::Present(Expr::Path(..))
        ) && let Some(existing) = self.env.callable_expr(*callee)
        {
            existing.clone()
        } else {
            match Callable::new(
                self.db,
                callee_prop.ty,
                callee.span(self.body()).into(),
                None,
            ) {
                Ok(callable) => callable,
                Err(diag) => {
                    self.push_diag(diag);
                    return ExprProp::invalid(self.db);
                }
            }
        };

        let call_span = expr.span(self.body()).into_call_expr();

        if let Some(kind) = self.code_region_intrinsic_kind(callable.callable_def())
            && args.len() == 1
            && let Some(result) = self.check_code_region_intrinsic(
                expr,
                &mut callable,
                args,
                kind,
                None,
                Some(*callee),
            )
        {
            return result;
        }

        callable.check_args(self, args, call_span.clone().args(), None, false);
        self.specialize_callable_layout_args(&mut callable, None, args);

        self.check_callable_effects(expr, &mut callable);

        callable.process_constraints(self, expr, call_span.callee().into());

        let normalized_ret_ty = self.normalize_callable_return(&callable);
        self.check_evidence_call_source(expr, &callable, normalized_ret_ty, args, None);

        self.report_instantiated_const_fault(
            &callable,
            normalized_ret_ty,
            expr.span(self.body()).into(),
        );
        if !self.in_const_context() && self.callable_contains_quote(callable.callable_def()) {
            self.push_diag(BodyDiag::QuoteCallAtRuntime(expr.span(self.body()).into()));
            return ExprProp::invalid(self.db);
        }
        if !self.check_obligation_introduction(expr, &callable) {
            return ExprProp::invalid(self.db);
        }
        if let Some(kind) = self.const_intrinsic_kind(callable.callable_def()) {
            if !self.check_and_register_const_intrinsic(expr, callable, kind) {
                return ExprProp::invalid(self.db);
            }
        } else {
            self.env.register_semantic_call(expr, callable);
        }
        ExprProp::new(normalized_ret_ty, true)
    }

    fn check_assert(&mut self, expr: ExprId, args: &[HirCallArg<'db>]) -> ExprProp<'db> {
        if !(1..=2).contains(&args.len()) {
            for arg in args {
                self.check_expr_unknown(arg.expr);
            }
            self.push_diag(BodyDiag::AssertArgNumMismatch {
                primary: expr.span(self.body()).into_macro_call_expr().args().into(),
                given: args.len(),
            });
            return ExprProp::invalid(self.db);
        }

        self.check_expr(args[0].expr, TyId::bool(self.db));
        if let Some(message_arg) = args.get(1) {
            let message = message_arg.expr;
            let Partial::Present(Expr::Lit(LitKind::String(string_id))) =
                message.data(self.db, self.body())
            else {
                self.check_expr_unknown(message);
                self.push_diag(BodyDiag::AssertMessageMustBeStringLiteral {
                    primary: message.span(self.body()).into(),
                });
                return ExprProp::invalid(self.db);
            };

            let expected = self.string_literal_byte_array_ty(string_id.len_bytes(self.db));
            self.check_expr(message, expected);
        }
        ExprProp::new(TyId::unit(self.db), true)
    }

    fn check_code_region_intrinsic(
        &mut self,
        expr: ExprId,
        callable: &mut Callable<'db>,
        args: &[crate::hir_def::CallArg<'db>],
        kind: CodeRegionIntrinsicKind,
        receiver: Option<(ExprId, ExprProp<'db>)>,
        direct_callee: Option<ExprId>,
    ) -> Option<ExprProp<'db>> {
        let arg_expr = args[0].expr;
        callable.check_args(
            self,
            args,
            expr.span(self.body()).into_call_expr().args(),
            receiver,
            false,
        );
        let arg_ty = self.env.typed_expr(arg_expr).map_or_else(
            || TyId::invalid(self.db, InvalidCause::Other),
            |prop| self.normalize_ty(prop.ty),
        );
        if !ty_may_be_code_region_token(self.db, arg_ty) {
            return None;
        }
        if let Some(callee) = direct_callee {
            self.env
                .type_expr(callee, ExprProp::new(callable.ty(self.db), true));
        }
        self.env
            .register_code_region_intrinsic(expr, callable.clone(), arg_expr, kind);
        Some(ExprProp::new(TyId::u256(self.db), true))
    }

    pub(super) fn check_callable_effects(&mut self, expr: ExprId, callable: &mut Callable<'db>) {
        let body = self.body();
        let call_span: DynLazySpan<'db> = expr.span(body).into();
        let args = self.resolve_callable_effects(call_span.clone(), callable);
        for arg in args {
            self.env.push_call_effect_arg(expr, arg);
        }
    }

    pub(super) fn resolve_callable_effects(
        &mut self,
        call_span: DynLazySpan<'db>,
        callable: &mut Callable<'db>,
    ) -> Vec<super::ResolvedEffectArg<'db>> {
        let CallableDef::Func(func) = callable.callable_def else {
            return Vec::new();
        };

        if func.is_method(self.db)
            && let Some(receiver) = callable.arg_ty(self.db, 0)
            && let Some((CapabilityKind::Mut, inner)) =
                self.table.fold_ty(self.db, receiver).as_capability(self.db)
            && self.is_storage_effect_ty(inner)
            && self.storage_write_provider().is_none()
            && !self.storage_write_authorized(func, None, 0, call_span.clone())
        {
            self.push_diag(BodyDiag::StorageWriteCapabilityRequired { primary: call_span });
            return Vec::new();
        }

        if !func.has_effects(self.db) {
            return Vec::new();
        }

        let mut resolved_args: Vec<super::ResolvedEffectArg<'db>> = Vec::new();
        let mut specialized_providers: FxHashMap<u32, EffectProviderSpecialization<'db>> =
            FxHashMap::default();

        let body = self.body();
        let callee_provider_arg_idx_by_effect =
            place_effect_provider_param_index_map(self.db, func);
        let callee_effect_env =
            crate::core::semantic::EffectEnvView::new(EffectParamSite::Func(func));

        let provided_span = |provided: ProvidedEffect<'db>| match provided.origin {
            EffectOrigin::With { value_expr } => Some(value_expr.span(body).into()),
            EffectOrigin::Param { .. } | EffectOrigin::RootProvider { .. } => None,
        };
        let reqs = effect_requirement_decls_for_callable(self.db, callable.callable_def);
        for (param_idx, req) in reqs.iter().enumerate() {
            let key_ty = req.key_ty;
            let Some(query) = build_effect_query_for_call(self, callable, req) else {
                continue;
            };
            let provider_arg_idx_for_param = callee_provider_arg_idx_by_effect
                .get(req.binding_idx as usize)
                .copied()
                .flatten();

            match self.resolve_effect_query(func, req.name, query.clone(), call_span.clone()) {
                EffectResolution::Chosen(evidence) => {
                    let (provider, arg_style, layout_view, key_kind, instantiated_key_ty) =
                        match *evidence {
                            EffectEvidence::Keyed {
                                provider,
                                key_kind,
                                target_ty,
                                commit,
                                arg_style,
                                layout_view,
                            } => {
                                let committed = self.apply_effect_commit_plan(commit);
                                debug_assert!(
                                    committed,
                                    "chosen keyed effect evidence commit failed"
                                );
                                if !committed {
                                    let diag = BodyDiag::MissingEffect {
                                        primary: call_span.clone(),
                                        func,
                                        key: key_ty,
                                    };
                                    self.push_diag(diag);
                                    continue;
                                }
                                (provider, arg_style, layout_view, key_kind, target_ty)
                            }
                            EffectEvidence::UnkeyedType {
                                provider,
                                commit,
                                arg_style,
                                layout_view,
                            } => {
                                let target_ty = match commit.key_match.clone() {
                                    Some(KeyMatchCommit::QueryToType { actual, .. }) => {
                                        Some(self.table.fold_ty(self.db, actual))
                                    }
                                    _ => match req.key.clone() {
                                        EffectRequirementKey::Type(_) => self
                                            .query_type_key(&query.key)
                                            .map(|ty| self.table.fold_ty(self.db, ty)),
                                        EffectRequirementKey::Trait(_) => None,
                                    },
                                };
                                let committed = self.apply_effect_commit_plan(commit);
                                debug_assert!(
                                    committed,
                                    "chosen unkeyed type effect evidence commit failed"
                                );
                                if !committed {
                                    let diag = BodyDiag::MissingEffect {
                                        primary: call_span.clone(),
                                        func,
                                        key: key_ty,
                                    };
                                    self.push_diag(diag);
                                    continue;
                                }
                                (
                                    provider,
                                    arg_style,
                                    layout_view,
                                    EffectKeyKind::Type,
                                    target_ty,
                                )
                            }
                            EffectEvidence::UnkeyedTrait {
                                provider,
                                commit,
                                arg_style,
                                layout_view,
                            } => {
                                let committed = self.apply_effect_commit_plan(commit);
                                debug_assert!(
                                    committed,
                                    "chosen unkeyed trait effect evidence commit failed"
                                );
                                if !committed {
                                    let diag = BodyDiag::MissingEffect {
                                        primary: call_span.clone(),
                                        func,
                                        key: key_ty,
                                    };
                                    self.push_diag(diag);
                                    continue;
                                }
                                (provider, arg_style, layout_view, EffectKeyKind::Trait, None)
                            }
                        };

                    let (arg, pass_mode) =
                        self.effect_arg_for_provider(provider, arg_style, req.required_mut);
                    let provider_target_ty = self.provider_target_ty_for_effect_arg(
                        provider,
                        instantiated_key_ty,
                        req.required_mut,
                    );
                    if req.required_mut && matches!(pass_mode, super::EffectPassMode::Unknown) {
                        let diag = BodyDiag::EffectMutabilityMismatch {
                            primary: call_span.clone(),
                            func,
                            key: key_ty,
                            provided_span: provided_span(provider),
                        };
                        self.push_diag(diag);
                        continue;
                    }
                    if req.required_mut
                        && (matches!(
                            self.concrete_borrow_provider_for_effect_handle_ty(provider.ty),
                            Some(
                                super::ProviderAddressSpace::Storage
                                    | super::ProviderAddressSpace::Transient
                            )
                        ) || self.is_storage_effect_ty(provider.ty)
                            || provider_target_ty.is_some_and(|ty| self.is_storage_effect_ty(ty)))
                        && !self.storage_write_authorized(
                            func,
                            req.name,
                            req.binding_idx,
                            call_span.clone(),
                        )
                    {
                        self.push_diag(BodyDiag::StorageWriteCapabilityRequired {
                            primary: call_span.clone(),
                        });
                        continue;
                    }
                    if !self.effect_arg_is_valid(&arg, pass_mode) {
                        let diag = BodyDiag::MissingEffect {
                            primary: call_span.clone(),
                            func,
                            key: key_ty,
                        };
                        self.push_diag(diag);
                        continue;
                    }
                    if let Some(provider_arg_idx) = provider_arg_idx_for_param
                        && let Some(provider_var) =
                            callable.generic_args().get(provider_arg_idx).copied()
                        && let Some(given) = self.inferred_provider_ty_for_effect_arg(
                            provider,
                            &arg,
                            pass_mode,
                            provider_target_ty,
                        )
                    {
                        let existing_provider = self.table.fold_ty(self.db, provider_var);
                        let snapshot = self.table.snapshot();
                        if self.table.unify(provider_var, given).is_err() {
                            self.table.rollback_to(snapshot);
                            self.push_diag(BodyDiag::EffectProviderMismatch {
                                primary: call_span.clone(),
                                func,
                                key: key_ty,
                                expected: existing_provider,
                                given,
                                provided_span: provided_span(provider),
                            });
                        }
                    }
                    if let Some(target_ty) = instantiated_key_ty {
                        self.instantiate_callable_effect_layout_args(
                            callable,
                            func,
                            req.binding_idx as usize,
                            target_ty,
                        );
                    }
                    self.specialize_callable_effect_layout_projections(
                        callable,
                        func,
                        req.binding_idx as usize,
                        provider,
                        &arg,
                    );
                    let provider_space = self
                        .effect_arg_provider_space(&arg, pass_mode)
                        .or_else(|| self.concrete_borrow_provider_for_effect_handle_ty(provider.ty))
                        .or_else(|| {
                            matches!(pass_mode, super::EffectPassMode::ByTempPlace)
                                .then_some(super::ProviderAddressSpace::Memory)
                        });
                    if matches!(
                        pass_mode,
                        super::EffectPassMode::ByPlace | super::EffectPassMode::ByTempPlace
                    ) && provider_space.is_none()
                        && !self.effect_arg_provider_space_can_remain_unresolved(&arg)
                    {
                        panic!(
                            "effect arg provider space must be explicit for {pass_mode:?} at {key_ty:?}"
                        );
                    }
                    if let Some(resolved_binding) =
                        callee_effect_env.resolved_binding(self.db, req.binding_idx as usize)
                    {
                        let provider_idx = resolved_binding.provider.provider_idx;
                        let specialization = self.specialize_effect_provider_binding(
                            resolved_binding.provider,
                            provider,
                            &arg,
                            pass_mode,
                            provider_target_ty,
                            provider_space,
                        );
                        if let Some(previous) =
                            specialized_providers.insert(provider_idx, specialization.clone())
                        {
                            assert_eq!(
                                previous, specialization,
                                "conflicting call-site provider specialization for function effect provider slot {provider_idx} at {key_ty:?}",
                            );
                        }
                    }
                    resolved_args.push(super::ResolvedEffectArg {
                        param_idx,
                        binding_idx: req.binding_idx,
                        key: key_ty,
                        arg,
                        with_source: match provider.origin {
                            EffectOrigin::With { value_expr } => Some(value_expr),
                            EffectOrigin::Param { .. } | EffectOrigin::RootProvider { .. } => None,
                        },
                        pass_mode,
                        layout_view,
                        required_mut: req.required_mut,
                        key_kind,
                        instantiated_key_ty,
                        provider_target_ty,
                        provider: provider_space,
                    });
                }
                EffectResolution::BlockedByBarrier => {}
                EffectResolution::Missing => {
                    self.push_diag(BodyDiag::MissingEffect {
                        primary: call_span.clone(),
                        func,
                        key: key_ty,
                    });
                }
                EffectResolution::Ambiguous => {
                    self.push_diag(BodyDiag::AmbiguousEffect {
                        primary: call_span.clone(),
                        func,
                        key: key_ty,
                    });
                }
            }
        }
        let mut providers = specialized_providers.into_values().collect::<Vec<_>>();
        providers.sort_by_key(|provider| provider.provider.provider_idx);
        *callable.effect_providers_mut() = providers;

        resolved_args
    }

    fn instantiate_callable_effect_layout_args(
        &mut self,
        callable: &mut Callable<'db>,
        callee: crate::hir_def::Func<'db>,
        effect_idx: usize,
        actual_key_ty: TyId<'db>,
    ) {
        instantiate_callable_effect_layout_args(
            self.db,
            callee,
            effect_idx,
            self.table.fold_ty(self.db, actual_key_ty),
            callable.generic_args_mut(),
        );
    }

    fn resolve_effect_query(
        &mut self,
        func: crate::hir_def::Func<'db>,
        required_name: Option<IdentId<'db>>,
        query: EffectQuery<'db>,
        call_span: DynLazySpan<'db>,
    ) -> EffectResolution<'db> {
        let mut viable: SmallVec<[EffectEvidence<'db>; 2]> = SmallVec::new();
        let effect_env = self.env.effect_env().clone();
        for frame in effect_env.lookup_effect_frames(&query, self) {
            match frame {
                FrameLookupResult::KeyedMatched {
                    entries,
                    blocked_by_barrier,
                    barrier_reason,
                } => {
                    for matched in entries.iter().cloned() {
                        if let Some(evidence) =
                            self.evaluate_keyed_entry(query.required_mut, matched)
                        {
                            viable.push(evidence);
                        }
                    }
                    if viable.is_empty() {
                        return if blocked_by_barrier {
                            let _ = barrier_reason;
                            EffectResolution::BlockedByBarrier
                        } else {
                            EffectResolution::Missing
                        };
                    }
                    return self.choose_effect_evidence(required_name, viable);
                }
                FrameLookupResult::KeyedFamily { entries, providers } => {
                    let mut family_viable = SmallVec::new();
                    for entry in entries.iter().cloned() {
                        let Some(matched) = self.match_family_keyed_entry(&query.key, entry) else {
                            continue;
                        };
                        if let Some(evidence) =
                            self.evaluate_keyed_entry(query.required_mut, matched)
                        {
                            family_viable.push(evidence);
                        }
                    }
                    for provider in providers.iter().copied() {
                        let evidence = match query.key.clone() {
                            EffectPatternKey::Type(type_query) => self
                                .evaluate_unkeyed_type_provider(
                                    type_query,
                                    provider,
                                    query.required_mut,
                                ),
                            EffectPatternKey::Trait(trait_query) => {
                                self.evaluate_unkeyed_trait_provider(trait_query, provider)
                            }
                        };
                        if let Some(evidence) = evidence {
                            family_viable.push(evidence);
                        }
                    }
                    if !family_viable.is_empty() {
                        return self.choose_effect_evidence(required_name, family_viable);
                    }
                }
                FrameLookupResult::Unkeyed { providers } => {
                    for provider in providers {
                        let evidence = match query.key.clone() {
                            EffectPatternKey::Type(type_query) => self
                                .evaluate_unkeyed_type_provider(
                                    type_query,
                                    provider,
                                    query.required_mut,
                                ),
                            EffectPatternKey::Trait(trait_query) => {
                                self.evaluate_unkeyed_trait_provider(trait_query, provider)
                            }
                        };
                        if let Some(evidence) = evidence {
                            viable.push(evidence);
                        }
                    }
                    if !viable.is_empty() {
                        return self.choose_effect_evidence(required_name, viable);
                    }
                }
            }
        }
        if let EffectPatternKey::Trait(key) = &query.key
            && query.required_mut
            && resolve_lib_trait_path(self.db, self.env.scope(), "std::evm::effects::RawStorage")
                == Some(key.def)
            && let Some(provider) = self.storage_write_provider()
            && let Some(evidence) = self.evaluate_unkeyed_trait_provider(key.clone(), provider)
        {
            return EffectResolution::Chosen(Box::new(evidence));
        }
        let _ = (func, call_span);
        EffectResolution::Missing
    }

    fn storage_write_authorized(
        &mut self,
        func: crate::hir_def::Func<'db>,
        required_name: Option<IdentId<'db>>,
        binding_idx: u32,
        span: DynLazySpan<'db>,
    ) -> bool {
        let Some(raw) =
            resolve_lib_trait_path(self.db, self.env.scope(), "std::evm::effects::RawStorage")
        else {
            return false;
        };
        let query = EffectQuery {
            binding_idx,
            required_mut: true,
            mode: EffectQueryMode::Precise,
            key: EffectPatternKey::Trait(TraitPatternKey {
                def: raw,
                args_no_self: SmallVec::new(),
                assoc_bindings: SmallVec::new(),
                family: EffectFamily::Trait(raw),
                slots: PatternSlots::empty(),
            }),
        };
        let EffectResolution::Chosen(evidence) =
            self.resolve_effect_query(func, required_name, query, span)
        else {
            return false;
        };
        if !self.provider_supports_mut(evidence_provider(&evidence)) {
            return false;
        }
        let commit = match *evidence {
            EffectEvidence::Keyed { commit, .. }
            | EffectEvidence::UnkeyedType { commit, .. }
            | EffectEvidence::UnkeyedTrait { commit, .. } => commit,
        };
        self.apply_effect_commit_plan(commit)
    }

    fn is_storage_effect_ty(&mut self, ty: TyId<'db>) -> bool {
        // Incomplete classification requires authority; it never grants authority.
        self.storage_effect_affinity(ty).unwrap_or(true)
    }

    fn storage_effect_affinity(&mut self, ty: TyId<'db>) -> Option<bool> {
        let Some(storage_effect) = resolve_lib_trait_path(
            self.db,
            self.env.scope(),
            "std::evm::effects::StorageEffect",
        ) else {
            // Core-only ingots cannot contain standard-library storage descriptors.
            return Some(false);
        };
        let mut pending = vec![ty];
        let mut visited = FxHashSet::default();
        while let Some(ty) = pending.pop() {
            let ty = self.table.fold_ty(self.db, ty);
            if !visited.insert(ty) {
                continue;
            }
            if visited.len() > 256 {
                return None;
            }
            if let Some((kind, inner)) = ty.as_capability(self.db) {
                if kind == CapabilityKind::Mut {
                    pending.push(inner);
                }
                continue;
            }
            let goal = TraitInstId::new(self.db, storage_effect, vec![ty], IndexMap::new());
            if matches!(
                self.trait_effect_goal_satisfiability(goal),
                GoalSatisfiability::Satisfied(_)
            ) {
                return Some(true);
            }
            pending.extend(ty.field_types(self.db));
            if let Some(adt) = ty.adt_def(self.db)
                && matches!(adt.adt_ref(self.db), AdtRef::Enum(_))
            {
                for (variant, fields) in adt.fields(self.db).iter().enumerate() {
                    for field in 0..fields.num_types() {
                        pending.push(instantiate_adt_field_shape(
                            self.db,
                            adt,
                            variant,
                            field,
                            ty.generic_args(self.db),
                        ));
                    }
                }
            }
            if ty.is_array(self.db) {
                pending.extend(ty.generic_args(self.db).first().copied());
            }
        }
        Some(false)
    }

    /// Authority comes from mutable effects or a mutable storage receiver whose
    /// caller was checked. A mutable local alone cannot grant authority.
    fn storage_write_provider(&mut self) -> Option<ProvidedEffect<'db>> {
        let site = crate::analysis::semantic::effect_param_site(self.env.owner())?;
        let view = crate::core::semantic::EffectEnvView::new(site);
        let raw =
            resolve_lib_trait_path(self.db, self.env.scope(), "std::evm::effects::RawStorage")?;
        let mut storage_authority = matches!(self.env.owner(), BodyOwner::ContractInit { .. });
        if let BodyOwner::Func(func) = self.env.owner()
            && func.is_method(self.db)
            && let Some(receiver) = func.arg_tys(self.db).first()
            && let Some((CapabilityKind::Mut, inner)) =
                receiver.instantiate_identity().as_capability(self.db)
        {
            storage_authority |= self.storage_effect_affinity(inner) == Some(true);
        }
        for requirement in view.requirements(self.db).into_iter().filter(|r| r.is_mut) {
            let idx = requirement.binding_idx as usize;
            let Some(binding) = view.resolved_binding(self.db, idx) else {
                continue;
            };
            let Some(ty) = view.visible_effect_binding_ty(self.db, idx) else {
                continue;
            };
            let raw_goal = TraitInstId::new(self.db, raw, vec![ty], IndexMap::new());
            if matches!(
                self.trait_effect_goal_satisfiability(raw_goal),
                GoalSatisfiability::Satisfied(_)
            ) {
                return Some(ProvidedEffect {
                    origin: EffectOrigin::Param {
                        site,
                        index: idx,
                        name: Some(requirement.binding_name),
                    },
                    ty,
                    is_mut: true,
                    binding: Some(LocalBinding::effect_param(&binding)),
                });
            }
            storage_authority |= matches!(
                binding.provider.semantics.address_space,
                Some(super::ProviderAddressSpace::Storage | super::ProviderAddressSpace::Transient)
            ) || self.storage_effect_affinity(ty) == Some(true);
        }
        if !storage_authority {
            return None;
        }
        let provider = view.providers(self.db).into_iter().find(|provider| {
            matches!(
                provider.source,
                crate::core::semantic::ProviderSource::RootProvider { .. }
            )
        })?;
        Some(ProvidedEffect {
            origin: EffectOrigin::RootProvider {
                site,
                provider_idx: provider.provider_idx,
            },
            ty: provider.provider_ty,
            is_mut: true,
            binding: None,
        })
    }

    fn match_family_keyed_entry(
        &mut self,
        query: &EffectPatternKey<'db>,
        entry: FamilyKeyedEntry<'db>,
    ) -> Option<MatchedKeyedEntry<'db>> {
        match entry {
            FamilyKeyedEntry::Witness(witness) => {
                let key_commit = query_matches_witness(self, query, &witness.key)?;
                Some(MatchedKeyedEntry::Witness(MatchedWitness {
                    witness,
                    key_commit,
                }))
            }
            FamilyKeyedEntry::Forwarder(forwarder) => {
                let key_commit = query_matches_forwarder(self, query, &forwarder.key)?;
                Some(MatchedKeyedEntry::Forwarder(MatchedForwarder {
                    forwarder,
                    key_commit,
                }))
            }
        }
    }

    fn choose_effect_evidence(
        &self,
        required_name: Option<IdentId<'db>>,
        viable: SmallVec<[EffectEvidence<'db>; 2]>,
    ) -> EffectResolution<'db> {
        if let [only] = viable.as_slice() {
            EffectResolution::Chosen(Box::new(only.clone()))
        } else {
            let Some(required_name) = required_name else {
                return EffectResolution::Ambiguous;
            };
            let mut name_matches = viable.into_iter().filter(|evidence| {
                let provider = evidence_provider(evidence);
                match (provider.origin, provider.binding) {
                    (
                        EffectOrigin::Param {
                            name: Some(name), ..
                        },
                        _,
                    ) => name == required_name,
                    (EffectOrigin::With { .. }, Some(binding)) => {
                        binding.binding_name(&self.env) == required_name
                    }
                    _ => false,
                }
            });
            if let Some(best) = name_matches.next()
                && name_matches.next().is_none()
            {
                EffectResolution::Chosen(Box::new(best))
            } else {
                EffectResolution::Ambiguous
            }
        }
    }

    fn evaluate_keyed_entry(
        &mut self,
        required_mut: bool,
        matched: MatchedKeyedEntry<'db>,
    ) -> Option<EffectEvidence<'db>> {
        let (provider, transport, key_kind, target_ty, key_commit) = match matched {
            MatchedKeyedEntry::Witness(matched) => {
                let (key_kind, target_ty) = match matched.witness.key {
                    StoredEffectKey::Type(stored) => (EffectKeyKind::Type, Some(stored.carrier)),
                    StoredEffectKey::Trait(_) => (EffectKeyKind::Trait, None),
                };
                (
                    matched.witness.provider,
                    matched.witness.transport,
                    key_kind,
                    target_ty,
                    matched.key_commit,
                )
            }
            MatchedKeyedEntry::Forwarder(matched) => {
                let (key_kind, target_ty) = match matched.forwarder.key {
                    ForwardedEffectKey::Type(forwarded) => {
                        (EffectKeyKind::Type, Some(forwarded.carrier))
                    }
                    ForwardedEffectKey::Trait(_) => (EffectKeyKind::Trait, None),
                };
                (
                    matched.forwarder.provider,
                    matched.forwarder.transport,
                    key_kind,
                    target_ty,
                    matched.key_commit,
                )
            }
        };
        let arg_style = match (transport, target_ty) {
            (WitnessTransport::Direct, Some(target_ty)) => self
                .direct_arg_style_for_provider(provider, target_ty, required_mut)
                .unwrap_or(EffectArgStyle::Value),
            _ => EffectArgStyle::Value,
        };
        let layout_view =
            if key_kind == EffectKeyKind::Type && transport == WitnessTransport::ByValue {
                EffectArgLayoutView::ProviderTarget
            } else {
                EffectArgLayoutView::Direct
            };
        Some(EffectEvidence::Keyed {
            provider,
            key_kind,
            target_ty,
            commit: EffectCommitPlan {
                key_match: Some(key_commit),
                trait_solutions: SmallVec::new(),
                provider_resolution: None,
                extra_unifications: SmallVec::new(),
            },
            arg_style,
            layout_view,
        })
    }

    fn evaluate_unkeyed_type_provider(
        &mut self,
        query: TypePatternKey<'db>,
        provider: ProvidedEffect<'db>,
        required_mut: bool,
    ) -> Option<EffectEvidence<'db>> {
        let direct_style =
            self.direct_arg_style_for_provider(provider, query.carrier, required_mut);
        if let Some(arg_style) = direct_style {
            let snapshot = self.snapshot_state();
            let ok = apply_key_match_commit(
                self,
                KeyMatchCommit::QueryToType {
                    query: query.clone(),
                    actual: provider
                        .ty
                        .as_capability(self.db)
                        .map_or(provider.ty, |(_, inner)| inner),
                },
            );
            self.rollback_state(snapshot);
            if ok {
                return Some(EffectEvidence::UnkeyedType {
                    provider,
                    commit: EffectCommitPlan {
                        key_match: Some(KeyMatchCommit::QueryToType {
                            query: query.clone(),
                            actual: provider
                                .ty
                                .as_capability(self.db)
                                .map_or(provider.ty, |(_, inner)| inner),
                        }),
                        trait_solutions: SmallVec::new(),
                        provider_resolution: None,
                        extra_unifications: SmallVec::new(),
                    },
                    arg_style,
                    layout_view: EffectArgLayoutView::Direct,
                });
            }
        }

        let resolution = self.effect_provider_target_resolution(provider.ty, required_mut)?;
        let snapshot = self.snapshot_state();
        let ok = apply_key_match_commit(
            self,
            KeyMatchCommit::QueryToType {
                query: query.clone(),
                actual: resolution.target_ty,
            },
        );
        self.rollback_state(snapshot);
        ok.then_some(EffectEvidence::UnkeyedType {
            provider,
            commit: EffectCommitPlan {
                key_match: Some(KeyMatchCommit::QueryToType {
                    query,
                    actual: resolution.target_ty,
                }),
                trait_solutions: SmallVec::new(),
                provider_resolution: Some(resolution),
                extra_unifications: SmallVec::new(),
            },
            arg_style: EffectArgStyle::Value,
            layout_view: EffectArgLayoutView::ProviderTarget,
        })
    }

    fn evaluate_unkeyed_trait_provider(
        &mut self,
        query: TraitPatternKey<'db>,
        provider: ProvidedEffect<'db>,
    ) -> Option<EffectEvidence<'db>> {
        let provider_ty = self.table.fold_ty(self.db, provider.ty);
        if provider_ty.has_var(self.db) {
            return None;
        }
        let instantiated = instantiate_trait_pattern_in(self.db, &mut self.table, query.clone());
        let args = std::iter::once(provider_ty)
            .chain(instantiated.args(self.db).iter().skip(1).copied())
            .collect::<Vec<_>>();
        let trait_goal = TraitInstId::new(
            self.db,
            instantiated.def(self.db),
            args,
            instantiated.assoc_type_bindings(self.db).clone(),
        );
        let GoalSatisfiability::Satisfied(solution) =
            self.trait_effect_goal_satisfiability(trait_goal)
        else {
            return None;
        };
        Some(EffectEvidence::UnkeyedTrait {
            provider,
            commit: EffectCommitPlan {
                key_match: None,
                trait_solutions: SmallVec::from_iter([(trait_goal, solution)]),
                provider_resolution: None,
                extra_unifications: SmallVec::new(),
            },
            arg_style: EffectArgStyle::Value,
            layout_view: EffectArgLayoutView::Direct,
        })
    }

    fn direct_arg_style_for_provider(
        &self,
        provider: ProvidedEffect<'db>,
        target_ty: TyId<'db>,
        _: bool,
    ) -> Option<EffectArgStyle> {
        if matches!(provider.origin, EffectOrigin::RootProvider { .. }) {
            return Some(EffectArgStyle::Value);
        }
        let target_ty = normalize_ty(
            self.db,
            target_ty,
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                self.env.scope(),
                self.env.assumptions(),
            ),
        );
        match provider_semantics(self.db, self.env.scope(), self.env.assumptions(), target_ty)
            .evidence
        {
            ProviderLayoutEvidence::ResolvedHandle(_)
            | ProviderLayoutEvidence::TraitBoundHandle(_) => {
                return Some(EffectArgStyle::Value);
            }
            ProviderLayoutEvidence::InvalidHandle(_) => return None,
            ProviderLayoutEvidence::Capability
            | ProviderLayoutEvidence::NotHandle
            | ProviderLayoutEvidence::ContractField => {}
        }
        let place = match provider.origin {
            EffectOrigin::RootProvider { .. } => None,
            EffectOrigin::With { value_expr } => self.env.expr_place(value_expr),
            EffectOrigin::Param { .. } => provider
                .binding
                .map(|binding| Place::new(PlaceBase::Binding(binding))),
        };
        Some(match place {
            Some(_) => EffectArgStyle::Place,
            None if matches!(provider.origin, EffectOrigin::With { .. }) => {
                EffectArgStyle::TempPlace
            }
            None => return None,
        })
    }

    fn effect_arg_for_provider(
        &mut self,
        provider: ProvidedEffect<'db>,
        arg_style: EffectArgStyle,
        required_mut: bool,
    ) -> (super::EffectArg<'db>, super::EffectPassMode) {
        if let EffectOrigin::RootProvider { site, provider_idx } = provider.origin {
            return (
                super::EffectArg::RootProvider { site, provider_idx },
                super::EffectPassMode::ByValue,
            );
        }
        if required_mut && !self.provider_supports_mut(provider) {
            return (super::EffectArg::Unknown, super::EffectPassMode::Unknown);
        }

        match arg_style {
            EffectArgStyle::Place => {
                let place = match provider.origin {
                    EffectOrigin::With { value_expr } => self.env.expr_place(value_expr),
                    EffectOrigin::RootProvider { .. } => {
                        unreachable!("root provider handled above")
                    }
                    EffectOrigin::Param { .. } => provider
                        .binding
                        .map(|binding| Place::new(PlaceBase::Binding(binding))),
                };
                (
                    place.map_or(super::EffectArg::Unknown, super::EffectArg::Place),
                    super::EffectPassMode::ByPlace,
                )
            }
            EffectArgStyle::TempPlace => match provider.origin {
                EffectOrigin::With { value_expr } => (
                    super::EffectArg::Value(value_expr),
                    super::EffectPassMode::ByTempPlace,
                ),
                EffectOrigin::RootProvider { .. } => unreachable!("root provider handled above"),
                EffectOrigin::Param { .. } => {
                    (super::EffectArg::Unknown, super::EffectPassMode::Unknown)
                }
            },
            EffectArgStyle::Value => (
                match provider.origin {
                    EffectOrigin::With { value_expr } => super::EffectArg::Value(value_expr),
                    EffectOrigin::RootProvider { .. } => {
                        unreachable!("root provider handled above")
                    }
                    EffectOrigin::Param { .. } => provider
                        .binding
                        .map_or(super::EffectArg::Unknown, super::EffectArg::Binding),
                },
                super::EffectPassMode::ByValue,
            ),
        }
    }

    fn provider_supports_mut(&mut self, provider: ProvidedEffect<'db>) -> bool {
        if let Some((kind, _)) = provider.ty.as_capability(self.db) {
            return matches!(kind, CapabilityKind::Mut)
                || self
                    .effect_provider_target_resolution(provider.ty, true)
                    .is_some();
        }

        provider.is_mut
            || self
                .effect_provider_target_resolution(provider.ty, true)
                .is_some()
    }

    fn inferred_provider_ty_for_effect_arg(
        &mut self,
        provider: ProvidedEffect<'db>,
        arg: &super::EffectArg<'db>,
        pass_mode: super::EffectPassMode,
        provider_target_ty: Option<TyId<'db>>,
    ) -> Option<TyId<'db>> {
        if let super::EffectArg::RootProvider { site, provider_idx } = arg {
            return self
                .env
                .provider_binding(*site, *provider_idx)
                .map(|p| p.provider_ty);
        }
        match pass_mode {
            super::EffectPassMode::ByValue => Some(self.table.fold_ty(self.db, provider.ty)),
            super::EffectPassMode::ByTempPlace => {
                let target_ty = self.table.fold_ty(self.db, provider_target_ty?);
                Some(TyId::ptr_to(self.db, target_ty))
            }
            super::EffectPassMode::ByPlace => {
                let binding = match arg {
                    super::EffectArg::Place(place) => {
                        let PlaceBase::Binding(binding) = place.base;
                        Some(binding)
                    }
                    super::EffectArg::Binding(binding) => Some(*binding),
                    super::EffectArg::RootProvider { .. }
                    | super::EffectArg::Value(_)
                    | super::EffectArg::Unknown => provider.binding,
                }?;

                let inferred = match binding {
                    LocalBinding::EffectParam { site, idx, .. } => self
                        .env
                        .resolved_provider_binding(site, idx)
                        .map_or(provider.ty, |binding| binding.provider_ty),
                    LocalBinding::Param {
                        site: ParamSite::EffectField(effect_site),
                        ..
                    } => {
                        let contract = match effect_site {
                            EffectParamSite::Contract(contract)
                            | EffectParamSite::ContractInit { contract }
                            | EffectParamSite::ContractRecvArm { contract, .. } => contract,
                            EffectParamSite::Func(_) => {
                                unreachable!(
                                    "effect field bindings cannot originate from function sites"
                                )
                            }
                        };
                        let ident = binding.binding_name(&self.env);
                        contract
                            .fields(self.db)
                            .get(&ident)
                            .map(|field| field.declared_shape_ty())?
                    }
                    LocalBinding::Local { .. } | LocalBinding::Param { .. } => provider.ty,
                };

                Some(self.table.fold_ty(self.db, inferred))
            }
            super::EffectPassMode::Unknown => None,
        }
    }

    fn provider_target_ty_for_effect_arg(
        &mut self,
        provider: ProvidedEffect<'db>,
        instantiated_key_ty: Option<TyId<'db>>,
        required_mut: bool,
    ) -> Option<TyId<'db>> {
        let instantiated_key_ty = instantiated_key_ty.map(|ty| self.table.fold_ty(self.db, ty));
        if let Some(target_ty) = instantiated_key_ty {
            let semantics =
                provider_semantics(self.db, self.env.scope(), self.env.assumptions(), target_ty);
            if matches!(
                semantics.evidence,
                ProviderLayoutEvidence::ResolvedHandle(_)
                    | ProviderLayoutEvidence::TraitBoundHandle(_)
            ) {
                return semantics
                    .target_ty
                    .map(|target_ty| self.table.fold_ty(self.db, target_ty));
            }
            if matches!(semantics.evidence, ProviderLayoutEvidence::InvalidHandle(_)) {
                return None;
            }
        }
        let provider_ty = self.table.fold_ty(self.db, provider.ty);
        self.effect_provider_target_resolution(provider_ty, required_mut)
            .map(|resolution| self.table.fold_ty(self.db, resolution.target_ty))
            .or(instantiated_key_ty)
    }

    fn effect_arg_is_valid(
        &self,
        arg: &super::EffectArg<'db>,
        pass_mode: super::EffectPassMode,
    ) -> bool {
        match pass_mode {
            super::EffectPassMode::ByPlace => matches!(arg, &super::EffectArg::Place(_)),
            super::EffectPassMode::ByTempPlace => matches!(arg, &super::EffectArg::Value(_)),
            super::EffectPassMode::ByValue => !matches!(
                arg,
                &super::EffectArg::Unknown | &super::EffectArg::Place(_)
            ),
            super::EffectPassMode::Unknown => false,
        }
    }

    fn effect_arg_provider_space(
        &self,
        arg: &super::EffectArg<'db>,
        pass_mode: super::EffectPassMode,
    ) -> Option<super::ProviderAddressSpace> {
        match pass_mode {
            super::EffectPassMode::ByTempPlace => match arg {
                super::EffectArg::Value(expr) => self.env.typed_expr(*expr).and_then(|prop| {
                    prop.borrow_provider
                        .or_else(|| self.concrete_borrow_provider_for_effect_handle_ty(prop.ty))
                }),
                super::EffectArg::Place(place) => self.concrete_borrow_provider_for_place(place),
                super::EffectArg::Binding(binding) => {
                    self.concrete_borrow_provider_for_binding(*binding)
                }
                super::EffectArg::RootProvider { site, provider_idx } => self
                    .env
                    .provider_binding(*site, *provider_idx)
                    .and_then(|p| p.semantics.address_space),
                super::EffectArg::Unknown => None,
            },
            super::EffectPassMode::ByPlace => match arg {
                super::EffectArg::Place(place) => self.concrete_borrow_provider_for_place(place),
                super::EffectArg::Binding(binding) => {
                    self.concrete_borrow_provider_for_binding(*binding)
                }
                super::EffectArg::RootProvider { .. }
                | super::EffectArg::Value(_)
                | super::EffectArg::Unknown => None,
            },
            super::EffectPassMode::ByValue => match arg {
                super::EffectArg::Place(place) => self.concrete_borrow_provider_for_place(place),
                super::EffectArg::Value(expr) => self.env.typed_expr(*expr).and_then(|prop| {
                    prop.borrow_provider
                        .or_else(|| self.concrete_borrow_provider_for_effect_handle_ty(prop.ty))
                }),
                super::EffectArg::Binding(binding) => {
                    self.concrete_borrow_provider_for_binding(*binding)
                }
                super::EffectArg::RootProvider { site, provider_idx } => self
                    .env
                    .provider_binding(*site, *provider_idx)
                    .and_then(|p| p.semantics.address_space),
                super::EffectArg::Unknown => None,
            },
            super::EffectPassMode::Unknown => None,
        }
    }

    fn effect_arg_provider_space_can_remain_unresolved(&self, arg: &super::EffectArg<'db>) -> bool {
        let binding = match arg {
            super::EffectArg::Place(place) => {
                let PlaceBase::Binding(binding) = place.base;
                Some(binding)
            }
            super::EffectArg::Binding(binding) => Some(*binding),
            super::EffectArg::RootProvider { .. }
            | super::EffectArg::Value(_)
            | super::EffectArg::Unknown => None,
        };

        matches!(binding, Some(LocalBinding::EffectParam { .. }))
    }

    fn specialize_effect_provider_binding(
        &mut self,
        slot: ProviderBinding<'db>,
        provided: ProvidedEffect<'db>,
        arg: &super::EffectArg<'db>,
        pass_mode: super::EffectPassMode,
        provider_target_ty: Option<TyId<'db>>,
        provider_space: Option<super::ProviderAddressSpace>,
    ) -> EffectProviderSpecialization<'db> {
        let provenance = self
            .effect_provider_provenance(provided, arg)
            .unwrap_or_else(|| {
                panic!(
                    "missing call-site provider provenance for {:?} in {:?}",
                    slot.provider_idx,
                    self.env.owner(),
                )
            });
        let target_ty = provider_target_ty.map(|ty| self.table.fold_ty(self.db, ty));
        let provider = self
            .existing_provider_binding_for_effect_arg(provided, arg)
            .filter(|provider| {
                target_ty.is_none_or(|target_ty| provider.effective_target_ty() == target_ty)
            })
            .map_or_else(
                || {
                    let provider_ty = self
                        .inferred_provider_ty_for_effect_arg(
                            provided,
                            arg,
                            pass_mode,
                            provider_target_ty,
                        )
                        .unwrap_or_else(|| self.table.fold_ty(self.db, provided.ty));
                    let semantics = provider_semantics_for_specialized_call(
                        self.db,
                        self.env.scope(),
                        self.env.assumptions(),
                        provider_ty,
                        target_ty,
                        provider_space,
                        match pass_mode {
                            super::EffectPassMode::ByPlace => ProviderTransport::ByPlace,
                            super::EffectPassMode::ByTempPlace => ProviderTransport::ByTempPlace,
                            super::EffectPassMode::ByValue | super::EffectPassMode::Unknown => {
                                ProviderTransport::ByValue
                            }
                        },
                    );
                    ProviderBinding {
                        provider_idx: slot.provider_idx,
                        provider_ty,
                        is_mut: provided.is_mut,
                        source: slot.source,
                        semantics,
                        layout_env: None,
                    }
                },
                |provider| ProviderBinding {
                    provider_idx: slot.provider_idx,
                    ..provider
                },
            );
        EffectProviderSpecialization {
            provider,
            provenance,
        }
    }

    fn existing_provider_binding_for_effect_arg(
        &self,
        provided: ProvidedEffect<'db>,
        arg: &super::EffectArg<'db>,
    ) -> Option<ProviderBinding<'db>> {
        if let super::EffectArg::RootProvider { site, provider_idx } = arg {
            return self.env.provider_binding(*site, *provider_idx);
        }
        let binding = match arg {
            super::EffectArg::Place(place) => {
                if !place.projections.is_empty() {
                    return None;
                }
                let PlaceBase::Binding(binding) = place.base;
                Some(binding)
            }
            super::EffectArg::Binding(binding) => Some(*binding),
            super::EffectArg::RootProvider { .. }
            | super::EffectArg::Value(_)
            | super::EffectArg::Unknown => provided.binding,
        }?;
        match binding {
            LocalBinding::EffectParam {
                site, provider_idx, ..
            } => self.env.provider_binding(site, provider_idx),
            LocalBinding::Param {
                site: ParamSite::EffectField(effect_site),
                idx,
                ..
            } => self.env.resolved_provider_binding(effect_site, idx),
            LocalBinding::Local { .. } | LocalBinding::Param { .. } => None,
        }
    }

    fn effect_provider_provenance(
        &self,
        provided: ProvidedEffect<'db>,
        arg: &super::EffectArg<'db>,
    ) -> Option<EffectProviderProvenance<'db>> {
        if let super::EffectArg::RootProvider { site, provider_idx } = arg {
            return Some(EffectProviderProvenance::RootProvider {
                owner: self.env.owner(),
                site: *site,
                provider_idx: *provider_idx,
            });
        }
        let owner = self.env.owner();
        let binding = match arg {
            super::EffectArg::Place(place) => {
                if !place.projections.is_empty()
                    && let EffectOrigin::With { value_expr } = provided.origin
                {
                    return Some(EffectProviderProvenance::Expr {
                        owner,
                        expr: value_expr,
                    });
                }
                let PlaceBase::Binding(binding) = place.base;
                Some(binding)
            }
            super::EffectArg::Binding(binding) => Some(*binding),
            super::EffectArg::RootProvider { .. }
            | super::EffectArg::Value(_)
            | super::EffectArg::Unknown => provided.binding,
        };
        binding
            .map(|binding| EffectProviderProvenance::Binding { owner, binding })
            .or(match provided.origin {
                EffectOrigin::With { value_expr } => Some(EffectProviderProvenance::Expr {
                    owner,
                    expr: value_expr,
                }),
                EffectOrigin::Param { .. } | EffectOrigin::RootProvider { .. } => None,
            })
    }

    fn query_type_key(&self, key: &EffectPatternKey<'db>) -> Option<TyId<'db>> {
        match key {
            EffectPatternKey::Type(key) => Some(key.carrier),
            EffectPatternKey::Trait(_) => None,
        }
    }

    fn trait_effect_goal_satisfiability(
        &self,
        trait_goal: TraitInstId<'db>,
    ) -> GoalSatisfiability<'db> {
        let solve_cx = self.env.solver_context();
        let query = solve_cx.goal_query(self.db, trait_goal);
        crate::analysis::ty::trait_resolution::is_goal_query_satisfiable(self.db, solve_cx, &query)
    }

    pub(super) fn commit_trait_goal_solution(
        &mut self,
        trait_goal: TraitInstId<'db>,
        solution: Solution<TraitGoalSolution<'db>>,
    ) -> TraitGoalSolution<'db> {
        let canonical_goal = Canonicalized::new(self.db, trait_goal);
        let solved = canonical_goal.extract_solution(&mut self.table, solution);
        self.table.unify(trait_goal, solved.inst).unwrap();
        solved
    }

    fn commit_provider_target_resolution(
        &mut self,
        resolution: ProviderTargetResolution<'db>,
    ) -> TyId<'db> {
        self.commit_provider_target_resolution_in_scope(
            resolution,
            self.env.scope(),
            self.env.assumptions(),
        )
    }

    pub(super) fn commit_provider_target_resolution_in_scope(
        &mut self,
        resolution: ProviderTargetResolution<'db>,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
    ) -> TyId<'db> {
        let mut target_ty = self.renormalize_effect_provider_target_ty_in_scope(
            resolution.target_seed_ty,
            scope,
            assumptions,
        );
        if let Some((handle_goal, handle_solution)) = resolution.handle_proof {
            self.commit_trait_goal_solution(handle_goal, handle_solution);
            target_ty =
                self.renormalize_effect_provider_target_ty_in_scope(target_ty, scope, assumptions);
        }
        if let Some((effect_ref_goal, effect_ref_solution)) = resolution.effect_ref_proof {
            self.commit_trait_goal_solution(effect_ref_goal, effect_ref_solution);
            target_ty =
                self.renormalize_effect_provider_target_ty_in_scope(target_ty, scope, assumptions);
        }
        if let Some((effect_ref_mut_goal, effect_ref_mut_solution)) =
            resolution.effect_ref_mut_proof
        {
            self.commit_trait_goal_solution(effect_ref_mut_goal, effect_ref_mut_solution);
            target_ty =
                self.renormalize_effect_provider_target_ty_in_scope(target_ty, scope, assumptions);
        }
        target_ty
    }

    fn renormalize_effect_provider_target_ty_in_scope(
        &mut self,
        target_ty: TyId<'db>,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
    ) -> TyId<'db> {
        normalize_ty(
            self.db,
            self.table.fold_ty(self.db, target_ty),
            crate::analysis::semantic::SolverPremises::from_unpositioned_pair(scope, assumptions),
        )
        .fold_with(self.db, &mut self.table)
    }

    fn select_type_effect_binding_match(
        &mut self,
        pattern: TypePatternKey<'db>,
        provided: ProvidedEffect<'db>,
    ) -> Option<TypeEffectBindingMatch<'db>> {
        self.select_type_effect_binding_match_in_scope(
            pattern,
            provided,
            self.env.scope(),
            self.env.assumptions(),
        )
    }

    pub(super) fn select_type_effect_binding_match_in_scope(
        &mut self,
        pattern: TypePatternKey<'db>,
        provided: ProvidedEffect<'db>,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
    ) -> Option<TypeEffectBindingMatch<'db>> {
        let can_commit_key_relation = |this: &mut Self, given: TyId<'db>| {
            let snapshot = this.snapshot_state();
            let ok = apply_key_match_commit(
                this,
                KeyMatchCommit::QueryToType {
                    query: pattern.clone(),
                    actual: given,
                },
            );
            this.rollback_state(snapshot);
            ok
        };

        let matches_key = |this: &mut Self, actual_ty: TyId<'db>| {
            if can_commit_key_relation(this, actual_ty) {
                return true;
            }

            false
        };
        let direct_ty = if let Some((_, inner)) = provided.ty.as_capability(self.db) {
            inner
        } else {
            provided.ty
        };

        if matches_key(self, direct_ty) {
            return Some(TypeEffectBindingMatch::Direct { given: direct_ty });
        }

        self.effect_provider_target_resolution_in_scope(provided.ty, false, scope, assumptions)
            .and_then(|resolution| {
                matches_key(self, resolution.target_ty)
                    .then_some(TypeEffectBindingMatch::Provider { resolution })
            })
    }

    fn effect_provider_target_resolution(
        &mut self,
        provided_ty: TyId<'db>,
        required_mut: bool,
    ) -> Option<ProviderTargetResolution<'db>> {
        self.effect_provider_target_resolution_in_scope(
            provided_ty,
            required_mut,
            self.env.scope(),
            self.env.assumptions(),
        )
    }

    fn effect_provider_target_resolution_in_scope(
        &mut self,
        provided_ty: TyId<'db>,
        required_mut: bool,
        scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
    ) -> Option<ProviderTargetResolution<'db>> {
        if let Some((kind, inner_ty)) = provided_ty.as_capability(self.db) {
            if required_mut && !matches!(kind, CapabilityKind::Mut) {
                return None;
            }
            return Some(ProviderTargetResolution::direct(inner_ty));
        }

        let effect_ref_trait = resolve_core_trait(self.db, scope, &["EffectRef"])
            .expect("missing required core trait `core::EffectRef`");
        let effect_ref_mut_trait = resolve_core_trait(self.db, scope, &["EffectRefMut"])
            .expect("missing required core trait `core::EffectRefMut`");
        let effect_handle_trait = resolve_core_trait(self.db, scope, &["EffectHandle"])
            .expect("missing required core trait `core::EffectHandle`");
        let target_ident = IdentId::new(self.db, "Target".to_string());
        let effect_handle_inst = TraitInstId::new(
            self.db,
            effect_handle_trait,
            vec![provided_ty],
            IndexMap::new(),
        );
        let GoalSatisfiability::Satisfied(handle_solution) =
            self.trait_effect_goal_satisfiability(effect_handle_inst)
        else {
            return None;
        };

        let snapshot = self.snapshot_state();
        let resolution = (|| {
            self.commit_trait_goal_solution(effect_handle_inst, handle_solution);

            let target_assoc = effect_handle_inst.project_assoc_ty(self.db, target_ident)?;
            let mut target_ty = normalize_ty(
                self.db,
                target_assoc,
                crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                    scope,
                    assumptions,
                ),
            )
            .fold_with(self.db, &mut self.table);
            let mut provided_ty = self.table.fold_ty(self.db, provided_ty);

            let effect_ref_inst = TraitInstId::new(
                self.db,
                effect_ref_trait,
                vec![provided_ty, target_ty],
                IndexMap::new(),
            );
            let GoalSatisfiability::Satisfied(effect_ref_solution) =
                self.trait_effect_goal_satisfiability(effect_ref_inst)
            else {
                return None;
            };
            self.commit_trait_goal_solution(effect_ref_inst, effect_ref_solution);
            provided_ty = self.table.fold_ty(self.db, provided_ty);
            target_ty =
                self.renormalize_effect_provider_target_ty_in_scope(target_ty, scope, assumptions);

            let effect_ref_mut_proof = if required_mut {
                let effect_ref_mut_inst = TraitInstId::new(
                    self.db,
                    effect_ref_mut_trait,
                    vec![provided_ty, target_ty],
                    IndexMap::new(),
                );
                let GoalSatisfiability::Satisfied(effect_ref_mut_solution) =
                    self.trait_effect_goal_satisfiability(effect_ref_mut_inst)
                else {
                    return None;
                };
                self.commit_trait_goal_solution(effect_ref_mut_inst, effect_ref_mut_solution);
                target_ty = self.renormalize_effect_provider_target_ty_in_scope(
                    target_ty,
                    scope,
                    assumptions,
                );
                Some((effect_ref_mut_inst, effect_ref_mut_solution))
            } else {
                None
            };

            Some(ProviderTargetResolution {
                target_ty,
                target_seed_ty: target_assoc,
                handle_proof: Some((effect_handle_inst, handle_solution)),
                effect_ref_proof: Some((effect_ref_inst, effect_ref_solution)),
                effect_ref_mut_proof,
            })
        })();
        self.rollback_state(snapshot);
        resolution
    }

    pub(super) fn apply_effect_commit_plan(&mut self, commit: EffectCommitPlan<'db>) -> bool {
        let snapshot = self.snapshot_state();
        let ok = self.apply_effect_commit_plan_inner(commit);
        if ok {
            self.commit_state(snapshot);
        } else {
            self.rollback_state(snapshot);
        }
        ok
    }

    fn apply_effect_commit_plan_inner(&mut self, commit: EffectCommitPlan<'db>) -> bool {
        for (goal, solution) in commit.trait_solutions {
            self.commit_trait_goal_solution(goal, solution);
        }
        if let Some(resolution) = commit.provider_resolution {
            self.commit_provider_target_resolution(resolution);
        }
        if let Some(key_match) = commit.key_match
            && !apply_key_match_commit(self, key_match)
        {
            return false;
        }
        commit
            .extra_unifications
            .into_iter()
            .all(|(expected, given)| self.table.unify(expected, given).is_ok())
    }

    fn validate_keyed_with(
        &mut self,
        key_path: PathId<'db>,
        provider: ProvidedEffect<'db>,
        span: DynLazySpan<'db>,
    ) -> Result<
        (
            EffectWitness<'db, ProvidedEffect<'db>>,
            EffectCommitPlan<'db>,
        ),
        Box<EffectBarrier<'db>>,
    > {
        let Some(pattern) = build_barrier_pattern_for_with_key(self, key_path) else {
            let fallback = build_conservative_same_family_barrier_pattern_in_scope(
                self.db,
                self.env.scope(),
                self.env.assumptions(),
                key_path,
            );
            if let Some(EffectPatternKey::Trait(pattern)) = fallback.clone() {
                self.push_diag(BodyDiag::WithEffectTraitUnsatisfied {
                    primary: span.clone(),
                    key: key_path,
                    trait_req: TraitInstId::new(
                        self.db,
                        pattern.def,
                        std::iter::once(provider.ty)
                            .chain(pattern.args_no_self.iter().copied())
                            .collect::<Vec<_>>(),
                        pattern
                            .assoc_bindings
                            .iter()
                            .copied()
                            .collect::<IndexMap<_, _>>(),
                    ),
                    given: provider.ty,
                });
                return Err(Box::new(EffectBarrier {
                    pattern: EffectPatternKey::Trait(pattern),
                    reason: BarrierReason::InvalidExplicitTraitKey { span, key_path },
                }));
            }
            let expected = TyId::invalid(self.db, InvalidCause::Other);
            self.push_diag(BodyDiag::WithEffectTypeUnsatisfied {
                primary: span.clone(),
                key: key_path,
                expected,
                given: provider.ty,
            });
            return Err(Box::new(EffectBarrier {
                pattern: fallback.unwrap_or(EffectPatternKey::Type(TypePatternKey {
                    carrier: expected,
                    family: crate::analysis::ty::effects::effect_family_for_type(self.db, expected),
                    slots: crate::analysis::ty::effects::PatternSlots::empty(),
                })),
                reason: BarrierReason::InvalidExplicitTypeKey { span, key_path },
            }));
        };

        self.build_keyed_witness_from_pattern_in_scope(
            pattern,
            key_path,
            provider,
            span,
            KeyedWitnessBuildOptions {
                scope: KeyedWitnessBuildScope {
                    scope: self.env.scope(),
                    assumptions: self.env.base_assumptions(),
                },
                emit_diag: true,
                mode: WitnessBuildMode::ExplicitKeyedWith,
            },
        )
    }

    pub(super) fn build_keyed_witness_from_pattern_in_scope(
        &mut self,
        pattern: EffectPatternKey<'db>,
        key_path: PathId<'db>,
        provider: ProvidedEffect<'db>,
        span: DynLazySpan<'db>,
        options: KeyedWitnessBuildOptions<'db>,
    ) -> Result<
        (
            EffectWitness<'db, ProvidedEffect<'db>>,
            EffectCommitPlan<'db>,
        ),
        Box<EffectBarrier<'db>>,
    > {
        let KeyedWitnessBuildOptions {
            scope: KeyedWitnessBuildScope { scope, assumptions },
            emit_diag,
            mode,
        } = options;
        match pattern {
            EffectPatternKey::Type(pattern) => {
                if matches!(mode, WitnessBuildMode::ExplicitKeyedWith)
                    && pattern
                        .slots
                        .entries
                        .iter()
                        .any(|slot| slot.kind == PatternSlotKind::OmittedExplicitArg)
                {
                    if emit_diag {
                        self.push_diag(BodyDiag::WithEffectTypeUnsatisfied {
                            primary: span.clone(),
                            key: key_path,
                            expected: pattern.carrier,
                            given: provider.ty,
                        });
                    }
                    return Err(Box::new(EffectBarrier {
                        pattern: EffectPatternKey::Type(pattern),
                        reason: BarrierReason::InvalidExplicitTypeKey {
                            span: span.clone(),
                            key_path,
                        },
                    }));
                }
                let Some(binding_match) =
                    self.select_type_effect_binding_match(pattern.clone(), provider)
                else {
                    if emit_diag {
                        self.push_diag(BodyDiag::WithEffectTypeUnsatisfied {
                            primary: span.clone(),
                            key: key_path,
                            expected: pattern.carrier,
                            given: provider.ty,
                        });
                    }
                    return Err(Box::new(EffectBarrier {
                        pattern: EffectPatternKey::Type(pattern),
                        reason: BarrierReason::InvalidExplicitTypeKey {
                            span: span.clone(),
                            key_path,
                        },
                    }));
                };
                let (actual, provider_resolution, transport) = match binding_match {
                    TypeEffectBindingMatch::Direct { given } => {
                        (given, None, WitnessTransport::Direct)
                    }
                    TypeEffectBindingMatch::Provider { resolution } => (
                        resolution.target_ty,
                        Some(resolution),
                        WitnessTransport::ByValue,
                    ),
                };
                let snapshot = self.snapshot_state();
                let specialized = self
                    .specialize_type_pattern_key(pattern.clone(), actual)
                    .map(|carrier| StoredTypeKey {
                        carrier,
                        family: pattern.family,
                    })
                    .and_then(|stored| {
                        finalize_stored_effect_key(
                            self.db,
                            StoredEffectKey::Type(stored),
                            scope,
                            assumptions,
                        )
                    });
                self.rollback_state(snapshot);
                let Some(key) = specialized.filter(|key| {
                    !matches!(
                        (mode, key),
                        (WitnessBuildMode::ExplicitKeyedWith, StoredEffectKey::Type(stored))
                            if stored_value_contains_implicit_layout_params(
                                self.db,
                                stored.carrier,
                            ) || stored_value_contains_out_of_scope_params(
                                self.db,
                                scope,
                                stored.carrier,
                            )
                    )
                }) else {
                    if emit_diag {
                        self.push_diag(BodyDiag::WithEffectTypeUnsatisfied {
                            primary: span.clone(),
                            key: key_path,
                            expected: pattern.carrier,
                            given: provider.ty,
                        });
                    }
                    return Err(Box::new(EffectBarrier {
                        pattern: EffectPatternKey::Type(pattern),
                        reason: BarrierReason::InvalidExplicitTypeKey {
                            span: span.clone(),
                            key_path,
                        },
                    }));
                };
                Ok((
                    EffectWitness {
                        key,
                        provider,
                        transport,
                    },
                    EffectCommitPlan {
                        key_match: Some(KeyMatchCommit::QueryToType {
                            query: pattern,
                            actual,
                        }),
                        trait_solutions: SmallVec::new(),
                        provider_resolution,
                        extra_unifications: SmallVec::new(),
                    },
                ))
            }
            EffectPatternKey::Trait(pattern) => {
                if matches!(mode, WitnessBuildMode::ExplicitKeyedWith)
                    && pattern
                        .slots
                        .entries
                        .iter()
                        .any(|slot| slot.kind == PatternSlotKind::OmittedExplicitArg)
                {
                    let trait_req = TraitInstId::new(
                        self.db,
                        pattern.def,
                        std::iter::once(provider.ty)
                            .chain(pattern.args_no_self.iter().copied())
                            .collect::<Vec<_>>(),
                        pattern
                            .assoc_bindings
                            .iter()
                            .copied()
                            .collect::<IndexMap<_, _>>(),
                    );
                    if emit_diag {
                        self.push_diag(BodyDiag::WithEffectTraitUnsatisfied {
                            primary: span.clone(),
                            key: key_path,
                            trait_req,
                            given: provider.ty,
                        });
                    }
                    return Err(Box::new(EffectBarrier {
                        pattern: EffectPatternKey::Trait(pattern),
                        reason: BarrierReason::InvalidExplicitTraitKey {
                            span: span.clone(),
                            key_path,
                        },
                    }));
                }
                if provider.ty.has_var(self.db) {
                    if emit_diag {
                        self.push_diag(BodyDiag::TypeAnnotationNeeded {
                            span: span.clone(),
                            ty: provider.ty,
                        });
                    }
                    return Err(Box::new(EffectBarrier {
                        pattern: EffectPatternKey::Trait(pattern),
                        reason: BarrierReason::UnstableExplicitKeyedProvider {
                            span: span.clone(),
                            key_path,
                        },
                    }));
                }
                let mut table = UnificationTable::new(self.db);
                let (instantiated, slot_bindings) = instantiate_trait_pattern_in_with_bindings(
                    self.db,
                    &mut table,
                    pattern.clone(),
                );
                let args = std::iter::once(provider.ty)
                    .chain(instantiated.args(self.db).iter().skip(1).copied())
                    .collect::<Vec<_>>();
                let trait_goal = TraitInstId::new(
                    self.db,
                    instantiated.def(self.db),
                    args,
                    instantiated.assoc_type_bindings(self.db).clone(),
                );
                let proof = if matches!(mode, WitnessBuildMode::SeededRequirement) {
                    self.env
                        .solver_context()
                        .provider_implementation_proof(self.db, trait_goal)
                } else {
                    self.trait_effect_goal_satisfiability(trait_goal)
                };
                let solved_inst = if let GoalSatisfiability::Satisfied(solution) = proof {
                    let canonical_goal = Canonicalized::new(self.db, trait_goal);
                    let solved = canonical_goal.extract_solution(&mut table, solution);
                    Some(
                        solved
                            .implementor
                            .trait_inst(self.db)
                            .fold_with(self.db, &mut table),
                    )
                } else {
                    None
                };
                let Some(solved_inst) = solved_inst else {
                    if emit_diag {
                        self.push_diag(BodyDiag::WithEffectTraitUnsatisfied {
                            primary: span.clone(),
                            key: key_path,
                            trait_req: trait_goal,
                            given: provider.ty,
                        });
                    }
                    return Err(Box::new(EffectBarrier {
                        pattern: EffectPatternKey::Trait(pattern),
                        reason: BarrierReason::InvalidExplicitTraitKey {
                            span: span.clone(),
                            key_path,
                        },
                    }));
                };
                let instantiated = reify_unresolved_pattern_slots(
                    self.db,
                    instantiated.fold_with(self.db, &mut table),
                    &slot_bindings,
                );
                let specialized = if matches!(mode, WitnessBuildMode::SeededRequirement)
                    && (stored_value_contains_implicit_layout_params(self.db, solved_inst)
                        || stored_value_contains_out_of_scope_params(self.db, scope, solved_inst))
                {
                    instantiated
                } else {
                    solved_inst
                };
                let Some(key) = finalize_stored_effect_key(
                    self.db,
                    StoredEffectKey::Trait(StoredTraitKey {
                        def: specialized.def(self.db),
                        args_no_self: specialized.args(self.db)[1..].iter().copied().collect(),
                        assoc_bindings: specialized
                            .assoc_ty_bindings(self.db)
                            .into_iter()
                            .collect(),
                        family: pattern.family,
                    }),
                    scope,
                    assumptions,
                ) else {
                    return Err(Box::new(EffectBarrier {
                        pattern: EffectPatternKey::Trait(pattern),
                        reason: BarrierReason::InvalidExplicitTraitKey { span, key_path },
                    }));
                };
                Ok((
                    EffectWitness {
                        key,
                        provider,
                        transport: WitnessTransport::ByValue,
                    },
                    EffectCommitPlan {
                        key_match: None,
                        trait_solutions: SmallVec::new(),
                        provider_resolution: None,
                        extra_unifications: SmallVec::new(),
                    },
                ))
            }
        }
    }

    fn specialize_type_pattern_key(
        &mut self,
        pattern: TypePatternKey<'db>,
        actual: TyId<'db>,
    ) -> Option<TyId<'db>> {
        let key_match = KeyMatchCommit::QueryToType {
            query: pattern,
            actual,
        };
        apply_key_match_commit(self, key_match).then(|| self.table.fold_ty(self.db, actual))
    }

    fn barrier_reason_for_pattern(
        &self,
        key_path: PathId<'db>,
        key: StoredEffectKey<'db>,
        span: DynLazySpan<'db>,
    ) -> BarrierReason<'db> {
        match key {
            StoredEffectKey::Type(_) => BarrierReason::InvalidExplicitTypeKey { span, key_path },
            StoredEffectKey::Trait(_) => BarrierReason::InvalidExplicitTraitKey { span, key_path },
        }
    }

    fn insert_effect_barrier(&mut self, barrier: EffectBarrier<'db>) {
        let Some(barrier) = self.refine_effect_barrier(barrier) else {
            return;
        };
        self.env
            .effect_env_mut()
            .insert_barrier(barrier.pattern.clone().family(), barrier);
    }

    fn refine_effect_barrier(&self, barrier: EffectBarrier<'db>) -> Option<EffectBarrier<'db>> {
        if self.barrier_pattern_is_precise(&barrier.pattern) {
            return Some(barrier);
        }

        // Exact invalid keyed barriers participate only when precise; otherwise we downgrade to
        // a same-family conservative barrier so invalid explicit keyed bindings still shadow outer
        // providers conservatively.
        let key_path = match &barrier.reason {
            BarrierReason::InvalidExplicitTypeKey { key_path, .. }
            | BarrierReason::InvalidExplicitTraitKey { key_path, .. }
            | BarrierReason::UnstableExplicitKeyedProvider { key_path, .. } => *key_path,
        };
        let pattern = build_conservative_same_family_barrier_pattern_in_scope(
            self.db,
            self.env.scope(),
            self.env.assumptions(),
            key_path,
        )?;
        Some(EffectBarrier {
            pattern,
            reason: barrier.reason,
        })
    }

    fn barrier_pattern_is_precise(&self, pattern: &EffectPatternKey<'db>) -> bool {
        !query_contains_unresolved_inference(self.db, pattern)
            && !contains_projection_or_invalid_query_state(self.db, pattern)
    }

    fn specialize_same_trait_method_inst(
        &mut self,
        method_name: IdentId<'db>,
        inst: TraitInstId<'db>,
    ) -> TraitInstId<'db> {
        let Some(CallableDef::Func(current_func)) = self.env.func() else {
            return inst;
        };
        if current_func.name(self.db).to_opt() != Some(method_name) {
            return inst;
        }

        let Some(enclosing_inst) = (match current_func.scope().parent_item(self.db) {
            Some(ItemKind::Trait(trait_)) => Some(TraitInstId::new(
                self.db,
                trait_,
                trait_.params(self.db).to_vec(),
                IndexMap::new(),
            )),
            Some(ItemKind::ImplTrait(impl_trait)) => impl_trait.trait_inst_result(self.db).ok(),
            _ => None,
        }) else {
            return inst;
        };

        if enclosing_inst.def(self.db) != inst.def(self.db) {
            return inst;
        }

        let enclosing_args = enclosing_inst.args(self.db);
        let inst_args = inst.args(self.db);
        if inst_args.len() != enclosing_args.len() || inst_args.is_empty() {
            return inst;
        }

        // The enclosing impl's arguments fill in what method selection left
        // open, as when forwarding to a field. They must not replace
        // arguments it already found, as for `self.inner.eq(other.inner)`
        // inside `impl<T> Eq for Wrap<T>`.
        let snapshot = self.snapshot_state();
        let compatible = inst_args[1..]
            .iter()
            .zip(&enclosing_args[1..])
            .all(|(&arg, &enclosing)| self.table.unify(arg, enclosing).is_ok());
        self.rollback_state(snapshot);
        if !compatible {
            return inst;
        }

        let mut args = inst_args.to_vec();
        args[1..].clone_from_slice(&enclosing_args[1..]);
        TraitInstId::new(
            self.db,
            inst.def(self.db),
            args,
            enclosing_inst.assoc_type_bindings(self.db).clone(),
        )
    }

    fn check_method_call(&mut self, expr: ExprId, expr_data: &Expr<'db>) -> ExprProp<'db> {
        let Expr::MethodCall(receiver, method_name, generic_args, args) = expr_data else {
            unreachable!()
        };
        let call_span = expr.span(self.body()).into_method_call_expr();
        let Some(method_name) = method_name.to_opt() else {
            return ExprProp::invalid(self.db);
        };

        let receiver_prop = self.check_expr_unknown(*receiver);
        if receiver_prop.ty.has_invalid(self.db) {
            return ExprProp::invalid(self.db);
        }

        let receiver_tys = self.method_receiver_tys(*receiver, &receiver_prop);
        let lookup_scope = self
            .body()
            .quote_scope(self.db, expr)
            .unwrap_or(self.env.scope());
        let lookup = self.env.method_lookup(lookup_scope);

        let mut selected_receiver_ty = receiver_tys[0];
        let mut canonical_r_ty = Canonicalized::new(self.db, selected_receiver_ty);
        let mut candidate =
            select_method_candidate(self.db, &canonical_r_ty, method_name, lookup, None);
        if matches!(
            candidate,
            Err(MethodSelectionError::NotFound | MethodSelectionError::ReceiverTypeMustBeKnown)
        ) {
            for &receiver_ty in receiver_tys.iter().skip(1) {
                let fallback_canonical = Canonicalized::new(self.db, receiver_ty);
                let fallback = select_method_candidate(
                    self.db,
                    &fallback_canonical,
                    method_name,
                    lookup,
                    None,
                );

                if fallback.is_ok() || !matches!(fallback, Err(MethodSelectionError::NotFound)) {
                    selected_receiver_ty = receiver_ty;
                    canonical_r_ty = fallback_canonical;
                    candidate = fallback;
                    break;
                }
            }
        }
        let candidate = match candidate {
            Ok(candidate) => candidate,
            Err(err) => {
                if let MethodSelectionError::AmbiguousTraitMethod(ambiguous) = err {
                    // Defer resolution using return-type constraints
                    let ret_ty = self.fresh_ty();
                    let typed = ExprProp::new(ret_ty, true);
                    self.env.type_expr(expr, typed.clone());
                    // Still type-check argument expressions so they have types and can
                    // participate in later constraint solving.
                    for arg in args.iter() {
                        self.check_expr_unknown(arg.expr);
                    }
                    let candidates = ambiguous
                        .candidates
                        .into_iter()
                        .map(|candidate| {
                            let inst = canonical_r_ty
                                .extract_solution(&mut self.table, candidate.cand.inst);
                            let inst = self.specialize_same_trait_method_inst(method_name, inst);
                            super::env::PendingMethodCandidate {
                                inst,
                                method: candidate.cand.method,
                            }
                        })
                        .collect();

                    self.env.register_pending_method(super::env::PendingMethod {
                        expr,
                        recv_ty: selected_receiver_ty,
                        method_name,
                        candidates,
                        span: call_span.method_name().into(),
                    });
                    return typed;
                } else {
                    if matches!(&err, MethodSelectionError::NotFound)
                        && let Some(diag) = named_member_diagnostic(
                            self.db,
                            self.env.scope(),
                            selected_receiver_ty,
                            method_name,
                            call_span.clone().method_name().into(),
                        )
                    {
                        self.push_diag(diag);
                        return ExprProp::invalid(self.db);
                    }
                    let diag = body_diag_from_method_selection_err(
                        self.db,
                        err,
                        Spanned::new(selected_receiver_ty, receiver.span(self.body()).into()),
                        Spanned::new(method_name, call_span.method_name().into()),
                    );
                    self.push_diag(diag);
                    return ExprProp::invalid(self.db);
                }
            }
        };

        let (func_ty, trait_inst) = match candidate {
            MethodCandidate::InherentMethod(cand) => (
                self.extract_inherent_method_to_term(cand, selected_receiver_ty),
                None,
            ),

            MethodCandidate::TraitMethod(cand) => {
                let inst = canonical_r_ty.extract_solution(&mut self.table, cand.inst);
                let inst = self.specialize_same_trait_method_inst(method_name, inst);
                let func_ty =
                    self.instantiate_trait_method_to_term(cand.method, selected_receiver_ty, inst);
                (func_ty, Some(inst))
            }

            MethodCandidate::NeedsConfirmation(cand) => {
                let inst = canonical_r_ty.extract_solution(&mut self.table, cand.inst);
                let inst = self.specialize_same_trait_method_inst(method_name, inst);
                let func_ty =
                    self.instantiate_trait_method_to_term(cand.method, selected_receiver_ty, inst);
                (func_ty, Some(inst))
            }
        };

        let mut callable = match Callable::new(
            self.db,
            func_ty,
            receiver.span(self.body()).into(),
            trait_inst,
        ) {
            Ok(callable) => callable,
            Err(diag) => {
                self.push_diag(diag);
                return ExprProp::invalid(self.db);
            }
        };

        let anchor = HoleAnchor::BodySyntax {
            body: self.body(),
            site: BodyHoleSite::Expr(expr),
        };
        if !callable.unify_generic_args(
            self,
            *generic_args,
            anchor,
            call_span.clone().generic_args(),
        ) {
            return ExprProp::invalid(self.db);
        }

        if !callable.callable_def.is_method(self.db) {
            let diag = BodyDiag::NotAMethod {
                span: call_span,
                receiver_ty: receiver_prop.ty,
                func_name: method_name,
                func_ty,
            };
            self.push_diag(diag);
            return ExprProp::invalid(self.db);
        }

        if let Some(kind) = self
            .code_region_intrinsic_kind(callable.callable_def())
            .or_else(|| self.code_region_method_kind(selected_receiver_ty, method_name))
            && args.len() == 1
            && let Some(result) = self.check_code_region_intrinsic(
                expr,
                &mut callable,
                args,
                kind,
                Some((*receiver, receiver_prop.clone())),
                None,
            )
        {
            return result;
        }

        callable.check_args(
            self,
            args,
            call_span.clone().args(),
            Some((*receiver, receiver_prop)),
            false,
        );
        self.specialize_callable_layout_args(&mut callable, Some(*receiver), args);

        // Check required effects for the method call
        self.check_callable_effects(expr, &mut callable);

        callable.process_constraints(self, expr, call_span.method_name().into());

        let normalized_ret_ty = self.normalize_callable_return(&callable);
        self.check_evidence_call_source(expr, &callable, normalized_ret_ty, args, Some(*receiver));
        self.report_instantiated_const_fault(
            &callable,
            normalized_ret_ty,
            expr.span(self.body()).into(),
        );
        if !self.in_const_context() && self.callable_contains_quote(callable.callable_def()) {
            self.push_diag(BodyDiag::QuoteCallAtRuntime(expr.span(self.body()).into()));
            return ExprProp::invalid(self.db);
        }
        if !self.check_obligation_introduction(expr, &callable) {
            return ExprProp::invalid(self.db);
        }
        if let Some(kind) = self.const_intrinsic_kind(callable.callable_def()) {
            if !self.check_and_register_const_intrinsic(expr, callable, kind) {
                return ExprProp::invalid(self.db);
            }
        } else {
            self.env.register_semantic_call(expr, callable);
        }
        ExprProp::new(normalized_ret_ty, true)
    }

    fn method_receiver_tys(
        &self,
        _receiver: ExprId,
        receiver_prop: &ExprProp<'db>,
    ) -> Vec<TyId<'db>> {
        self.capability_fallback_candidates(receiver_prop.ty)
    }

    fn check_path(&mut self, expr: ExprId, expr_data: &Expr<'db>) -> ExprProp<'db> {
        let Expr::Path(path) = expr_data else {
            unreachable!()
        };

        let Partial::Present(path) = path else {
            return ExprProp::invalid(self.db);
        };
        let path = *path;

        let path_expr_span = expr.span(self.body()).into_path_expr();
        let path_span = path_expr_span.clone().path();

        let is_call_callee = self.env.parent_expr().is_some_and(|parent| {
            matches!(
                self.body().exprs(self.db)[parent],
                Partial::Present(Expr::Call(callee, _)) if callee == expr
            )
        });

        let idx = path.segment_index(self.db);
        let generic_args = path.generic_args(self.db);
        let generic_args_span = path_span.clone().segment(idx).generic_args();
        let anchor = HoleAnchor::BodySyntax {
            body: self.body(),
            site: BodyHoleSite::Expr(expr),
        };
        let minter = LoweringContext::new(anchor)
            .with_context(self.env.selection_context(), self.env.assumptions());
        let unify_generic_args = |tc: &mut Self, callable: &mut Callable<'db>| {
            callable.unify_generic_args(tc, generic_args, anchor, generic_args_span.clone())
        };

        let quote_scope = self.body().quote_scope(self.db, expr);
        let res = if path.is_bare_ident(self.db) && quote_scope.is_none() {
            let ident_span: DynLazySpan<'db> = path_expr_span.clone().into();
            resolve_ident_expr(self.db, &self.env, path, ident_span, &minter)
        } else {
            match self.resolve_path_with_visibility(
                path,
                true,
                path_span.clone(),
                path_expr_span.clone().into(),
                &minter,
                PathResolutionScopes {
                    lookup: self.env.scope(),
                    visibility: quote_scope.unwrap_or(self.env.scope()),
                },
            ) {
                Ok(r) => ResolvedPathInBody::Reso(r),
                Err(err) => {
                    let expected_kind = if is_call_callee {
                        ExpectedPathKind::Function
                    } else {
                        ExpectedPathKind::Value
                    };

                    let named_only = match (&err.kind, err.failed_at == path) {
                        (
                            PathResErrorKind::NotFound {
                                parent: Some(PathRes::Ty(ty) | PathRes::TyAlias(_, ty, _)),
                                ..
                            },
                            true,
                        ) => path.ident(self.db).to_opt().and_then(|member| {
                            named_member_diagnostic(
                                self.db,
                                self.env.scope(),
                                *ty,
                                member,
                                path_span.clone().segment(idx).ident().into(),
                            )
                        }),
                        _ => None,
                    };
                    if let Some(diag) = named_only
                        .or_else(|| err.into_diag(self.db, path, path_span.clone(), expected_kind))
                    {
                        self.push_diag(diag)
                    }
                    ResolvedPathInBody::Invalid
                }
            }
        };
        match res {
            ResolvedPathInBody::Binding(binding) => {
                let ty = self
                    .env
                    .lookup_binding_ty(&binding)
                    .fold_with(self.db, &mut self.table);
                let ty = self.normalize_ty(ty);
                let mut is_mut = binding.is_mut();
                if let Some((cap, _)) = ty.as_capability(self.db) {
                    is_mut = match cap {
                        CapabilityKind::Mut => true,
                        CapabilityKind::Ref => false,
                        CapabilityKind::View => binding.is_mut(),
                    };
                }
                ExprProp {
                    ty,
                    is_mut,
                    binding: Some(binding),
                    borrow_provider: self.concrete_borrow_provider_for_binding(binding),
                    path_read_semantics: None,
                }
            }
            ResolvedPathInBody::NewBinding(ident) => {
                let assoc_fn = enclosing_assoc_fn(self.db, self.body(), ident);
                let diag = BodyDiag::UndefinedVariable(path_expr_span.into(), ident, assoc_fn);
                self.push_diag(diag);

                ExprProp::invalid(self.db)
            }
            ResolvedPathInBody::Diag(diag) => {
                self.push_diag(diag);
                ExprProp::invalid(self.db)
            }
            ResolvedPathInBody::Invalid => ExprProp::invalid(self.db),

            ResolvedPathInBody::Reso(reso) => match reso {
                PathRes::Ty(ty) | PathRes::TyAlias(_, ty, _) => {
                    if let Some(const_ty_ty) = ty.const_ty_ty(self.db) {
                        self.env
                            .register_value_path_ref(expr, ValuePathRef::TypeConst(ty));
                        ExprProp::new(self.table.instantiate_to_term(const_ty_ty), true)
                    } else {
                        let diag = if ty.is_struct(self.db) {
                            let record_like = RecordLike::from_ty(ty);
                            BodyDiag::unit_variant_expected(
                                self.db,
                                path_expr_span.clone().into(),
                                record_like,
                            )
                        } else {
                            BodyDiag::NotValue {
                                primary: path_expr_span.clone().into(),
                                given: Either::Right(ty),
                            }
                        };
                        self.push_diag(diag);

                        ExprProp::invalid(self.db)
                    }
                }
                PathRes::Func(ty) => {
                    let ty = self.instantiate_to_term(ty);
                    let mut callable =
                        Callable::new(self.db, ty, expr.span(self.body()).into(), None)
                            .expect("function item path should resolve to callable");
                    if !unify_generic_args(self, &mut callable) {
                        return ExprProp::invalid(self.db);
                    }

                    if !is_call_callee {
                        // A function value owns its checked bounds at this path,
                        // just as a direct call owns them at its call expression.
                        callable.process_constraints(self, expr, path_expr_span.clone().into());
                    }
                    self.env
                        .register_value_path_ref(expr, ValuePathRef::FunctionItem);
                    ExprProp::new(callable.ty(self.db), true)
                }
                PathRes::Trait(trait_) => {
                    let diag = BodyDiag::NotValue {
                        primary: path_expr_span.clone().into(),
                        given: Either::Left(trait_.def(self.db).into()),
                    };
                    self.push_diag(diag);
                    ExprProp::invalid(self.db)
                }
                PathRes::EnumVariant(variant) => {
                    let ty = match variant.kind(self.db) {
                        VariantKind::Unit => {
                            self.env
                                .register_value_path_ref(expr, ValuePathRef::UnitVariant(variant));
                            variant.ty
                        }
                        VariantKind::Tuple(_) => {
                            self.env
                                .register_value_path_ref(expr, ValuePathRef::FunctionItem);
                            let ty = variant.constructor_func_ty(self.db).unwrap();
                            self.instantiate_to_term(ty)
                        }
                        VariantKind::Record(_) => {
                            let record_like = RecordLike::from_variant(variant);
                            let diag = BodyDiag::unit_variant_expected(
                                self.db,
                                expr.span(self.body()).into(),
                                record_like,
                            );
                            self.push_diag(diag);

                            TyId::invalid(self.db, InvalidCause::Other)
                        }
                    };

                    ExprProp::new(self.instantiate_to_term(ty), true)
                }
                PathRes::Const(const_def, ty) => {
                    self.env
                        .register_const_ref(expr, ConstRef::Const(const_def));
                    ExprProp::new(ty, true)
                }
                PathRes::Method(receiver_ty, candidate) => {
                    let canonical_r_ty = Canonicalized::new(self.db, receiver_ty);
                    let (method_ty, trait_inst) = match candidate {
                        MethodCandidate::InherentMethod(cand) => (
                            self.extract_inherent_method_to_term(cand, receiver_ty),
                            None,
                        ),
                        MethodCandidate::TraitMethod(cand)
                        | MethodCandidate::NeedsConfirmation(cand) => {
                            let inst = canonical_r_ty.extract_solution(&mut self.table, cand.inst);
                            let inst = if inst
                                .self_ty(self.db)
                                .kind(self.db)
                                .does_match(receiver_ty.kind(self.db))
                            {
                                let mut args = inst.args(self.db).to_vec();
                                if let Some(self_arg) = args.first_mut() {
                                    *self_arg = receiver_ty;
                                }
                                TraitInstId::new(
                                    self.db,
                                    inst.def(self.db),
                                    args,
                                    inst.assoc_type_bindings(self.db).clone(),
                                )
                            } else {
                                inst
                            };
                            if !is_call_callee
                                && matches!(candidate, MethodCandidate::NeedsConfirmation(_))
                            {
                                self.env.register_trait_obligation(TraitObligation {
                                    context: self.env.position_context(),
                                    goal: inst,
                                    origin: TraitObligationOrigin::GenericConfirmation,
                                    span: path_expr_span.clone().into(),
                                });
                            }
                            let method_ty = if cand.method.is_method(self.db) {
                                self.instantiate_trait_method_to_term(
                                    cand.method,
                                    receiver_ty,
                                    inst,
                                )
                            } else {
                                self.instantiate_trait_assoc_fn_to_term(
                                    cand.method.as_callable(self.db).unwrap(),
                                    inst,
                                )
                            };
                            (method_ty, Some(inst))
                        }
                    };

                    let mut callable = Callable::new(
                        self.db,
                        method_ty,
                        expr.span(self.body()).into(),
                        trait_inst,
                    )
                    .expect("method path should resolve to callable");

                    if !unify_generic_args(self, &mut callable) {
                        return ExprProp::invalid(self.db);
                    }

                    let method_ty = callable.ty(self.db);
                    self.env.register_callable(expr, callable);
                    self.env
                        .register_value_path_ref(expr, ValuePathRef::FunctionItem);
                    ExprProp::new(method_ty, true)
                }
                PathRes::TraitMethod(trait_inst, method) => {
                    if let Some(existing) = self.env.callable_expr(expr) {
                        return ExprProp::new(existing.ty(self.db), true);
                    }

                    let inst = if matches!(
                        trait_inst.self_ty(self.db).data(self.db),
                        TyData::TyParam(param) if param.is_trait_self()
                    ) {
                        let old_self = trait_inst.self_ty(self.db);
                        let new_self = self.table.new_var_from_param(old_self);

                        struct ReplaceSelf<'db> {
                            from: TyId<'db>,
                            to: TyId<'db>,
                        }

                        impl<'db> TyFolder<'db> for ReplaceSelf<'db> {
                            fn fold_ty(
                                &mut self,
                                db: &'db dyn HirAnalysisDb,
                                ty: TyId<'db>,
                            ) -> TyId<'db> {
                                if ty == self.from {
                                    return self.to;
                                }
                                ty.super_fold_with(db, self)
                            }
                        }

                        let mut folder = ReplaceSelf {
                            from: old_self,
                            to: new_self,
                        };

                        let args = trait_inst
                            .args(self.db)
                            .iter()
                            .map(|&ty| ty.fold_with(self.db, &mut folder))
                            .collect::<Vec<_>>();

                        let assoc_type_bindings: IndexMap<IdentId<'db>, TyId<'db>> = trait_inst
                            .assoc_type_bindings(self.db)
                            .iter()
                            .map(|(name, ty)| (*name, ty.fold_with(self.db, &mut folder)))
                            .collect();

                        TraitInstId::new(
                            self.db,
                            trait_inst.def(self.db),
                            args,
                            assoc_type_bindings,
                        )
                    } else {
                        trait_inst
                    };

                    // Direct calls confirm their originating trait in process_constraints,
                    // at the whole-call span. Standalone values own their path use here.
                    if !is_call_callee {
                        self.env.register_trait_obligation(TraitObligation {
                            context: self.env.position_context(),
                            goal: inst,
                            origin: TraitObligationOrigin::GenericConfirmation,
                            span: path_expr_span.clone().into(),
                        });
                    }

                    let func_ty = self.instantiate_trait_assoc_fn_to_term(
                        method.as_callable(self.db).unwrap(),
                        inst,
                    );

                    let mut callable =
                        Callable::new(self.db, func_ty, expr.span(self.body()).into(), Some(inst))
                            .expect("trait method path should resolve to callable");

                    if !unify_generic_args(self, &mut callable) {
                        return ExprProp::invalid(self.db);
                    }

                    let func_ty = callable.ty(self.db);
                    self.env.register_callable(expr, callable);
                    self.env
                        .register_value_path_ref(expr, ValuePathRef::FunctionItem);
                    ExprProp::new(func_ty, true)
                }
                PathRes::TraitConst(recv_ty, inst, name) => {
                    let mut args = inst.args(self.db).clone();
                    if let Some(self_arg) = args.first_mut() {
                        *self_arg = recv_ty;
                    }
                    let inst = TraitInstId::new(
                        self.db,
                        inst.def(self.db),
                        args,
                        inst.assoc_type_bindings(self.db).clone(),
                    );

                    let provision =
                        self.provision_for_path(path, inst, path_expr_span.clone().into());
                    if !self.check_trait_const_obligation(
                        inst,
                        path_expr_span.clone().into(),
                        provision,
                    ) {
                        return ExprProp::invalid(self.db);
                    }

                    self.env.register_const_ref(
                        expr,
                        ConstRef::TraitConst(
                            AssocConstUse::new(
                                crate::analysis::semantic::SolverPremises::from_unpositioned_pair(
                                    self.env.scope(),
                                    self.env.assumptions(),
                                ),
                                inst,
                                name,
                            )
                            .with_provision(provision)
                            .with_dependent_provisions(self.db, self.env.position_context()),
                        ),
                    );
                    // Look up the associated const's declared type in the trait and
                    // instantiate it with the trait instance's args (including Self).
                    let trait_ = inst.def(self.db);
                    if let Some(const_view) = trait_.const_(self.db, name)
                        && let Some(ty_binder) = const_view.ty_binder(self.db)
                    {
                        // Instantiate with the concrete args of the trait instance
                        let instantiated = ty_binder.instantiate(self.db, inst.args(self.db));
                        let ty = self.table.instantiate_to_term(instantiated);

                        ExprProp::new(ty, true)
                    } else {
                        // Fallback to invalid type if the declaration isn't found
                        ExprProp::invalid(self.db)
                    }
                }
                PathRes::InherentConst(recv_ty, impl_, name) => {
                    if let Some(ty) = instantiate_inherent_const_decl_ty(
                        self.db,
                        &mut self.table,
                        impl_,
                        recv_ty,
                        name,
                    ) {
                        self.env.register_const_ref(
                            expr,
                            ConstRef::InherentConst(InherentConstUse::new(
                                self.env.scope(),
                                self.env.assumptions(),
                                impl_,
                                recv_ty,
                                name,
                            )),
                        );
                        ExprProp::new(ty, true)
                    } else {
                        ExprProp::invalid(self.db)
                    }
                }
                PathRes::Mod(scope) => {
                    let diag = BodyDiag::NotValue {
                        primary: path_expr_span.clone().into(),
                        given: Either::Left(scope.item()),
                    };
                    self.push_diag(diag);
                    ExprProp::invalid(self.db)
                }
                PathRes::FuncParam(..) => {
                    // An anonymous const body can see the declaration's scope,
                    // but does not have its runtime parameter bindings.
                    self.push_diag(BodyDiag::ConstValueMustBeKnown(
                        path_expr_span.clone().into(),
                    ));
                    ExprProp::invalid(self.db)
                }
            },
        }
    }

    fn check_record_init(
        &mut self,
        expr: ExprId,
        expr_data: &Expr<'db>,
        expected: TyId<'db>,
    ) -> ExprProp<'db> {
        let Expr::RecordInit(path, ..) = expr_data else {
            unreachable!()
        };
        let span = expr.span(self.body()).into_record_init_expr();

        let Partial::Present(path) = path else {
            return ExprProp::invalid(self.db);
        };

        let path_span = span.clone().path();
        let minter = LoweringContext::new(HoleAnchor::BodySyntax {
            body: self.body(),
            site: BodyHoleSite::Expr(expr),
        })
        .with_context(self.env.selection_context(), self.env.assumptions());
        // `Self` refers to the generated impl, while field visibility still
        // uses the provider authority below. Issued fields also authorize their
        // declaring record's path; their identities are checked by feed_label.
        let scope = if path.is_self_ty(self.db) {
            self.env.scope()
        } else {
            self.body()
                .quote_authority(self.db, expr.into())
                .map(|authority| {
                    authority
                        .fields
                        .first()
                        .map(|grant| grant.field)
                        .unwrap_or(authority.scope)
                })
                .unwrap_or(self.env.scope())
        };
        let reso = match self.resolve_path_with_visibility(
            *path,
            true,
            path_span.clone(),
            span.clone().into(),
            &minter,
            PathResolutionScopes {
                lookup: self.env.scope(),
                visibility: scope,
            },
        ) {
            Ok(reso) => reso,
            Err(err) => {
                if let Some(diag) =
                    err.into_diag(self.db, *path, path_span, ExpectedPathKind::Record)
                {
                    self.push_diag(diag);
                }
                return ExprProp::invalid(self.db);
            }
        };

        match reso {
            PathRes::Ty(ty) | PathRes::TyAlias(_, ty, _) => {
                // Use the expected type to constrain the record's generic args
                // before checking fields. A structural layout hole unifies as
                // a wildcard, so a successful nominal match must also adopt
                // the expected application; that is how an assigned layout
                // view reaches aggregate construction.
                let snapshot = self.snapshot_state();
                let ty = if self.table.unify(ty, expected).is_ok() {
                    self.commit_state(snapshot);
                    self.canonical_nominal_ty_from_expected(ty, expected)
                } else {
                    self.rollback_state(snapshot);
                    ty.fold_with(self.db, &mut self.table)
                };

                let record_like = RecordLike::from_ty(ty);
                if record_like.is_record(self.db) {
                    self.env
                        .register_record_init_lowering(expr, RecordInitLowering::Struct);
                    self.check_record_init_fields(&record_like, expr);
                    ExprProp::new(ty, true)
                } else {
                    let diag =
                        BodyDiag::record_expected(self.db, span.path().into(), Some(record_like));
                    self.push_diag(diag);
                    ExprProp::invalid(self.db)
                }
            }

            PathRes::Func(ty)
            | PathRes::Const(_, ty)
            | PathRes::TraitConst(ty, ..)
            | PathRes::InherentConst(ty, ..) => {
                let record_like = RecordLike::from_ty(ty);
                let diag =
                    BodyDiag::record_expected(self.db, span.path().into(), Some(record_like));
                self.push_diag(diag);
                ExprProp::invalid(self.db)
            }
            PathRes::TraitMethod(..) | PathRes::Method(..) | PathRes::FuncParam(..) => {
                let diag = BodyDiag::record_expected(self.db, span.path().into(), None);
                self.push_diag(diag);
                ExprProp::invalid(self.db)
            }

            PathRes::EnumVariant(variant) => {
                // Constrain the variant type with the expected type before
                // checking fields (same rationale as record inits).
                let resolved_ty = variant.ty;
                let snapshot = self.snapshot_state();
                let ty = if self.table.unify(resolved_ty, expected).is_ok() {
                    self.commit_state(snapshot);
                    self.canonical_nominal_ty_from_expected(resolved_ty, expected)
                } else {
                    self.rollback_state(snapshot);
                    resolved_ty.fold_with(self.db, &mut self.table)
                };
                let variant = crate::analysis::name_resolution::ResolvedVariant { ty, ..variant };

                let record_like = RecordLike::from_variant(variant);
                if record_like.is_record(self.db) {
                    self.env.register_record_init_lowering(
                        expr,
                        RecordInitLowering::EnumVariant(variant),
                    );
                    self.check_record_init_fields(&record_like, expr);
                    ExprProp::new(ty, true)
                } else {
                    let diag = BodyDiag::record_expected(self.db, span.path().into(), None);
                    self.push_diag(diag);

                    ExprProp::invalid(self.db)
                }
            }
            PathRes::Mod(scope) => {
                let diag = BodyDiag::NotValue {
                    primary: span.into(),
                    given: Either::Left(scope.item()),
                };
                self.push_diag(diag);
                ExprProp::invalid(self.db)
            }
            PathRes::Trait(trait_) => {
                let diag = BodyDiag::NotValue {
                    primary: span.into(),
                    given: Either::Left(trait_.def(self.db).into()),
                };
                self.push_diag(diag);
                ExprProp::invalid(self.db)
            }
        }
    }

    fn check_record_init_fields(&mut self, record_like: &RecordLike<'db>, expr: ExprId) {
        let hir_db = self.db;

        let Partial::Present(Expr::RecordInit(_, fields)) = expr.data(hir_db, self.body()) else {
            unreachable!()
        };
        let (source_body, source) = self
            .body()
            .quote_authority(self.db, expr.into())
            .and_then(|authority| authority.source)
            .and_then(|(body, site)| match site {
                crate::hir_def::QuoteScopeSite::Expr(expr) => Some((body, expr)),
                _ => None,
            })
            .unwrap_or((self.body(), expr));
        let span = source.span(source_body).into_record_init_expr().fields();

        let authority = self
            .body()
            .quote_authority(self.db, expr.into())
            .cloned()
            .unwrap_or_else(|| self.env.scope().into());
        let mut rec_checker = RecordInitChecker::new(self, record_like, authority);

        for (i, field) in fields.iter().enumerate() {
            let label = field.label_eagerly(rec_checker.tc.db, rec_checker.tc.body());
            let field_span = span.clone().field(i).into();

            let expected = match rec_checker.feed_label(label, field_span) {
                Ok(ty) => ty,
                Err(diag) => {
                    rec_checker.tc.push_diag(diag);
                    TyId::invalid(rec_checker.tc.db, InvalidCause::Other)
                }
            };

            let prop = rec_checker.tc.check_expr(field.expr, expected);
            rec_checker
                .tc
                .record_implicit_move_for_owned_expr(field.expr, prop.ty);
        }

        if let Err(diag) = rec_checker.finalize(span.into(), false) {
            self.push_diag(diag);
        }
        let ty = match record_like {
            RecordLike::Type(ty) => *ty,
            RecordLike::EnumVariant(variant) => variant.ty,
        }
        .fold_with(self.db, &mut self.table);
        for (constraint_idx, &goal) in
            crate::analysis::ty::trait_resolution::constraint::ty_constraints(self.db, ty)
                .list(self.db)
                .iter()
                .enumerate()
        {
            let provision = self.env.provision_for_goal(goal);
            self.check_trait_const_obligation_at(
                goal,
                expr.span(self.body()).into(),
                provision,
                super::env::TraitObligationOrigin::ConstructorConstraint {
                    constructor: expr,
                    constraint_idx,
                },
            );
        }
    }

    fn check_field(&mut self, expr: ExprId, expr_data: &Expr<'db>) -> ExprProp<'db> {
        let Expr::Field(lhs, index) = expr_data else {
            unreachable!()
        };
        let Partial::Present(field) = index else {
            return ExprProp::invalid(self.db);
        };

        let lhs_ty = self.fresh_ty();
        let typed_lhs = self.check_expr(*lhs, lhs_ty);
        let lhs_ty = typed_lhs.ty;
        let lhs_place_ty = lhs_ty
            .as_capability(self.db)
            .map_or(lhs_ty, |(_, inner)| inner);
        // let lhs_ty = normalize_ty(self.db, lhs_ty, crate::analysis::semantic::SolverPremises::from_unpositioned_pair(self.env.scope(), self.env.assumptions()));

        if lhs_place_ty.has_invalid(self.db) {
            return ExprProp::invalid(self.db);
        }

        if lhs_place_ty.is_ty_var(self.db) {
            let diag = BodyDiag::TypeMustBeKnown(lhs.span(self.body()).into());
            self.push_diag(diag);
            return ExprProp::invalid(self.db);
        }

        if let Some(resolved_field) = resolve_place_field(self.db, lhs_place_ty, *field) {
            let (ty_base, ty_args) = resolved_field.base_ty.decompose_ty_app(self.db);
            let is_mut = typed_lhs.is_mut || resolved_field.implicit_deref_ty.is_some();

            match field {
                FieldIndex::Ident(label) => {
                    let record_like = RecordLike::from_ty(resolved_field.base_ty);
                    if let Some(field_ty) = record_like.record_field_ty(self.db, *label) {
                        if let Some(scope) = record_like.record_field_scope(self.db, *label) {
                            let issued = self
                                .body()
                                .quote_authority(self.db, expr.into())
                                .and_then(|authority| authority.field(self.db, *label));
                            if let Some(expected) = issued
                                && expected.field != scope
                            {
                                self.push_diag(BodyDiag::QuoteFieldOwnerMismatch {
                                    primary: expected
                                        .source
                                        .map(|(body, expr)| expr.span(body).into())
                                        .unwrap_or_else(|| expr.span(self.body()).into()),
                                    expected: expected.field,
                                    actual: scope,
                                    generated: self.body().expr(self.db).span(self.body()).into(),
                                });
                                return ExprProp::invalid(self.db);
                            }
                            if !issued.is_some_and(|grant| grant.field == scope)
                                && !is_scope_visible_from(
                                    self.db,
                                    scope,
                                    self.body()
                                        .quote_scope(self.db, expr)
                                        .unwrap_or(self.env.scope()),
                                )
                            {
                                if let Some(authority) =
                                    self.body().quote_authority(self.db, expr.into())
                                {
                                    let (source_body, source) = authority
                                        .source
                                        .and_then(|(body, site)| match site {
                                            crate::hir_def::QuoteScopeSite::Expr(expr) => {
                                                Some((body, expr))
                                            }
                                            _ => None,
                                        })
                                        .unwrap_or((self.body(), expr));
                                    self.push_diag(BodyDiag::QuoteFieldVisibility {
                                        primary: source
                                            .span(source_body)
                                            .into_field_expr()
                                            .accessor()
                                            .into(),
                                        field: scope,
                                        generated: self
                                            .body()
                                            .expr(self.db)
                                            .span(self.body())
                                            .into(),
                                    });
                                } else {
                                    self.push_diag(PathResDiag::Invisible(
                                        expr.span(self.body()).into_field_expr().accessor().into(),
                                        *label,
                                        scope.name_span(self.db),
                                    ));
                                }
                                return ExprProp::invalid(self.db);
                            }
                        }
                        self.env
                            .register_resolved_field_index(expr, resolved_field.index);
                        if let Some(projected) =
                            self.contract_field_projected_field_ty(*lhs, resolved_field.index)
                        {
                            return ExprProp::new(self.table.fold_ty(self.db, projected), is_mut);
                        }
                        if let Some(projected) =
                            self.callable_input_projected_field_ty(*lhs, resolved_field.index)
                        {
                            return ExprProp::new(self.table.fold_ty(self.db, projected), is_mut);
                        }
                        let context = self.env.selection_context();
                        let field_ty = if !context.selections(self.db).is_empty()
                            || !context.witnesses(self.db).is_empty()
                        {
                            let source = instantiate_adt_field_source_for_concrete_demand(
                                self.db,
                                resolved_field
                                    .base_ty
                                    .adt_def(self.db)
                                    .expect("record field ADT"),
                                0,
                                usize::from(resolved_field.index),
                                resolved_field.base_ty.generic_args(self.db),
                            );
                            let source = source.fold_with(self.db, &mut self.table);
                            let source =
                                context.contextualize_const_types_with(self.db, source, |goal| {
                                    self.env.provision_for_goal(goal)
                                });
                            self.normalize_ty(source)
                        } else {
                            field_ty
                        };
                        return ExprProp::new(field_ty, is_mut);
                    }
                }

                FieldIndex::Index(_) => {
                    if ty_base.is_tuple(self.db) {
                        self.env
                            .register_resolved_field_index(expr, resolved_field.index);
                        if let Some(projected) =
                            self.contract_field_projected_field_ty(*lhs, resolved_field.index)
                        {
                            return ExprProp::new(self.table.fold_ty(self.db, projected), is_mut);
                        }
                        if let Some(projected) =
                            self.callable_input_projected_field_ty(*lhs, resolved_field.index)
                        {
                            return ExprProp::new(self.table.fold_ty(self.db, projected), is_mut);
                        }
                        let ty = ty_args[usize::from(resolved_field.index)];
                        return ExprProp::new(ty, is_mut);
                    }
                }
                // A spliced field never resolves where its template is written.
                FieldIndex::Hole(_) => {}
            }
        }

        let diag = BodyDiag::AccessedFieldNotFound {
            primary: expr.span(self.body()).into(),
            given_ty: lhs_place_ty,
            index: *field,
        };
        self.push_diag(diag);

        ExprProp::invalid(self.db)
    }

    fn contract_field_projected_field_ty(
        &self,
        lhs: ExprId,
        field_index: u16,
    ) -> Option<TyId<'db>> {
        let (field, view, mut projections) = self.contract_field_layout_context(lhs)?;
        projections.push(LayoutProjection::Field(field_index));
        let selection = field
            .selection_for_projections(self.db, view, &projections)
            .ok()?;
        self.selected_contract_layout_ty(field, view, &selection)
    }

    fn contract_field_projected_index_ty(&self, lhs: ExprId) -> Option<TyId<'db>> {
        let (field, view, mut projections) = self.contract_field_layout_context(lhs)?;
        projections.push(LayoutProjection::Index);
        let selection = field
            .selection_for_projections(self.db, view, &projections)
            .ok()?;
        self.selected_contract_layout_ty(field, view, &selection)
    }

    fn selected_contract_layout_ty(
        &self,
        field: &FieldStorageLayout<'db>,
        view: LayoutViewKind,
        selection: &crate::semantic::LayoutSelection,
    ) -> Option<TyId<'db>> {
        match field.projected_concrete_ty(self.db, view, selection) {
            Ok(ty) => Some(ty),
            Err(
                LayoutViewError::NonPhysicalRoot { .. } | LayoutViewError::RootNeedsLanding { .. },
            ) => field
                .project(self.db, view, selection)
                .ok()
                .map(|view| view.shape_ty()),
            Err(
                LayoutViewError::RootNotClassified { .. }
                | LayoutViewError::MissingAllocation { .. }
                | LayoutViewError::InvalidProjection,
            ) => None,
        }
    }

    fn contract_field_layout_context(
        &self,
        expr: ExprId,
    ) -> Option<(
        &'db FieldStorageLayout<'db>,
        LayoutViewKind,
        Vec<LayoutProjection>,
    )> {
        let place = self.env.expr_place(expr)?;
        self.contract_field_layout_context_for_place(&place)
    }

    fn contract_field_layout_context_for_place(
        &self,
        place: &Place<'db>,
    ) -> Option<(
        &'db FieldStorageLayout<'db>,
        LayoutViewKind,
        Vec<LayoutProjection>,
    )> {
        let PlaceBase::Binding(binding) = place.base;
        let provider = match binding {
            LocalBinding::EffectParam {
                site, provider_idx, ..
            } => self.env.provider_binding(site, provider_idx),
            LocalBinding::Param {
                site: ParamSite::EffectField(site),
                idx,
                ..
            } => self.env.resolved_provider_binding(site, idx),
            LocalBinding::Local { .. } | LocalBinding::Param { .. } => None,
        }?;
        let layout_env = provider.layout_env?;
        let field = layout_env
            .field
            .contract
            .storage_layout(self.db)
            .values()
            .find(|field| field.field == layout_env.field)?;
        let projections = place
            .projections
            .iter()
            .filter_map(|projection| match projection {
                PlaceProjection::Deref { .. } => None,
                PlaceProjection::Field { index, .. } => Some(LayoutProjection::Field(*index)),
                PlaceProjection::Index { .. } => Some(LayoutProjection::Index),
            })
            .collect::<Vec<_>>();
        Some((field, layout_env.view, projections))
    }

    fn contract_field_layout_context_for_effect_arg(
        &self,
        arg: &super::EffectArg<'db>,
    ) -> Option<(
        &'db FieldStorageLayout<'db>,
        LayoutViewKind,
        Vec<LayoutProjection>,
    )> {
        match arg {
            super::EffectArg::Place(place) => self.contract_field_layout_context_for_place(place),
            super::EffectArg::Binding(binding) => self
                .contract_field_layout_context_for_place(&Place::new(PlaceBase::Binding(*binding))),
            super::EffectArg::Value(expr) => self.contract_field_layout_context(*expr),
            super::EffectArg::RootProvider { .. } | super::EffectArg::Unknown => None,
        }
    }

    pub(super) fn specialize_callable_layout_args(
        &self,
        callable: &mut Callable<'db>,
        receiver: Option<ExprId>,
        args: &[HirCallArg<'db>],
    ) {
        let CallableDef::Func(func) = callable.callable_def() else {
            return;
        };
        if let Some(receiver) = receiver {
            self.specialize_callable_layout_origin(
                callable,
                func,
                receiver,
                crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::Receiver,
            );
        }
        let param_offset = usize::from(receiver.is_some());
        for (idx, arg) in args.iter().enumerate() {
            self.specialize_callable_layout_origin(
                callable,
                func,
                arg.expr,
                crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::ValueParam(
                    idx + param_offset,
                ),
            );
        }
    }

    fn specialize_callable_layout_origin(
        &self,
        callable: &mut Callable<'db>,
        func: crate::hir_def::Func<'db>,
        expr: ExprId,
        origin: crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin,
    ) {
        let Some((field, view, base_projections)) = self.contract_field_layout_context(expr) else {
            return;
        };
        self.specialize_callable_layout_context(
            callable,
            func,
            origin,
            field,
            view,
            &base_projections,
        );
    }

    fn specialize_callable_layout_context(
        &self,
        callable: &mut Callable<'db>,
        func: crate::hir_def::Func<'db>,
        origin: crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin,
        field: &FieldStorageLayout<'db>,
        view: LayoutViewKind,
        base_projections: &[LayoutProjection],
    ) {
        if let Ok(selection) = field.selection_for_projections(self.db, view, base_projections)
            && let Some(actual_ty) = self.selected_contract_layout_ty(field, view, &selection)
        {
            match origin {
                crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::Receiver => {
                    callable.specialize_arg_from_actual(self.db, 0, actual_ty);
                }
                crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::ValueParam(idx) => {
                    callable.specialize_arg_from_actual(self.db, idx, actual_ty);
                }
                crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::Effect(_) => {
                    if let Some(expected_ty) =
                        callable_input_layout_origin_ty(self.db, func, origin)
                    {
                        callable.specialize_params_from_actual(self.db, expected_ty, actual_ty);
                    }
                }
            }
        }
        for path in callable_input_layout_projection_paths(self.db, func, origin) {
            let Some(path_projections) = layout_projections_from_callable_path(&path) else {
                continue;
            };
            let mut projections = base_projections.to_vec();
            projections.extend(path_projections);
            let Ok(selection) = field.selection_for_projections(self.db, view, &projections) else {
                continue;
            };
            let Some(actual_ty) = self.selected_contract_layout_ty(field, view, &selection) else {
                continue;
            };
            instantiate_callable_projection_layout_args(
                self.db,
                func,
                origin,
                &path,
                actual_ty,
                callable.generic_args_mut(),
            );
        }
    }

    fn specialize_callable_effect_layout_projections(
        &self,
        callable: &mut Callable<'db>,
        callee: crate::hir_def::Func<'db>,
        effect_idx: usize,
        provider: ProvidedEffect<'db>,
        arg: &super::EffectArg<'db>,
    ) {
        let callee_origin =
            crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::Effect(effect_idx);
        if let Some((field, view, base_projections)) =
            self.contract_field_layout_context_for_effect_arg(arg)
        {
            self.specialize_callable_layout_context(
                callable,
                callee,
                callee_origin,
                field,
                view,
                &base_projections,
            );
            return;
        }
        let Some(LocalBinding::EffectParam {
            site: EffectParamSite::Func(caller),
            idx: caller_effect_idx,
            ..
        }) = provider.binding
        else {
            return;
        };
        let caller_origin =
            crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::Effect(caller_effect_idx);
        for path in callable_input_layout_projection_paths(self.db, callee, callee_origin) {
            let Some(actual_ty) =
                callable_input_projected_layout_ty(self.db, caller, caller_origin, &path)
            else {
                continue;
            };
            instantiate_callable_projection_layout_args(
                self.db,
                callee,
                callee_origin,
                &path,
                actual_ty,
                callable.generic_args_mut(),
            );
        }
    }

    pub(super) fn pattern_layout_context(&self, expr: ExprId) -> Option<PatternLayoutContext<'db>> {
        self.pattern_layout_context_for_projection(expr, &[])
    }

    pub(super) fn pattern_layout_context_for_projection(
        &self,
        expr: ExprId,
        projection: &[LayoutBundlePathStep],
    ) -> Option<PatternLayoutContext<'db>> {
        if let Some((field, view, mut base_projections)) = self.contract_field_layout_context(expr)
        {
            base_projections.extend(layout_projections_from_callable_path(projection)?);
            return Some(PatternLayoutContext::ContractField {
                field,
                view,
                base_projections,
            });
        }
        let place = self.env.expr_place(expr)?;
        let (func, origin) = match place.base {
            PlaceBase::Binding(LocalBinding::Param {
                site: ParamSite::Func(func),
                idx,
                ..
            }) => {
                let param = func.params(self.db).nth(idx)?;
                let origin = if param.is_self_param(self.db) {
                    crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::Receiver
                } else {
                    crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::ValueParam(idx)
                };
                (func, origin)
            }
            PlaceBase::Binding(LocalBinding::EffectParam {
                site: EffectParamSite::Func(func),
                idx,
                ..
            })
            | PlaceBase::Binding(LocalBinding::Param {
                site: ParamSite::EffectField(EffectParamSite::Func(func)),
                idx,
                ..
            }) => (
                func,
                crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::Effect(idx),
            ),
            PlaceBase::Binding(
                LocalBinding::Local { .. }
                | LocalBinding::Param { .. }
                | LocalBinding::EffectParam { .. },
            ) => return None,
        };
        let mut base_path = place
            .projections
            .iter()
            .filter_map(|projection| match projection {
                PlaceProjection::Deref { .. } => None,
                PlaceProjection::Field { index, .. } => Some(LayoutBundlePathStep::Field(*index)),
                PlaceProjection::Index { .. } => Some(LayoutBundlePathStep::Index),
            })
            .collect::<Vec<_>>();
        base_path.extend_from_slice(projection);
        Some(PatternLayoutContext::CallableInput {
            func,
            origin,
            base_path,
        })
    }

    pub(super) fn projected_pattern_layout_ty(
        &self,
        context: &PatternLayoutContext<'db>,
        path: &[LayoutBundlePathStep],
    ) -> Option<TyId<'db>> {
        match context {
            PatternLayoutContext::ContractField {
                field,
                view,
                base_projections,
            } => {
                let mut projections = base_projections.clone();
                projections.extend(layout_projections_from_callable_path(path)?);
                let selection = field
                    .selection_for_projections(self.db, *view, &projections)
                    .ok()?;
                self.selected_contract_layout_ty(field, *view, &selection)
            }
            PatternLayoutContext::CallableInput {
                func,
                origin,
                base_path,
            } => {
                let mut projection = base_path.clone();
                projection.extend_from_slice(path);
                callable_input_projected_layout_ty(self.db, *func, *origin, &projection)
            }
        }
    }

    fn callable_input_projected_field_ty(
        &self,
        lhs: ExprId,
        field_index: u16,
    ) -> Option<TyId<'db>> {
        let place = self.env.expr_place(lhs)?;
        let PlaceBase::Binding(LocalBinding::Param {
            site: ParamSite::Func(func),
            idx,
            ..
        }) = place.base
        else {
            return None;
        };
        let param = func.params(self.db).nth(idx)?;
        let origin = if param.is_self_param(self.db) {
            crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::Receiver
        } else {
            crate::analysis::ty::const_ty::CallableInputLayoutHoleOrigin::ValueParam(idx)
        };
        let mut path = place
            .projections
            .iter()
            .filter_map(|projection| match projection {
                PlaceProjection::Deref { .. } => None,
                PlaceProjection::Field { index, .. } => Some(LayoutBundlePathStep::Field(*index)),
                PlaceProjection::Index { .. } => Some(LayoutBundlePathStep::Index),
            })
            .collect::<Vec<_>>();
        path.push(LayoutBundlePathStep::Field(field_index));
        callable_input_carrier_projected_layout_ty(self.db, func, origin, &path)
    }

    fn check_tuple(
        &mut self,
        _expr: ExprId,
        expr_data: &Expr<'db>,
        expected: TyId<'db>,
    ) -> ExprProp<'db> {
        let Expr::Tuple(elems) = expr_data else {
            unreachable!()
        };

        let elem_tys = match expected.decompose_ty_app(self.db) {
            (base, args) if base.is_tuple(self.db) && args.len() == elems.len() => args.to_vec(),
            _ => self.fresh_tys_n(elems.len()),
        };

        for (elem, elem_ty) in elems.iter().zip(elem_tys.iter()) {
            let prop = self.check_expr(*elem, *elem_ty);
            self.record_implicit_move_for_owned_expr(*elem, prop.ty);
        }

        let ty = TyId::tuple_with_elems(self.db, &elem_tys);
        ExprProp::new(ty, true)
    }

    fn check_array(
        &mut self,
        _expr: ExprId,
        expr_data: &Expr<'db>,
        expected: TyId<'db>,
    ) -> ExprProp<'db> {
        let Expr::Array(elems) = expr_data else {
            unreachable!()
        };

        let mut expected_elem_ty = match expected.decompose_ty_app(self.db) {
            (base, args) if base.is_array(self.db) => args[0],
            _ => self.fresh_ty(),
        };

        for elem in elems {
            let prop = self.check_expr(*elem, expected_elem_ty);
            expected_elem_ty = prop.ty;
            self.record_implicit_move_for_owned_expr(*elem, expected_elem_ty);
        }

        let ty = TyId::array_with_len(self.db, expected_elem_ty, elems.len());
        ExprProp::new(ty, true)
    }

    fn check_array_rep(
        &mut self,
        _expr: ExprId,
        expr_data: &Expr<'db>,
        expected: TyId<'db>,
    ) -> ExprProp<'db> {
        let Expr::ArrayRep(elem, len) = expr_data else {
            unreachable!()
        };

        let mut expected_elem_ty = match expected.decompose_ty_app(self.db) {
            (base, args) if base.is_array(self.db) => args[0],
            _ => self.fresh_ty(),
        };

        let prop = self.check_expr(*elem, expected_elem_ty);
        expected_elem_ty = prop.ty;
        if !expected_elem_ty.has_invalid(self.db) && !self.ty_is_copy(expected_elem_ty) {
            self.push_diag(BodyDiag::ArrayRepeatRequiresCopy {
                primary: elem.span(self.body()).into(),
                ty: expected_elem_ty,
            });
        }

        let array = TyId::array(self.db, expected_elem_ty);
        let ty = if let Some(len_body) = len.to_opt() {
            let expected_len_ty = array
                .applicable_ty(self.db)
                .and_then(|applicable| applicable.const_ty);

            let len_ty = ConstTyId::from_body(self.db, len_body, expected_len_ty, None)
                .with_provision_at(self.db, self.env.selection_context(), &|goal| {
                    self.env.provision_for_goal(goal)
                });
            let len_ty = TyId::const_ty(self.db, len_ty);
            let array_ty = TyId::app(self.db, array, len_ty);

            if let Some(diag) = array_ty.emit_diag(self.db, len_body.span().into()) {
                self.push_diag(diag);
            }

            let mut requires_known_const = false;
            if !array_ty.has_invalid(self.db)
                && let (_, args) = array_ty.decompose_ty_app(self.db)
                && let Some(len_ty) = args.get(1)
                && let TyData::ConstTy(const_ty) = len_ty.data(self.db)
                && !self.array_len_const_is_acceptable(*const_ty)
            {
                requires_known_const = true;
                self.push_diag(BodyDiag::ConstValueMustBeKnown(len_body.span().into()));
            }

            if requires_known_const && expected.base_ty(self.db).is_array(self.db) {
                expected
            } else {
                array_ty
            }
        } else {
            let len_ty = ConstTyId::invalid(self.db, InvalidCause::ParseError);
            let len_ty = TyId::const_ty(self.db, len_ty);
            TyId::app(self.db, array, len_ty)
        };

        ExprProp::new(ty, true)
    }

    /// Array counts can retain captured scalar arithmetic until their compiler-
    /// known provision is instantiated. CTFE still checks operations and budgets.
    fn array_len_const_is_acceptable(&self, const_ty: ConstTyId<'db>) -> bool {
        let mut pending = vec![const_ty];
        let mut arithmetic = false;
        let mut opened = false;
        while let Some(value) = pending.pop() {
            if value.integer_value(self.db).is_some() {
                continue;
            }
            match value.data(self.db) {
                ConstTyData::TyParam(..) => {}
                ConstTyData::Computation { description, .. } => match description.repr() {
                    ConstRepr::Term(term) => pending.push(*term),
                    _ => return false,
                },
                ConstTyData::Abstract(expr, _) => {
                    let operands: &[TyId<'db>] = match expr.data(self.db) {
                        ConstExpr::TraitConst(use_) => {
                            opened |= use_.provision().is_some();
                            continue;
                        }
                        ConstExpr::ArithBinOp { lhs, rhs, .. } => &[*lhs, *rhs],
                        ConstExpr::UnOp { expr, .. } | ConstExpr::Cast { expr, .. } => &[*expr],
                        _ => return false,
                    };
                    arithmetic = true;
                    for operand in operands {
                        let TyData::ConstTy(value) = operand.data(self.db) else {
                            return false;
                        };
                        pending.push(*value);
                    }
                }
                _ => return false,
            }
        }
        !arithmetic || opened
    }

    fn check_evidence_call_source(
        &mut self,
        expr: ExprId,
        callable: &super::Callable<'db>,
        result: TyId<'db>,
        args: &[HirCallArg<'db>],
        receiver: Option<ExprId>,
    ) {
        if !self.type_contains_evidence(result) {
            return;
        }
        let failure = if !matches!(callable.callable_def(), CallableDef::Func(func) if func.is_const(self.db))
        {
            Some(expr)
        } else {
            args.iter()
                .find_map(|arg| self.compile_time_source_failure(arg.expr, true))
                .or_else(|| {
                    receiver.and_then(|receiver| self.compile_time_source_failure(receiver, true))
                })
        };
        if let Some(failure) = failure {
            self.report_evidence_source_failure(failure);
        }
    }

    /// Recognize compile-time source forms without executing or propagating values.
    pub(super) fn compile_time_source_failure(
        &self,
        root: ExprId,
        allow_method: bool,
    ) -> Option<ExprId> {
        let mut pending = vec![root];
        let mut seen = FxHashSet::default();
        while let Some(expr) = pending.pop() {
            if !seen.insert(expr) {
                continue;
            }
            let Partial::Present(data) = expr.data(self.db, self.body()) else {
                return Some(expr);
            };
            match data {
                Expr::Lit(_) => {}
                Expr::Un(inner, _) | Expr::Cast(inner, _) => pending.push(*inner),
                Expr::Bin(lhs, rhs, _) => pending.extend([*lhs, *rhs]),
                Expr::Tuple(items) | Expr::Array(items) => pending.extend(items.iter().copied()),
                Expr::RecordInit(_, fields) => {
                    pending.extend(fields.iter().map(|field| field.expr))
                }
                Expr::With(_, body) => pending.push(*body),
                Expr::Block(stmts) => {
                    let Some(stmt) = stmts.last() else { continue };
                    let Partial::Present(Stmt::Expr(tail)) = stmt.data(self.db, self.body()) else {
                        return Some(expr);
                    };
                    pending.push(*tail);
                }
                Expr::Path(_) => {
                    if self.env.expr_const_ref(expr).is_some()
                        || matches!(
                            self.env.expr_value_path_ref(expr),
                            Some(ValuePathRef::TypeConst(_) | ValuePathRef::UnitVariant(_))
                        )
                    {
                        continue;
                    }
                    let Some(LocalBinding::Local { pat, is_mut: false }) =
                        self.env.typed_expr(expr).and_then(|p| p.binding)
                    else {
                        return Some(expr);
                    };
                    let initializer =
                        self.body()
                            .stmts(self.db)
                            .iter()
                            .find_map(|(_, stmt)| match stmt {
                                Partial::Present(Stmt::Let(bound, _, Some(init)))
                                    if *bound == pat =>
                                {
                                    Some(*init)
                                }
                                _ => None,
                            });
                    let Some(initializer) = initializer else {
                        return Some(expr);
                    };
                    pending.push(initializer);
                }
                Expr::If(cond, then, otherwise) => {
                    if let Some(failure) = self.compile_time_condition_failure(*cond) {
                        return Some(failure);
                    }
                    pending.push(*then);
                    pending.extend(otherwise.iter().copied());
                }
                Expr::Call(_, args) | Expr::MethodCall(_, _, _, args) => {
                    if let Expr::MethodCall(receiver, ..) = data {
                        if !allow_method {
                            return Some(expr);
                        }
                        pending.push(*receiver);
                    }
                    let Some(callable) = self.env.callable_expr(expr) else {
                        return Some(expr);
                    };
                    let CallableDef::Func(func) = callable.callable_def() else {
                        return Some(expr);
                    };
                    if !func.is_const(self.db) {
                        return Some(expr);
                    }
                    pending.extend(args.iter().map(|arg| arg.expr));
                }
                _ => return Some(expr),
            }
        }
        None
    }

    pub(super) fn compile_time_condition_failure(&self, root: CondId) -> Option<ExprId> {
        let mut pending = vec![root];
        while let Some(cond) = pending.pop() {
            match cond.data(self.db, self.body()) {
                Partial::Present(Cond::Expr(expr)) => {
                    if let Some(failure) = self.compile_time_source_failure(*expr, false) {
                        return Some(failure);
                    }
                }
                Partial::Present(Cond::Bin(lhs, rhs, _)) => pending.extend([*lhs, *rhs]),
                Partial::Present(Cond::Let(_, value)) => return Some(*value),
                _ => return Some(self.body().expr(self.db)),
            }
        }
        None
    }

    pub(super) fn report_evidence_source_failure(&mut self, expr: ExprId) {
        let reason = match expr.data(self.db, self.body()) {
            Partial::Present(Expr::Path(Partial::Present(path))) => {
                let name = path.pretty_print(self.db);
                match self.env.typed_expr(expr).and_then(|prop| prop.binding) {
                    Some(LocalBinding::Param { .. } | LocalBinding::EffectParam { .. }) => {
                        format!("`{name}` is a parameter, not a compile-time source")
                    }
                    Some(LocalBinding::Local { is_mut: true, .. }) => {
                        format!("`{name}` is a mutable place")
                    }
                    _ => format!("`{name}` is a runtime value"),
                }
            }
            Partial::Present(Expr::Field(..) | Expr::MethodCall(..)) => {
                "this selector cannot choose evidence at compile time".to_string()
            }
            Partial::Present(Expr::Call(..)) => {
                "this call is not a compile-time evidence source".to_string()
            }
            _ => "this expression is not a compile-time evidence source".to_string(),
        };
        self.push_diag(BodyDiag::EvidenceChoiceNotCompileTime {
            primary: expr.span(self.body()).into(),
            reason: Some(reason),
        });
    }

    fn check_if(
        &mut self,
        _expr: ExprId,
        expr_data: &Expr<'db>,
        expected: TyId<'db>,
        result_discarded: bool,
    ) -> ExprProp<'db> {
        let Expr::If(cond, then, else_) = expr_data else {
            unreachable!()
        };

        // Keep let-chain bindings scoped to this conditional so they can flow
        // into the then branch (and chained `&&` segments) without leaking to
        // the enclosing scope.
        self.env.enter_lexical_scope();
        self.check_cond(*cond);

        if let Some(else_) = else_ {
            self.env.enter_scope(*then);
            self.env.flush_pending_bindings();
            let then_prop = if result_discarded {
                self.check_expr_with_discarded_result(*then, expected)
            } else {
                self.check_expr(*then, expected)
            };
            self.env.leave_scope();
            self.env.clear_pending_bindings();
            self.env.leave_scope();
            let else_prop = self.check_expr_in_new_scope(*else_, expected, result_discarded);
            if self.type_contains_evidence(else_prop.ty)
                && let Some(failure) = self.compile_time_condition_failure(*cond)
            {
                self.report_evidence_source_failure(failure);
            }
            let borrow_provider = self.merge_concrete_borrow_providers(
                then.span(self.body()).into(),
                then_prop.borrow_provider,
                else_.span(self.body()).into(),
                else_prop.borrow_provider,
            );
            ExprProp {
                ty: else_prop.ty,
                is_mut: true,
                binding: None,
                borrow_provider,
                path_read_semantics: None,
            }
        } else {
            let if_ty = self.fresh_ty();
            // If there is no else branch, the if expression itself typed as `()`
            self.env.enter_scope(*then);
            self.env.flush_pending_bindings();
            self.check_expr_with_discarded_result(*then, if_ty);
            self.env.leave_scope();
            self.env.clear_pending_bindings();
            self.env.leave_scope();
            ExprProp::new(TyId::unit(self.db), true)
        }
    }

    fn check_match(
        &mut self,
        expr: ExprId,
        expr_data: &Expr<'db>,
        expected: TyId<'db>,
        result_discarded: bool,
    ) -> ExprProp<'db> {
        let Expr::Match(scrutinee, arms) = expr_data else {
            unreachable!()
        };

        let scrutinee_ty = self.fresh_ty();
        let scrutinee_ty = self.check_expr(*scrutinee, scrutinee_ty).ty;
        let (scrutinee_pat_ty, mode) = self.destructure_source_mode(scrutinee_ty);
        let pattern_layout = self.pattern_layout_context(*scrutinee);

        let Partial::Present(arms) = arms else {
            return ExprProp::invalid(self.db);
        };

        if arms.iter().any(|arm| arm.parts().is_none()) {
            self.push_diag(BodyDiag::UnexpandedSplice {
                primary: expr.span(self.body()).into(),
                kind: crate::hir_def::QuoteKind::Arm,
            });
            return ExprProp::invalid(self.db);
        }

        let mut match_ty = expected;
        let mut first_provider: Option<(DynLazySpan<'db>, super::ProviderAddressSpace)> = None;
        let mut provider_unknown = false;
        let mut provider_conflict = false;
        let mut arm_statuses = Vec::with_capacity(arms.len());

        for arm in arms.iter() {
            let pat_result =
                self.check_pat_with_layout(arm.pat(), scrutinee_pat_ty, pattern_layout.as_ref());
            if let super::PatternDestructureMode::Borrow(kind) = mode {
                self.retype_pattern_bindings_for_borrow(arm.pat(), kind);
            }
            arm_statuses.push(pat_result.analysis);

            self.env.enter_scope(arm.body());
            self.env.flush_pending_bindings();
            let arm_prop = if result_discarded {
                self.check_expr_with_discarded_result(arm.body(), match_ty)
            } else {
                self.check_expr(arm.body(), match_ty)
            };
            match_ty = arm_prop.ty;
            self.env.leave_scope();

            if arm_prop.ty.as_capability(self.db).is_some() {
                if let Some(provider) = arm_prop.borrow_provider {
                    if let Some((ref span, previous)) = first_provider {
                        provider_conflict |= self
                            .merge_concrete_borrow_providers(
                                span.clone(),
                                Some(previous),
                                arm.body().span(self.body()).into(),
                                Some(provider),
                            )
                            .is_none();
                    } else {
                        first_provider = Some((arm.body().span(self.body()).into(), provider));
                    }
                } else {
                    provider_unknown = true;
                }
            }
        }

        if self.type_contains_evidence(match_ty)
            && let Some(failure) = self.compile_time_source_failure(*scrutinee, false)
        {
            self.report_evidence_source_failure(failure);
        }

        if !scrutinee_pat_ty.has_invalid(self.db)
            && arm_statuses.iter().all(|status| status.is_ready())
        {
            let mut prober = super::env::Prober::new(&mut self.table, self.env.scope());
            let pattern_store = self
                .env
                .pattern_store()
                .clone()
                .fold_with(self.db, &mut prober);
            let scrutinee_pat_ty = scrutinee_pat_ty.fold_with(self.db, &mut prober);
            let roots: Vec<_> = arm_statuses
                .iter()
                .filter_map(|status| status.ready_root())
                .collect();
            let analysis = crate::analysis::ty::pattern_analysis::analyze_match(
                self.db,
                &pattern_store,
                &roots,
                scrutinee_pat_ty,
            );

            for i in analysis.unreachable_arms {
                let diag = BodyDiag::UnreachablePattern {
                    primary: arms[i].pat().span(self.body()).into(),
                };
                self.push_diag(diag);
            }

            match analysis.exhaustiveness {
                crate::analysis::ty::pattern_analysis::MatchExhaustiveness::Exhaustive => {}
                crate::analysis::ty::pattern_analysis::MatchExhaustiveness::NonExhaustive(
                    missing_patterns,
                ) => {
                    let diag = BodyDiag::NonExhaustiveMatch {
                        primary: expr.span(self.body()).into(),
                        scrutinee_ty: scrutinee_pat_ty,
                        missing_patterns,
                    };
                    self.push_diag(diag);
                }
                crate::analysis::ty::pattern_analysis::MatchExhaustiveness::Inconclusive(
                    reason,
                ) => {
                    let diag = BodyDiag::PatternAnalysisInconclusive {
                        primary: expr.span(self.body()).into(),
                        reason,
                    };
                    self.push_diag(diag);
                }
            }
        }

        ExprProp {
            ty: match_ty,
            is_mut: true,
            binding: None,
            borrow_provider: if provider_unknown || provider_conflict {
                None
            } else {
                first_provider.map(|(_, provider)| provider)
            },
            path_read_semantics: None,
        }
    }

    fn assignment_target_ty(&self, lhs: ExprId, ty: TyId<'db>) -> TyId<'db> {
        // An explicit raw-pointer dereference selects the stored pointee, even
        // when that value is a capability. Ordinary capability places instead
        // assign through their referent.
        if self.is_pointer_deref_expr(lhs) {
            ty
        } else {
            ty.as_capability(self.db).map_or(ty, |(_, target)| target)
        }
    }

    fn check_assign(&mut self, _expr: ExprId, expr_data: &Expr<'db>) -> ExprProp<'db> {
        let Expr::Assign(lhs, rhs) = expr_data else {
            unreachable!()
        };

        if let Some(unit) = self.check_index_mut_assign(*lhs, *rhs) {
            return unit;
        }

        let typed_lhs = self.check_expr_unknown(*lhs);
        let lhs_ty = self.assignment_target_ty(*lhs, typed_lhs.ty);
        // Assignment is an expected-type boundary. In particular, an assigned
        // contract-field view can carry concrete layout roots that must reach
        // aggregate constructors before their runtime layout is selected.
        let mut rhs_prop = self.check_expr(*rhs, lhs_ty);
        if let Some(coerced) =
            self.try_coerce_capability_for_expr_to_expected(*rhs, rhs_prop.ty, lhs_ty)
        {
            rhs_prop.ty = coerced;
        }
        rhs_prop.ty = self.unify_ty(Typeable::Expr(*rhs, rhs_prop.clone()), rhs_prop.ty, lhs_ty);

        if let Some(place) = self.env.expr_place(*lhs) {
            let PlaceBase::Binding(binding) = place.base;
            for boundary in self.env.selection_boundaries(Some(binding)) {
                if self.reject_selection_type_escape(rhs_prop.ty, boundary) {
                    return ExprProp::invalid(self.db);
                }
            }
        }

        let lhs_status = self.check_assign_lhs(*lhs, &typed_lhs);
        self.record_implicit_move_for_owned_expr(*rhs, rhs_prop.ty);

        if lhs_status == AssignLhsStatus::Assignable
            && typed_lhs.ty.as_capability(self.db).is_some()
            && let Some(place) = self.env.expr_place(*lhs)
            && place.projections.is_empty()
        {
            let PlaceBase::Binding(binding) = place.base;
            self.merge_concrete_borrow_providers(
                binding.def_span(&self.env),
                self.concrete_borrow_provider_for_binding(binding),
                rhs.span(self.body()).into(),
                rhs_prop.borrow_provider,
            );
        }

        ExprProp::new(TyId::unit(self.db), true)
    }

    fn check_index_mut_assign(&mut self, lhs: ExprId, rhs: ExprId) -> Option<ExprProp<'db>> {
        let target = self.check_index_mut_target(lhs)?;
        if target.prop.ty.has_invalid(self.db) {
            return Some(ExprProp::new(TyId::unit(self.db), true));
        }

        let mut rhs_prop = self.check_expr(rhs, target.target_ty);
        if let Some(coerced) =
            self.try_coerce_capability_for_expr_to_expected(rhs, rhs_prop.ty, target.target_ty)
        {
            rhs_prop.ty = coerced;
        }
        rhs_prop.ty = self.unify_ty(
            Typeable::Expr(rhs, rhs_prop.clone()),
            rhs_prop.ty,
            target.target_ty,
        );

        if !target.trait_lowered {
            let lhs_status = self.check_assign_lhs(lhs, &target.prop);
            if lhs_status == AssignLhsStatus::Assignable
                && target.prop.ty.as_capability(self.db).is_some()
                && let Some(place) = self.env.expr_place(lhs)
                && place.projections.is_empty()
            {
                let PlaceBase::Binding(binding) = place.base;
                self.merge_concrete_borrow_providers(
                    binding.def_span(&self.env),
                    self.concrete_borrow_provider_for_binding(binding),
                    rhs.span(self.body()).into(),
                    rhs_prop.borrow_provider,
                );
            }
        }

        self.record_implicit_move_for_owned_expr(rhs, rhs_prop.ty);

        Some(ExprProp::new(TyId::unit(self.db), true))
    }

    fn check_index_mut_target(&mut self, lhs: ExprId) -> Option<MutableIndexTarget<'db>> {
        let Partial::Present(Expr::Bin(base, index, BinOp::Index)) = self.env.expr_data(lhs) else {
            return None;
        };

        let base_prop = self.check_expr_unknown(*base);
        if base_prop.ty.has_invalid(self.db) {
            return Some(MutableIndexTarget {
                prop: ExprProp::invalid(self.db),
                target_ty: TyId::invalid(self.db, InvalidCause::Other),
                trait_lowered: false,
            });
        }

        let base_place_ty = base_prop
            .ty
            .as_capability(self.db)
            .map_or(base_prop.ty, |(_, inner)| inner);
        if base_place_ty.is_array(self.db) {
            let args = base_place_ty.generic_args(self.db);
            let lhs_ty = args[0];
            let index_ty = args[1].const_ty_ty(self.db).unwrap();
            self.check_expr(*index, index_ty);
            if let Some(index_value) = self.try_eval_static_int(*index, index_ty)
                && let Some(len) = base_place_ty.array_len(self.db)
                && index_value.data(self.db) >= &BigUint::from(len)
            {
                self.push_diag(BodyDiag::ArrayIndexOutOfBounds {
                    primary: index.span(self.body()).into(),
                    index: index_value,
                    len,
                })
            }
            let typed_lhs = ExprProp::new(lhs_ty, base_prop.is_mut);
            self.unify_ty(
                Typeable::Expr(lhs, typed_lhs.clone()),
                typed_lhs.ty,
                typed_lhs.ty,
            );

            return Some(MutableIndexTarget {
                prop: typed_lhs,
                target_ty: self.assignment_target_ty(lhs, lhs_ty),
                trait_lowered: false,
            });
        }

        let indexed = self.check_ops_trait(lhs, base_prop.ty, &IndexMutOp, Some(*index));
        if indexed.ty.has_invalid(self.db) {
            return Some(MutableIndexTarget {
                prop: indexed,
                target_ty: TyId::invalid(self.db, InvalidCause::Other),
                trait_lowered: true,
            });
        }

        let Some((CapabilityKind::Mut, lhs_ty)) = indexed.ty.as_capability(self.db) else {
            let expected = TyId::borrow_mut_of(self.db, self.fresh_ty());
            self.push_diag(BodyDiag::TypeMismatch {
                span: lhs.span(self.body()).into(),
                expected,
                given: indexed.ty,
            });
            return Some(MutableIndexTarget {
                prop: ExprProp::invalid(self.db),
                target_ty: TyId::invalid(self.db, InvalidCause::Other),
                trait_lowered: true,
            });
        };
        self.unify_ty(Typeable::Expr(lhs, indexed.clone()), indexed.ty, indexed.ty);

        Some(MutableIndexTarget {
            prop: indexed,
            target_ty: lhs_ty,
            trait_lowered: true,
        })
    }

    fn check_aug_assign(&mut self, expr: ExprId, expr_data: &Expr<'db>) -> ExprProp<'db> {
        let Expr::AugAssign(lhs, rhs, op) = expr_data else {
            unreachable!()
        };

        let unit = ExprProp::new(TyId::unit(self.db), true);

        let (typed_lhs, lhs_place_ty, trait_lowered) =
            if let Some(target) = self.check_index_mut_target(*lhs) {
                (target.prop, target.target_ty, target.trait_lowered)
            } else {
                let typed_lhs = self.check_expr_unknown(*lhs);
                let lhs_place_ty = typed_lhs
                    .ty
                    .as_capability(self.db)
                    .map_or(typed_lhs.ty, |(_, target)| target);
                (typed_lhs, lhs_place_ty, false)
            };
        if typed_lhs.ty.has_invalid(self.db) {
            return unit;
        }
        // Compound assignment mutates the referent. A writable slot containing
        // a shared reference permits replacing the reference, not mutating it.
        if matches!(
            typed_lhs.ty.as_capability(self.db),
            Some((CapabilityKind::Ref, _))
        ) {
            self.push_diag(BodyDiag::ImmutableAssignment {
                primary: lhs.span(self.body()).into(),
                binding: None,
            });
            self.check_expr_unknown(*rhs);
            return unit;
        }
        if !trait_lowered
            && self.check_assign_lhs(*lhs, &typed_lhs) == AssignLhsStatus::NonAssignable
        {
            return unit;
        }

        // Avoid 'type must be known' diagnostics for unknown integer ty
        if lhs_place_ty.is_integral_var(self.db) {
            self.check_expr(*rhs, lhs_place_ty);
            return unit;
        }

        let lhs_base_ty = lhs_place_ty.base_ty(self.db);
        if lhs_base_ty.is_ty_var(self.db) {
            let diag = BodyDiag::TypeMustBeKnown(lhs.span(self.body()).into());
            self.push_diag(diag);
            return unit;
        }

        // `x += y` is semantically defined by the `*Assign` traits. Primitive
        // integer fast paths are introduced later during MIR lowering without
        // changing which trait method the source program resolves to here.
        self.check_ops_trait(expr, lhs_place_ty, &AugAssignOp(*op), Some(*rhs));

        // Return unit ty even if trait resolution fails
        unit
    }

    /// Resolve a core::ops trait method for an operator on a given LHS type and
    /// optionally check the RHS against the inferred method parameter type.
    /// Returns the fully-instantiated function type and concrete trait instance.
    fn check_ops_trait(
        &mut self,
        expr: ExprId,
        lhs_ty: TyId<'db>,
        op: &dyn TraitOps,
        rhs_expr: Option<ExprId>,
    ) -> ExprProp<'db> {
        let Some(trait_def) =
            resolve_core_trait(self.db, self.env.scope(), &op.trait_path_segments())
        else {
            return ExprProp::invalid(self.db);
        };

        let lhs_candidates = self.capability_fallback_candidates(lhs_ty);
        let lookup = self.env.method_lookup(self.env.scope());
        let mut checked_rhs_ty = None;

        let (method, inst) = if let Some(rhs_expr) = rhs_expr {
            let mut selected_lhs_ty = lhs_candidates[0];
            let mut c_lhs_ty = Canonicalized::new(self.db, selected_lhs_ty);
            let mut method_candidates = select_trait_method_candidates(
                self.db,
                &c_lhs_ty,
                op.trait_method(self.db),
                lookup,
                trait_def,
            );
            if matches!(
                method_candidates,
                Err(MethodSelectionError::NotFound | MethodSelectionError::ReceiverTypeMustBeKnown)
            ) {
                for &candidate_ty in lhs_candidates.iter().skip(1) {
                    let c_candidate_ty = Canonicalized::new(self.db, candidate_ty);
                    let fallback = select_trait_method_candidates(
                        self.db,
                        &c_candidate_ty,
                        op.trait_method(self.db),
                        lookup,
                        trait_def,
                    );
                    if fallback.is_ok() || !matches!(fallback, Err(MethodSelectionError::NotFound))
                    {
                        selected_lhs_ty = candidate_ty;
                        c_lhs_ty = c_candidate_ty;
                        method_candidates = fallback;
                        break;
                    }
                }
            }

            let method_candidates = match method_candidates {
                Ok(method_candidates) => method_candidates,
                Err(MethodSelectionError::NotFound) => {
                    let diag = BodyDiag::ops_trait_not_implemented(
                        self.db,
                        expr.span(self.body()).into(),
                        lhs_ty,
                        op,
                    );
                    self.push_diag(diag);
                    return ExprProp::invalid(self.db);
                }
                Err(err) => {
                    let span = expr.span(self.body());
                    let diag = body_diag_from_method_selection_err(
                        self.db,
                        err,
                        Spanned::new(lhs_ty, span.clone().into()),
                        Spanned::new(op.trait_method(self.db), span.into()),
                    );
                    self.push_diag(diag);
                    return ExprProp::invalid(self.db);
                }
            };
            let rhs = self
                .env
                .typed_expr(rhs_expr)
                .unwrap_or_else(|| self.check_expr_unknown(rhs_expr));
            if rhs.ty.has_invalid(self.db) {
                return ExprProp::invalid(self.db);
            }

            let mut viable = if let [candidate] = method_candidates.candidates.as_slice() {
                vec![*candidate]
            } else {
                let mut viable = Vec::new();
                for candidate in method_candidates.candidates.iter().copied() {
                    let snapshot = self.snapshot_state();
                    let unifies = (|| {
                        let inst = c_lhs_ty.extract_solution(&mut self.table, candidate.cand.inst);
                        let func_ty = super::try_instantiate_trait_method(
                            self.db,
                            candidate.cand.method,
                            &mut self.table,
                            selected_lhs_ty,
                            inst,
                        )
                        .ok()?;
                        let func_ty = self.table.instantiate_to_term(func_ty);
                        let expected_rhs = self.instantiated_ops_rhs_ty(func_ty, inst)?;
                        let rhs_ty = self
                            .try_coerce_capability_for_expr_to_expected(
                                rhs_expr,
                                rhs.ty,
                                expected_rhs,
                            )
                            .unwrap_or(rhs.ty);
                        self.table.unify(rhs_ty, expected_rhs).ok()
                    })()
                    .is_some();
                    self.rollback_state(snapshot);
                    if unifies {
                        viable.push(candidate);
                    }
                }
                viable
            };

            match viable.len() {
                0 => {
                    let diag = BodyDiag::ops_trait_not_implemented(
                        self.db,
                        expr.span(self.body()).into(),
                        lhs_ty,
                        op,
                    );
                    self.push_diag(diag);
                    return ExprProp::invalid(self.db);
                }
                1 => {
                    let candidate = viable.pop().unwrap();
                    let inst = c_lhs_ty.extract_solution(&mut self.table, candidate.cand.inst);
                    let func_ty = self.instantiate_trait_method_to_term(
                        candidate.cand.method,
                        selected_lhs_ty,
                        inst,
                    );
                    if let Some(expected_rhs) = self.instantiated_ops_rhs_ty(func_ty, inst) {
                        let rhs_ty = self
                            .try_coerce_capability_for_expr_to_expected(
                                rhs_expr,
                                rhs.ty,
                                expected_rhs,
                            )
                            .unwrap_or(rhs.ty);
                        // Like a call argument, the operand keeps its own type:
                        // the call boundary applies the coercion, so a viewed
                        // operand is borrowed in place rather than moved.
                        checked_rhs_ty = Some(self.equate_ty(
                            rhs_ty,
                            expected_rhs,
                            rhs_expr.span(self.body()).into(),
                        ));
                    }
                    (func_ty, inst)
                }
                _ => {
                    let cands = viable
                        .into_iter()
                        .map(|candidate| {
                            c_lhs_ty.extract_solution(&mut self.table, candidate.cand.inst)
                        })
                        .collect();
                    self.push_diag(BodyDiag::AmbiguousTraitInst {
                        primary: expr.span(self.body()).into(),
                        cands,
                        required_by: None,
                    });
                    return ExprProp::invalid(self.db);
                }
            }
        } else {
            let mut selected_lhs_ty = lhs_candidates[0];
            let mut c_lhs_ty = Canonicalized::new(self.db, selected_lhs_ty);
            let mut method_candidate = select_method_candidate(
                self.db,
                &c_lhs_ty,
                op.trait_method(self.db),
                lookup,
                Some(trait_def),
            );
            if matches!(
                method_candidate,
                Err(MethodSelectionError::NotFound | MethodSelectionError::ReceiverTypeMustBeKnown)
            ) {
                for &candidate_ty in lhs_candidates.iter().skip(1) {
                    let c_candidate_ty = Canonicalized::new(self.db, candidate_ty);
                    let fallback = select_method_candidate(
                        self.db,
                        &c_candidate_ty,
                        op.trait_method(self.db),
                        lookup,
                        Some(trait_def),
                    );
                    if fallback.is_ok() || !matches!(fallback, Err(MethodSelectionError::NotFound))
                    {
                        selected_lhs_ty = candidate_ty;
                        c_lhs_ty = c_candidate_ty;
                        method_candidate = fallback;
                        break;
                    }
                }
            }

            match method_candidate {
                Ok(MethodCandidate::InherentMethod(_)) => unreachable!(),
                Ok(
                    MethodCandidate::TraitMethod(cand) | MethodCandidate::NeedsConfirmation(cand),
                ) => {
                    let inst = c_lhs_ty.extract_solution(&mut self.table, cand.inst);

                    let func_ty =
                        self.instantiate_trait_method_to_term(cand.method, selected_lhs_ty, inst);
                    (func_ty, inst)
                }
                Err(MethodSelectionError::AmbiguousTraitMethod(_)) => {
                    unreachable!("unary core::ops ambiguity")
                }
                Err(MethodSelectionError::NotFound) => {
                    let diag = BodyDiag::ops_trait_not_implemented(
                        self.db,
                        expr.span(self.body()).into(),
                        lhs_ty,
                        op,
                    );
                    self.push_diag(diag);
                    return ExprProp::invalid(self.db);
                }
                Err(err) => {
                    let span = expr.span(self.body());
                    let diag = body_diag_from_method_selection_err(
                        self.db,
                        err,
                        Spanned::new(lhs_ty, span.clone().into()),
                        Spanned::new(op.trait_method(self.db), span.into()),
                    );
                    self.push_diag(diag);
                    return ExprProp::invalid(self.db);
                }
            }
        };

        let mut callable =
            Callable::new(self.db, method, expr.span(self.body()).into(), Some(inst))
                .expect("failed to create Callable for core::ops trait method");

        let mut checked_inputs = vec![self.normalize_ty(lhs_ty)];
        if let Some(rhs_expr) = rhs_expr {
            let rhs_ty = checked_rhs_ty.unwrap_or_else(|| {
                self.env
                    .typed_expr(rhs_expr)
                    .expect("checked operator RHS has a type")
                    .ty
            });
            checked_inputs.push(self.normalize_ty(rhs_ty));
        }
        callable.set_checked_input_tys(checked_inputs);

        callable.process_constraints(self, expr, expr.span(self.body()).into());
        let ret_ty = self.normalize_ty(callable.ret_ty(self.db));
        self.env.register_semantic_call(expr, callable);
        ExprProp::new(ret_ty, true)
    }

    fn instantiated_ops_rhs_ty(
        &mut self,
        func_ty: TyId<'db>,
        inst: TraitInstId<'db>,
    ) -> Option<TyId<'db>> {
        let (base, gen_args) = func_ty.decompose_ty_app(self.db);
        let TyData::TyBase(TyBase::Func(func_def)) = base.data(self.db) else {
            return None;
        };
        let expected_rhs = func_def
            .arg_tys(self.db)
            .get(1)?
            .instantiate(self.db, gen_args);
        let expected_rhs = self.normalize_ty(normalize_with_trait_evidence(
            self.db,
            expected_rhs,
            self.env.scope(),
            inst,
        ));
        Some(expected_rhs)
    }

    fn check_assign_lhs(&mut self, lhs: ExprId, typed_lhs: &ExprProp<'db>) -> AssignLhsStatus {
        if !self.is_assignable_expr(lhs) || !self.env.is_place_expr(lhs) {
            if !typed_lhs.ty.has_invalid(self.db) {
                let diag = BodyDiag::NonAssignableExpr(lhs.span(self.body()).into());
                self.push_diag(diag);
            }

            return AssignLhsStatus::NonAssignable;
        }

        if !typed_lhs.is_mut {
            let binding = self.find_base_binding(lhs);
            let diag = match binding {
                Some(binding) => {
                    let (ident, def_span) =
                        (binding.binding_name(&self.env), binding.def_span(&self.env));

                    BodyDiag::ImmutableAssignment {
                        primary: lhs.span(self.body()).into(),
                        binding: Some((ident, def_span)),
                    }
                }

                None => BodyDiag::ImmutableAssignment {
                    primary: lhs.span(self.body()).into(),
                    binding: None,
                },
            };

            self.push_diag(diag);
            return AssignLhsStatus::Immutable;
        }

        AssignLhsStatus::Assignable
    }

    fn check_expr_in_new_scope(
        &mut self,
        expr: ExprId,
        expected: TyId<'db>,
        result_discarded: bool,
    ) -> ExprProp<'db> {
        self.env.enter_scope(expr);
        let ty = if result_discarded {
            self.check_expr_with_discarded_result(expr, expected)
        } else {
            self.check_expr(expr, expected)
        };
        self.env.leave_scope();

        ty
    }

    pub(super) fn report_cannot_borrow_mut(&mut self, expr: ExprId, primary: DynLazySpan<'db>) {
        let binding = self
            .find_base_binding(expr)
            .map(|binding| (binding.binding_name(&self.env), binding.def_span(&self.env)));
        self.push_diag(BodyDiag::CannotBorrowMut { primary, binding });
    }

    /// Returns the base binding for a given expression if it exists.
    ///
    /// This function traverses the expression tree to find the base binding,
    /// which is the original variable or binding that the expression refers to.
    ///
    /// # Parameters
    ///
    /// - `expr`: The expression ID for which to find the base binding.
    ///
    /// # Returns
    ///
    /// An `Option` containing the `LocalBinding` if a base binding is found,
    /// or `None` if there is no base binding.
    fn find_base_binding(&self, expr: ExprId) -> Option<LocalBinding<'db>> {
        let Partial::Present(expr_data) = self.env.expr_data(expr) else {
            return None;
        };

        match expr_data {
            Expr::Field(lhs, ..) => self.find_base_binding(*lhs),
            Expr::Bin(lhs, _rhs, op) if *op == BinOp::Index => self.find_base_binding(*lhs),
            Expr::Un(inner, UnOp::Deref) => self.find_base_binding(*inner),
            Expr::Path(..) => self.env.typed_expr(expr)?.binding,
            _ => None,
        }
    }

    /// Returns `true`` if the expression can be used as an left hand side of an
    /// assignment.
    /// This method doesn't take mutability into account.
    pub(super) fn is_assignable_expr(&self, expr: ExprId) -> bool {
        let Partial::Present(expr_data) = expr.data(self.db, self.body()) else {
            return false;
        };

        match expr_data {
            Expr::Path(..) | Expr::Field(..) => true,
            Expr::Bin(_, _, op) if *op == BinOp::Index => true,
            Expr::Un(_, UnOp::Deref) => true,
            _ => false,
        }
    }

    fn is_pointer_deref_expr(&self, expr: ExprId) -> bool {
        matches!(
            expr.data(self.db, self.body()),
            Partial::Present(Expr::Un(_, UnOp::Deref))
        )
    }
}

fn reify_unresolved_pattern_slots<'db, T>(
    db: &'db dyn HirAnalysisDb,
    value: T,
    slot_bindings: &FxHashMap<TyId<'db>, TyId<'db>>,
) -> T
where
    T: crate::analysis::ty::fold::TyFoldable<'db>,
{
    struct SlotReifier<'a, 'db> {
        slot_bindings: &'a FxHashMap<TyId<'db>, TyId<'db>>,
    }

    impl<'db> crate::analysis::ty::fold::TyFolder<'db> for SlotReifier<'_, 'db> {
        fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
            self.slot_bindings
                .get(&ty)
                .copied()
                .unwrap_or_else(|| ty.super_fold_with(db, self))
        }
    }

    value.fold_with(db, &mut SlotReifier { slot_bindings })
}

fn named_member_diagnostic<'db>(
    db: &'db dyn HirAnalysisDb,
    scope: ScopeId<'db>,
    receiver: TyId<'db>,
    member: IdentId<'db>,
    primary: DynLazySpan<'db>,
) -> Option<PathResDiag<'db>> {
    let candidates = named_impls_for_member(db, scope, receiver, member);
    match candidates.as_slice() {
        [] => None,
        [(selection, selected)] => Some(PathResDiag::NamedImplementationRequired {
            primary,
            member,
            selection: *selection,
            defined_at: selected.span().selection_name().into(),
        }),
        _ => {
            // A repeated alias has its own selection ambiguity, so it is not
            // a useful choice to suggest at an unscoped call.
            if candidates.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                return None;
            }
            Some(PathResDiag::NamedImplementationChoiceRequired {
                primary,
                member,
                choices: candidates
                    .into_iter()
                    .map(|(selection, selected)| {
                        (selection, selected.span().selection_name().into())
                    })
                    .collect(),
            })
        }
    }
}

fn body_diag_from_method_selection_err<'db>(
    db: &'db dyn HirAnalysisDb,
    err: MethodSelectionError<'db>,
    receiver: Spanned<'db, TyId<'db>>,
    method: Spanned<'db, IdentId<'db>>,
) -> FuncBodyDiag<'db> {
    match err {
        MethodSelectionError::ReceiverTypeMustBeKnown => {
            BodyDiag::TypeMustBeKnown(receiver.span).into()
        }
        MethodSelectionError::AmbiguousInherentMethod(candidates) => {
            BodyDiag::AmbiguousInherentMethodCall {
                primary: method.span,
                method_name: method.data,
                candidates,
            }
            .into()
        }

        MethodSelectionError::AmbiguousTraitMethod(ambiguous) => BodyDiag::AmbiguousTrait {
            primary: method.span,
            method_name: method.data,
            traits: ambiguous.diagnostic_traits,
        }
        .into(),

        MethodSelectionError::NotFound => {
            let base_ty = receiver.data.base_ty(db);
            PathResDiag::MethodNotFound {
                primary: method.span,
                method_name: method.data,
                receiver: Either::Left(base_ty),
            }
            .into()
        }

        MethodSelectionError::InvisibleInherentMethod(func) => {
            PathResDiag::Invisible(method.span, method.data, func.name_span().into()).into()
        }

        MethodSelectionError::InvisibleTraitMethod(traits) => BodyDiag::InvisibleAmbiguousTrait {
            primary: method.span,
            traits,
        }
        .into(),
    }
}

pub(super) fn resolve_ident_expr<'db>(
    db: &'db dyn HirAnalysisDb,
    env: &TyCheckEnv<'db>,
    path: PathId<'db>,
    ident_span: DynLazySpan<'db>,
    minter: &LoweringContext<'db>,
) -> ResolvedPathInBody<'db> {
    let ident = path.ident(db).unwrap();

    let resolve_bucket = |bucket: &NameResBucket<'db>, scope| {
        // First, surface any ambiguity/conflict in the bucket as a dedicated
        // name-resolution diagnostic instead of silently degrading to
        // "undefined variable".
        for (_, err) in bucket.errors() {
            match err {
                NameResolutionError::Ambiguous(cands) => {
                    let mut cand_spans = Vec::new();
                    for name in cands.iter() {
                        if let Some(span) = name.kind.name_span(db) {
                            let from_implicit = name
                                .derivation
                                .use_stmt()
                                .is_some_and(|use_| use_.is_synthetic_use(db));
                            cand_spans.push((span, from_implicit));
                        }
                    }

                    let diag = PathResDiag::Ambiguous(ident_span.clone(), ident, cand_spans);
                    return ResolvedPathInBody::Diag(diag.into());
                }
                NameResolutionError::Conflict(conf_ident, spans) => {
                    let diag = PathResDiag::Conflict(*conf_ident, spans.clone());
                    return ResolvedPathInBody::Diag(diag.into());
                }
                _ => {}
            }
        }

        let Ok(res) = bucket.pick_any(&[NameDomain::VALUE, NameDomain::TYPE]) else {
            return ResolvedPathInBody::Invalid;
        };
        let Ok(reso) =
            resolve_name_res_with_minter(db, res, None, path, scope, env.assumptions(), minter)
        else {
            return ResolvedPathInBody::Invalid;
        };
        ResolvedPathInBody::Reso(reso)
    };

    let mut current_idx = env.current_block_idx();

    loop {
        let block = env.get_block(current_idx);
        if let Some(binding) = block.lookup_var(ident) {
            return ResolvedPathInBody::Binding(binding);
        }

        let scope = block.scope;
        let directive = QueryDirective::for_scope(db, scope).disallow_lex();
        let query = EarlyNameQueryId::new(db, ident, scope, directive);
        let bucket = resolve_query(db, query);

        let resolved = resolve_bucket(bucket, scope);
        if matches!(resolved, ResolvedPathInBody::Invalid) {
            if current_idx == 0 {
                break;
            }
            current_idx -= 1;
        } else {
            return resolved;
        }
    }

    let body_scope = env.body().scope();
    let directive = QueryDirective::for_scope(db, body_scope);
    let query = EarlyNameQueryId::new(db, ident, body_scope, directive);
    let bucket = resolve_query(db, query);
    match resolve_bucket(bucket, env.scope()) {
        ResolvedPathInBody::Invalid => ResolvedPathInBody::NewBinding(ident),
        r => r,
    }
}

/// If `ident` names a function of the `impl`, `impl trait` or `trait` that
/// encloses `body`, returns the container kind (`"impl"` or `"trait"`) and
/// whether that function takes `self`. Such functions are not in scope as bare
/// names.
fn enclosing_assoc_fn<'db>(
    db: &'db dyn HirAnalysisDb,
    body: crate::hir_def::Body<'db>,
    ident: IdentId<'db>,
) -> Option<(&'static str, bool)> {
    let mut scope = body.scope();
    let (container, kind) = loop {
        let item = scope.parent_item(db)?;
        match item {
            ItemKind::Impl(_) | ItemKind::ImplTrait(_) => break (item, "impl"),
            ItemKind::Trait(_) => break (item, "trait"),
            ItemKind::Mod(_) | ItemKind::TopMod(_) => return None,
            _ => scope = ScopeId::from_item(item),
        }
    };
    ScopeId::from_item(container)
        .child_items(db)
        .find_map(|child| match child {
            ItemKind::Func(func) if func.name(db).to_opt() == Some(ident) => {
                Some((kind, func.is_method(db)))
            }
            _ => None,
        })
}

/// This traits are intended to be implemented by the operators that can work as
/// a syntax sugar for a trait method. For example, binary `+` operator
/// implements this trait to be able to work as a syntax sugar for
/// `core::ops::Add` trait method.
///
/// TODO: We need to refine this trait definition to connect core library traits
/// smoothly.
pub(crate) trait TraitOps {
    fn trait_path_segments(&self) -> [&str; 2] {
        ["ops", self.triple()[0]]
    }

    fn core_trait_path<'db>(&self, db: &'db dyn HirAnalysisDb) -> PathId<'db> {
        let mut path = PathId::from_ident(db, IdentId::new(db, "core".to_string()));
        for s in self.trait_path_segments() {
            path = path.push_ident(db, IdentId::new(db, s.to_string()));
        }
        path
    }

    fn trait_method<'db>(&self, db: &'db dyn HirAnalysisDb) -> IdentId<'db> {
        IdentId::new(db, self.triple()[1].to_string())
    }

    fn op_symbol<'db>(&self, db: &'db dyn HirAnalysisDb) -> IdentId<'db> {
        IdentId::new(db, self.triple()[2].to_string())
    }

    fn triple(&self) -> [&str; 3];
}

impl TraitOps for UnOp {
    fn triple(&self) -> [&str; 3] {
        match self {
            UnOp::Plus => ["UnaryPlus", "add", "+"],
            UnOp::Minus => ["Neg", "neg", "-"],
            UnOp::Not => ["Not", "not", "!"],
            UnOp::BitNot => ["BitNot", "bit_not", "~"],
            UnOp::Mut => ["MutBorrow", "mut_borrow", "mut"],
            UnOp::Ref => ["RefBorrow", "ref_borrow", "ref"],
            UnOp::Deref => unreachable!("pointer dereference is not trait-lowered"),
        }
    }
}

impl TraitOps for BinOp {
    fn triple(&self) -> [&str; 3] {
        match self {
            BinOp::Arith(arith_op) => {
                use ArithBinOp::*;

                match arith_op {
                    Add => ["Add", "add", "+"],
                    Sub => ["Sub", "sub", "-"],
                    Mul => ["Mul", "mul", "*"],
                    Div => ["Div", "div", "/"],
                    Rem => ["Rem", "rem", "%"],
                    Pow => ["Pow", "pow", "**"],
                    LShift => ["Shl", "shl", "<<"],
                    RShift => ["Shr", "shr", ">>"],
                    BitAnd => ["BitAnd", "bitand", "&"],
                    BitOr => ["BitOr", "bitor", "|"],
                    BitXor => ["BitXor", "bitxor", "^"],
                    // Range is handled specially - it constructs a Range type
                    // rather than calling a trait method
                    Range => ["Range", "range", ".."],
                }
            }

            BinOp::Comp(comp_op) => {
                use crate::core::hir_def::CompBinOp::*;

                // Comp
                match comp_op {
                    Eq => ["Eq", "eq", "=="],
                    NotEq => ["Eq", "ne", "!="],
                    Lt => ["Ord", "lt", "<"],
                    LtEq => ["Ord", "le", "<="],
                    Gt => ["Ord", "gt", ">"],
                    GtEq => ["Ord", "ge", ">="],
                }
            }

            BinOp::Logical(_) => {
                unreachable!()
            }

            BinOp::Index => ["Index", "index", "[]"],
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct AugAssignOp(ArithBinOp);

impl TraitOps for AugAssignOp {
    fn triple(&self) -> [&str; 3] {
        use ArithBinOp::*;
        match self.0 {
            Add => ["AddAssign", "add_assign", "+="],
            Sub => ["SubAssign", "sub_assign", "-="],
            Mul => ["MulAssign", "mul_assign", "*="],
            Div => ["DivAssign", "div_assign", "/="],
            Rem => ["RemAssign", "rem_assign", "%="],
            Pow => ["PowAssign", "pow_assign", "**="],
            LShift => ["ShlAssign", "shl_assign", "<<="],
            RShift => ["ShrAssign", "shr_assign", ">>="],
            BitAnd => ["BitAndAssign", "bitand_assign", "&="],
            BitOr => ["BitOrAssign", "bitor_assign", "|="],
            BitXor => ["BitXorAssign", "bitxor_assign", "^="],
            // Range doesn't have an augmented assignment form
            Range => unreachable!("Range operator cannot be used in augmented assignment"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct IndexMutOp;

impl TraitOps for IndexMutOp {
    fn triple(&self) -> [&str; 3] {
        ["IndexMut", "index_mut", "[]="]
    }
}
