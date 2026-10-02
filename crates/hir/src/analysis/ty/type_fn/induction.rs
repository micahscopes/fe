//! Course-of-values membership for checked computed shapes. This establishes a
//! predicate, never a particular runtime implementation or associated binding.

use crate::analysis::ty::{
    binder::Binder,
    const_ty::{ConstTyData, ConstTyId},
    fold::{TyFoldable, TyFolder},
    trait_def::TraitInstId,
    trait_resolution::{
        CanonicalGoalQuery, GoalSatisfiability, PredicateListId, TraitSolveCx, check_ty_wf,
        is_goal_query_satisfiable,
    },
    ty_def::{TyBase, TyData, TyFlags, TyId, TyParam},
    visitor::{TyVisitable, TyVisitor, walk_const_ty},
};
use crate::{
    analysis::{HirAnalysisDb, semantic::ctfe::ConstRepr},
    hir_def::{IdentId, TypeFnDef},
};

fn strict_prove<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: TraitSolveCx<'db>,
    goal: TraitInstId<'db>,
) -> bool {
    let mut validity = InputGate {
        db,
        invalid: false,
        reject_computed: false,
    };
    goal.visit_with(&mut validity);
    cx.assumptions().visit_with(&mut validity);
    if validity.invalid {
        return false;
    }
    let query = CanonicalGoalQuery::new(db, goal, cx.assumptions());
    if query
        .canonical()
        .flags(db)
        .intersects(TyFlags::HAS_INVALID | TyFlags::HAS_VAR)
    {
        return false;
    }
    matches!(
        is_goal_query_satisfiable(db, cx, &query),
        GoalSatisfiability::Satisfied(_)
    )
}

fn strict_formation<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: TraitSolveCx<'db>,
    root: TyId<'db>,
) -> bool {
    if !check_ty_wf(db, cx, root).is_wf() {
        return false;
    }
    struct Requirements<'db> {
        db: &'db dyn HirAnalysisDb,
        cx: TraitSolveCx<'db>,
        valid: bool,
    }
    impl<'db> TyVisitor<'db> for Requirements<'db> {
        fn db(&self) -> &'db dyn HirAnalysisDb {
            self.db
        }
        fn visit_ty(&mut self, ty: TyId<'db>) {
            if !self.valid {
                return;
            }
            for &goal in
                crate::analysis::ty::trait_resolution::constraint::ty_constraints(self.db, ty)
                    .list(self.db)
            {
                if !strict_prove(self.db, self.cx, goal) {
                    self.valid = false;
                    return;
                }
            }
            let (base, args) = ty.decompose_ty_app(self.db);
            for &arg in args {
                arg.visit_with(self);
            }
            match base.data(self.db) {
                // This body belongs to its own binder and was checked there.
                TyData::TypeFamily { .. } => return,
                TyData::AssocTy(assoc) => {
                    self.valid &= strict_prove(self.db, self.cx, assoc.trait_.as_predicate(self.db))
                }
                TyData::QualifiedTy(goal) => self.valid &= strict_prove(self.db, self.cx, *goal),
                _ => {}
            }
            // Walk base content, not the partial application spine.
            crate::analysis::ty::visitor::walk_ty(self, base);
        }
    }
    let mut visitor = Requirements {
        db,
        cx,
        valid: true,
    };
    root.visit_with(&mut visitor);
    visitor.valid
}

fn with_self<'db>(
    db: &'db dyn HirAnalysisDb,
    goal: TraitInstId<'db>,
    ty: TyId<'db>,
) -> TraitInstId<'db> {
    TraitInstId::new_simple(db, goal.def(db), vec![ty])
}

/// Only complete proofs contribute a lemma. A cycle contributes no premise.
#[salsa::tracked(cycle_fn=cycle_recover, cycle_initial=cycle_initial)]
pub(in crate::analysis::ty) fn discharge<'db>(
    db: &'db dyn HirAnalysisDb,
    cx: TraitSolveCx<'db>,
    goal: TraitInstId<'db>,
) -> bool {
    if goal.args(db).len() != 1 || !goal.assoc_type_bindings(db).is_empty() {
        return false;
    }
    let (head, args) = goal.self_ty(db).decompose_ty_app(db);
    let TyData::TyBase(TyBase::TypeFn(def)) = head.data(db) else {
        return false;
    };
    let Some((subject, invariants)) = args.split_last() else {
        return false;
    };
    let TyData::ConstTy(subject) = subject.data(db) else {
        return false;
    };
    match subject.data(db) {
        ConstTyData::TyParam(param, _) if param.is_normal() => {}
        ConstTyData::Abstract(..) => {}
        ConstTyData::Computation { description, .. } if matches!(description.repr(), ConstRepr::Term(term) if TyId::const_ty(db, *term).has_param(db)) =>
            {}
        _ => return false,
    }
    // Keep the historical class: type invariants and one recursive occurrence
    // per arm. In particular, no proof may rely on a specific subject value.
    for &arg in invariants {
        let mut gate = InputGate {
            db,
            invalid: arg.is_const_ty(db),
            reject_computed: true,
        };
        arg.visit_with(&mut gate);
        if gate.invalid {
            return false;
        }
    }
    let checked = super::check_definition_templates(db, *def);
    let Some(arms) = &checked.arms else {
        return false;
    };
    for (index, &template) in arms.iter().enumerate() {
        let opaque = TyParam::induction_param(
            IdentId::new(db, format!("<induction:{index}>")),
            index,
            template.kind(db).clone(),
            def.scope(),
        )
        .ty(db);
        let mut replace = RecursiveCall {
            def: *def,
            opaque,
            count: 0,
        };
        let instantiated = Binder::bind((*def).into(), template).instantiate(db, args);
        let body = instantiated.fold_with(db, &mut replace);
        if replace.count > 1 {
            return false;
        }
        let mut gate = SubjectGate {
            db,
            subject: *subject,
            found: false,
        };
        body.visit_with(&mut gate);
        if gate.found {
            return false;
        }
        let mut assumptions = cx.assumptions().list(db).clone();
        if replace.count == 1 {
            assumptions.push(with_self(db, goal, opaque));
        }
        let arm_cx = cx.with_assumptions(PredicateListId::new(db, assumptions));
        if !strict_formation(db, arm_cx, body)
            || !strict_prove(db, arm_cx, with_self(db, goal, body))
        {
            return false;
        }
    }
    let mut assumptions = cx.assumptions().list(db).clone();
    assumptions.push(goal);
    strict_prove(
        db,
        cx.with_assumptions(PredicateListId::new(db, assumptions)),
        goal,
    )
}

struct RecursiveCall<'db> {
    def: TypeFnDef<'db>,
    opaque: TyId<'db>,
    count: usize,
}
impl<'db> TyFolder<'db> for RecursiveCall<'db> {
    fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        let (head, _) = ty.decompose_ty_app(db);
        if matches!(head.data(db), TyData::TyBase(TyBase::TypeFn(def)) if *def == self.def) {
            self.count += 1;
            self.opaque
        } else {
            ty.super_fold_with(db, self)
        }
    }
}

struct InputGate<'db> {
    db: &'db dyn HirAnalysisDb,
    invalid: bool,
    reject_computed: bool,
}
impl<'db> TyVisitor<'db> for InputGate<'db> {
    fn db(&self) -> &'db dyn HirAnalysisDb {
        self.db
    }
    fn visit_ty(&mut self, ty: TyId<'db>) {
        self.invalid |= ty.has_invalid(self.db) || ty.has_var(self.db);
        crate::analysis::ty::visitor::walk_ty(self, ty);
    }
    fn visit_type_fn(&mut self, _: TypeFnDef<'db>) {
        self.invalid |= self.reject_computed;
    }
    fn visit_const_ty(&mut self, value: &ConstTyId<'db>) {
        self.invalid |= matches!(value.data(self.db), ConstTyData::Hole(..));
        walk_const_ty(self, value);
    }
}

struct SubjectGate<'db> {
    db: &'db dyn HirAnalysisDb,
    subject: ConstTyId<'db>,
    found: bool,
}
impl<'db> TyVisitor<'db> for SubjectGate<'db> {
    fn db(&self) -> &'db dyn HirAnalysisDb {
        self.db
    }
    fn visit_const_ty(&mut self, value: &ConstTyId<'db>) {
        self.found |= *value == self.subject;
        if matches!(value.data(self.db), ConstTyData::UnEvaluated { .. }) {
            // An unevaluated body is opaque to the ordinary type visitor.
            // Ground payloads can be checked directly; an unresolved payload
            // supplies no proof that it is independent of the subject.
            let evaluated = value.evaluate(self.db, None);
            if evaluated == *value || !matches!(evaluated.data(self.db), ConstTyData::Value(..)) {
                self.found = true;
            } else {
                self.visit_const_ty(&evaluated);
            }
        } else {
            walk_const_ty(self, value);
        }
    }
}

fn cycle_initial<'db>(
    _: &'db dyn HirAnalysisDb,
    _: TraitSolveCx<'db>,
    _: TraitInstId<'db>,
) -> bool {
    false
}
fn cycle_recover<'db>(
    _: &'db dyn HirAnalysisDb,
    _: &bool,
    _: u32,
    _: TraitSolveCx<'db>,
    _: TraitInstId<'db>,
) -> salsa::CycleRecoveryAction<bool> {
    salsa::CycleRecoveryAction::Iterate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analysis::ty::ty_def::InvalidCause, test_db::HirAnalysisTestDb};

    #[test]
    fn membership_proof_rejects_ambiguity_cycles_and_invalid_input() {
        let mut db = HirAnalysisTestDb::default();
        let file = db.new_stand_alone(
            "strict_membership.fe".into(),
            r#"
trait Mark {}
struct Yes {}
impl Mark for Yes {}
struct Ambiguous {}
impl Mark for Ambiguous {}
impl Mark for Ambiguous {}
struct Cycle {}
impl Mark for Cycle where Cycle: Mark {}
struct Guarded<T: Mark> {}
"#,
        );
        let (top, _) = db.top_mod(file);
        let trait_ = top.all_traits(&db)[0];
        let cx = TraitSolveCx::new(&db, top.scope());
        for (name, expected) in [("Yes", true), ("Ambiguous", false), ("Cycle", false)] {
            let adt = top
                .all_structs(&db)
                .iter()
                .copied()
                .find(|adt| {
                    adt.name(&db)
                        .to_opt()
                        .is_some_and(|ident| ident.data(&db) == name)
                })
                .unwrap();
            let goal = TraitInstId::new_simple(
                &db,
                trait_,
                vec![TyId::adt(
                    &db,
                    crate::analysis::ty::adt_def::AdtRef::from(adt).as_adt(&db),
                )],
            );
            assert_eq!(strict_prove(&db, cx, goal), expected, "{name}");
            let guarded = top
                .all_structs(&db)
                .iter()
                .copied()
                .find(|adt| {
                    adt.name(&db)
                        .to_opt()
                        .is_some_and(|ident| ident.data(&db) == "Guarded")
                })
                .unwrap();
            let guarded = TyId::adt(
                &db,
                crate::analysis::ty::adt_def::AdtRef::from(guarded).as_adt(&db),
            );
            let applied = TyId::foldl(&db, guarded, goal.args(&db));
            assert_eq!(
                strict_formation(&db, cx, applied),
                expected,
                "formation: {name}"
            );
            if name == "Ambiguous" {
                let query = CanonicalGoalQuery::new(&db, goal, cx.assumptions());
                assert!(matches!(
                    is_goal_query_satisfiable(&db, cx, &query),
                    GoalSatisfiability::NeedsConfirmation { .. }
                ));
            }
        }
        let invalid = TyId::invalid(&db, InvalidCause::TypeLoweringCycle);
        let goal = TraitInstId::new_simple(&db, trait_, vec![invalid]);
        let cx = cx.with_assumptions(PredicateListId::new(&db, vec![goal]));
        assert!(!strict_prove(&db, cx, goal));
    }
}
