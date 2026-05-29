#![allow(dead_code)]

use common::indexmap::IndexSet;

use crate::analysis::{
    HirAnalysisDb,
    ty::{
        fold::{TyFoldable, TyFolder},
        trait_def::{ImplementorId, TraitInstId},
        trait_resolution::PredicateListId,
        ty_def::TyId,
        visitor::{TyVisitable, TyVisitor},
    },
};
use crate::hir_def::{Body, IdentId, Trait, WhereClauseOwner, scope_graph::ScopeId};

// ConstraintListId is the canonical internal representation for assumptions and
// obligations. PredicateListId is now a trait-solver compatibility projection,
// not a separate semantic collection pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ConstraintKind<'db> {
    Trait(TraitInstId<'db>),
    ConstPredicate(ConstPredicateInstId<'db>),
    EffectCapability(EffectCapabilityId<'db>),
    TypeEqual(TypeEqualId<'db>),
    AssocTypeEqual(AssocTypeEqualId<'db>),
    ConstraintApplication(ConstraintApplicationId<'db>),
    WellFormed(TyId<'db>),
    Invalid,
}

impl<'db> From<TraitInstId<'db>> for ConstraintKind<'db> {
    fn from(inst: TraitInstId<'db>) -> Self {
        Self::Trait(inst)
    }
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ConstraintId<'db> {
    pub(crate) kind: ConstraintKind<'db>,
}

impl<'db> ConstraintId<'db> {
    pub(crate) fn from_trait(db: &'db dyn HirAnalysisDb, inst: TraitInstId<'db>) -> Self {
        Self::new(db, ConstraintKind::Trait(inst))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum CompilerCapabilityKind<'db> {
    Reflect(TyId<'db>),
    TypeInfo(TyId<'db>),
    ImplBuilder(ConstraintId<'db>),
    EvidenceBuilder(ConstraintId<'db>),
    ModuleBuilder(ScopeId<'db>),
    ItemBuilder(ScopeId<'db>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum CapabilityMode {
    Read,
    Mut,
}

impl CapabilityMode {
    fn pretty_print(self) -> &'static str {
        match self {
            Self::Read => "read capability",
            Self::Mut => "mut capability",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum EffectCapabilityKey<'db> {
    Type {
        provider: TyId<'db>,
        target: TyId<'db>,
    },
    Trait {
        provider: TyId<'db>,
        requirement: TraitInstId<'db>,
    },
    Compiler(CompilerCapabilityKind<'db>),
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct EffectCapabilityId<'db> {
    pub(crate) mode: CapabilityMode,
    pub(crate) key: EffectCapabilityKey<'db>,
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct TypeEqualId<'db> {
    pub(crate) lhs: TyId<'db>,
    pub(crate) rhs: TyId<'db>,
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct AssocTypeEqualId<'db> {
    pub(crate) trait_inst: TraitInstId<'db>,
    pub(crate) assoc_name: IdentId<'db>,
    pub(crate) value: TyId<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum BuiltinConstraintHead {
    TypeEqual,
    WellFormed,
    EffectCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ConstraintHeadKind<'db> {
    ConcreteTrait(Trait<'db>),
    GenericParam(TyId<'db>),
    Builtin(BuiltinConstraintHead),
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ConstraintHeadId<'db> {
    pub(crate) kind: ConstraintHeadKind<'db>,
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ConstraintApplicationId<'db> {
    pub(crate) head: ConstraintHeadId<'db>,

    #[return_ref]
    pub(crate) args: Vec<TyId<'db>>,
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ConstraintListId<'db> {
    #[return_ref]
    pub(crate) list: Vec<ConstraintId<'db>>,
}

impl<'db> ConstraintListId<'db> {
    pub(crate) fn empty(db: &'db dyn HirAnalysisDb) -> Self {
        Self::new(db, Vec::new())
    }

    pub(crate) fn from_trait_predicates(
        db: &'db dyn HirAnalysisDb,
        predicates: PredicateListId<'db>,
    ) -> Self {
        Self::new(
            db,
            predicates
                .list(db)
                .iter()
                .map(|pred| ConstraintId::from_trait(db, *pred))
                .collect::<Vec<_>>(),
        )
    }

    pub(crate) fn merge(self, db: &'db dyn HirAnalysisDb, other: Self) -> Self {
        let mut constraints = self.list(db).clone();
        constraints.extend(other.list(db));
        Self::new(db, constraints)
    }

    pub(crate) fn is_empty(self, db: &'db dyn HirAnalysisDb) -> bool {
        self.list(db).is_empty()
    }

    /// Project constraints for the current trait-solver backend.
    pub(crate) fn trait_predicates(self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        PredicateListId::new(
            db,
            self.list(db)
                .iter()
                .filter_map(|constraint| match constraint.kind(db) {
                    ConstraintKind::Trait(inst) => Some(inst),
                    _ => None,
                })
                .collect::<Vec<_>>(),
        )
    }

    pub(crate) fn const_predicates(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<ConstPredicateInstId<'db>> {
        self.list(db)
            .iter()
            .filter_map(|constraint| match constraint.kind(db) {
                ConstraintKind::ConstPredicate(pred) => Some(pred),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn extend_all_trait_bounds(self, db: &'db dyn HirAnalysisDb) -> Self {
        let mut constraints: IndexSet<ConstraintId<'db>> = self.list(db).iter().copied().collect();
        for &trait_pred in self.trait_predicates(db).extend_all_bounds(db).list(db) {
            constraints.insert(ConstraintId::from_trait(db, trait_pred));
        }
        Self::new(db, constraints.into_iter().collect::<Vec<_>>())
    }

    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        format!(
            "{{{}}}",
            self.list(db)
                .iter()
                .map(|constraint| constraint.pretty_print(db))
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

impl<'db> ConstraintId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        match self.kind(db) {
            ConstraintKind::Trait(inst) => inst.pretty_print(db, true),
            ConstraintKind::ConstPredicate(pred) => pred.pretty_print(db),
            ConstraintKind::EffectCapability(capability) => capability.pretty_print(db),
            ConstraintKind::TypeEqual(equal) => equal.pretty_print(db),
            ConstraintKind::AssocTypeEqual(equal) => equal.pretty_print(db),
            ConstraintKind::ConstraintApplication(application) => application.pretty_print(db),
            ConstraintKind::WellFormed(ty) => format!("WellFormed<{}>", ty.pretty_print(db)),
            ConstraintKind::Invalid => "<invalid constraint>".to_string(),
        }
    }
}

impl<'db> EffectCapabilityId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        let prefix = self.mode(db).pretty_print();
        match self.key(db) {
            EffectCapabilityKey::Type { provider, target } => {
                format!(
                    "{prefix} {} -> {}",
                    provider.pretty_print(db),
                    target.pretty_print(db)
                )
            }
            EffectCapabilityKey::Trait {
                provider,
                requirement,
            } => format!(
                "{prefix} {} -> {}",
                provider.pretty_print(db),
                requirement.pretty_print(db, true)
            ),
            EffectCapabilityKey::Compiler(capability) => {
                format!("{prefix} {}", capability.pretty_print(db))
            }
        }
    }
}

impl<'db> CompilerCapabilityKind<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        match self {
            Self::Reflect(ty) => format!("Reflect<{}>", ty.pretty_print(db)),
            Self::TypeInfo(ty) => format!("TypeInfo<{}>", ty.pretty_print(db)),
            Self::ImplBuilder(goal) => format!("ImplBuilder<{}>", goal.pretty_print(db)),
            Self::EvidenceBuilder(goal) => {
                format!("EvidenceBuilder<{}>", goal.pretty_print(db))
            }
            Self::ModuleBuilder(_) => "ModuleBuilder".to_string(),
            Self::ItemBuilder(_) => "ItemBuilder".to_string(),
        }
    }
}

impl<'db> TypeEqualId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        format!(
            "{} == {}",
            self.lhs(db).pretty_print(db),
            self.rhs(db).pretty_print(db)
        )
    }
}

impl<'db> AssocTypeEqualId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        format!(
            "{}::{} == {}",
            self.trait_inst(db).pretty_print(db, false),
            self.assoc_name(db).data(db),
            self.value(db).pretty_print(db)
        )
    }
}

impl<'db> ConstraintHeadId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        match self.kind(db) {
            ConstraintHeadKind::ConcreteTrait(trait_) => trait_
                .name(db)
                .to_opt()
                .map_or("<missing trait>".to_string(), |name| {
                    name.data(db).to_string()
                }),
            ConstraintHeadKind::GenericParam(ty) => ty.pretty_print(db).to_string(),
            ConstraintHeadKind::Builtin(head) => head.pretty_print().to_string(),
        }
    }
}

impl BuiltinConstraintHead {
    pub(crate) fn pretty_print(self) -> &'static str {
        match self {
            Self::TypeEqual => "TypeEqual",
            Self::WellFormed => "WellFormed",
            Self::EffectCapability => "EffectCapability",
        }
    }
}

impl<'db> ConstraintApplicationId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        let head = self.head(db).pretty_print(db);
        let args = self
            .args(db)
            .iter()
            .map(|arg| arg.pretty_print(db).to_string())
            .collect::<Vec<_>>();
        if args.is_empty() {
            head
        } else {
            format!("{}<{}>", head, args.join(", "))
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ConstPredicateRef<'db> {
    pub(crate) owner: WhereClauseOwner<'db>,
    pub(crate) index: u32,
}

impl<'db> ConstPredicateRef<'db> {
    pub(crate) fn body(self, db: &'db dyn HirAnalysisDb) -> Body<'db> {
        self.owner
            .where_clause(db)
            .const_predicates(db)
            .get(self.index as usize)
            .copied()
            .expect("const predicate ref index should resolve to a where-clause predicate")
    }
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ConstPredicateInstId<'db> {
    pub(crate) predicate: ConstPredicateRef<'db>,

    #[return_ref]
    pub(crate) args: Vec<TyId<'db>>,
}

impl<'db> ConstPredicateInstId<'db> {
    pub(crate) fn body(self, db: &'db dyn HirAnalysisDb) -> Body<'db> {
        self.predicate(db).body(db)
    }

    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        let body = self.body(db);
        format!(
            "const predicate #{} on {}",
            self.predicate(db).index,
            body.top_mod(db).name(db).data(db).as_str()
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ConstProofId<'db> {
    pub(crate) predicate: ConstPredicateInstId<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct GeneratedImplId<'db> {
    pub(crate) implementor: ImplementorId<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum BuiltinEvidenceKind {
    TypeEquality,
    WellFormedness,
    EffectCapability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ErasedProofTermId<'db> {
    pub(crate) constraint: ConstraintId<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum EvidenceKind<'db> {
    Impl(ImplementorId<'db>),
    Assumption(ConstraintId<'db>),
    ConstProof(ConstProofId<'db>),
    GeneratedImpl(GeneratedImplId<'db>),
    EffectWitness(EffectCapabilityId<'db>),
    Builtin(BuiltinEvidenceKind),
    ErasedProofTerm(ErasedProofTermId<'db>),
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct EvidenceId<'db> {
    pub(crate) kind: EvidenceKind<'db>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ParamEnv<'db> {
    pub(crate) scope: ScopeId<'db>,
    pub(crate) constraints: ConstraintListId<'db>,
}

impl<'db> ParamEnv<'db> {
    pub(crate) fn trait_assumptions(self, db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        self.constraints.trait_predicates(db)
    }

    pub(crate) fn const_assumptions(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Vec<ConstPredicateInstId<'db>> {
        self.constraints.const_predicates(db)
    }
}

impl<'db> TyVisitable<'db> for CompilerCapabilityKind<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        match self {
            Self::Reflect(ty) | Self::TypeInfo(ty) => ty.visit_with(visitor),
            Self::ImplBuilder(goal) | Self::EvidenceBuilder(goal) => goal.visit_with(visitor),
            Self::ModuleBuilder(_) | Self::ItemBuilder(_) => {}
        }
    }
}

impl<'db> TyFoldable<'db> for CompilerCapabilityKind<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        match self {
            Self::Reflect(ty) => Self::Reflect(ty.fold_with(db, folder)),
            Self::TypeInfo(ty) => Self::TypeInfo(ty.fold_with(db, folder)),
            Self::ImplBuilder(goal) => Self::ImplBuilder(goal.fold_with(db, folder)),
            Self::EvidenceBuilder(goal) => Self::EvidenceBuilder(goal.fold_with(db, folder)),
            Self::ModuleBuilder(scope) => Self::ModuleBuilder(scope),
            Self::ItemBuilder(scope) => Self::ItemBuilder(scope),
        }
    }
}

impl<'db> TyVisitable<'db> for EffectCapabilityKey<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        match self {
            Self::Type {
                provider, target, ..
            } => {
                provider.visit_with(visitor);
                target.visit_with(visitor);
            }
            Self::Trait {
                provider,
                requirement,
            } => {
                provider.visit_with(visitor);
                requirement.visit_with(visitor);
            }
            Self::Compiler(capability) => capability.visit_with(visitor),
        }
    }
}

impl<'db> TyFoldable<'db> for EffectCapabilityKey<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        match self {
            Self::Type { provider, target } => Self::Type {
                provider: provider.fold_with(db, folder),
                target: target.fold_with(db, folder),
            },
            Self::Trait {
                provider,
                requirement,
            } => Self::Trait {
                provider: provider.fold_with(db, folder),
                requirement: requirement.fold_with(db, folder),
            },
            Self::Compiler(capability) => Self::Compiler(capability.fold_with(db, folder)),
        }
    }
}

impl<'db> TyVisitable<'db> for EffectCapabilityId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.key(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for EffectCapabilityId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(db, self.mode(db), self.key(db).fold_with(db, folder))
    }
}

impl<'db> TyVisitable<'db> for TypeEqualId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.lhs(visitor.db()).visit_with(visitor);
        self.rhs(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for TypeEqualId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(
            db,
            self.lhs(db).fold_with(db, folder),
            self.rhs(db).fold_with(db, folder),
        )
    }
}

impl<'db> TyVisitable<'db> for AssocTypeEqualId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.trait_inst(visitor.db()).visit_with(visitor);
        self.value(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for AssocTypeEqualId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(
            db,
            self.trait_inst(db).fold_with(db, folder),
            self.assoc_name(db),
            self.value(db).fold_with(db, folder),
        )
    }
}

impl<'db> TyVisitable<'db> for ConstraintHeadKind<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        match self {
            Self::ConcreteTrait(_) | Self::Builtin(_) => {}
            Self::GenericParam(ty) => ty.visit_with(visitor),
        }
    }
}

impl<'db> TyFoldable<'db> for ConstraintHeadKind<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        match self {
            Self::ConcreteTrait(trait_) => Self::ConcreteTrait(trait_),
            Self::GenericParam(ty) => Self::GenericParam(ty.fold_with(db, folder)),
            Self::Builtin(head) => Self::Builtin(head),
        }
    }
}

impl<'db> TyVisitable<'db> for ConstraintHeadId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.kind(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ConstraintHeadId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(db, self.kind(db).fold_with(db, folder))
    }
}

impl<'db> TyVisitable<'db> for ConstraintApplicationId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.head(visitor.db()).visit_with(visitor);
        self.args(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ConstraintApplicationId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(
            db,
            self.head(db).fold_with(db, folder),
            self.args(db)
                .iter()
                .map(|arg| arg.fold_with(db, folder))
                .collect::<Vec<_>>(),
        )
    }
}

impl<'db> TyVisitable<'db> for ConstraintKind<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        match self {
            Self::Trait(inst) => inst.visit_with(visitor),
            Self::ConstPredicate(pred) => pred.visit_with(visitor),
            Self::EffectCapability(capability) => capability.visit_with(visitor),
            Self::TypeEqual(equal) => equal.visit_with(visitor),
            Self::AssocTypeEqual(equal) => equal.visit_with(visitor),
            Self::ConstraintApplication(application) => application.visit_with(visitor),
            Self::WellFormed(ty) => ty.visit_with(visitor),
            Self::Invalid => {}
        }
    }
}

impl<'db> TyFoldable<'db> for ConstraintKind<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        match self {
            Self::Trait(inst) => Self::Trait(inst.fold_with(db, folder)),
            Self::ConstPredicate(pred) => Self::ConstPredicate(pred.fold_with(db, folder)),
            Self::EffectCapability(capability) => {
                Self::EffectCapability(capability.fold_with(db, folder))
            }
            Self::TypeEqual(equal) => Self::TypeEqual(equal.fold_with(db, folder)),
            Self::AssocTypeEqual(equal) => Self::AssocTypeEqual(equal.fold_with(db, folder)),
            Self::ConstraintApplication(application) => {
                Self::ConstraintApplication(application.fold_with(db, folder))
            }
            Self::WellFormed(ty) => Self::WellFormed(ty.fold_with(db, folder)),
            Self::Invalid => Self::Invalid,
        }
    }
}

impl<'db> TyVisitable<'db> for ConstraintId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.kind(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ConstraintId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(db, self.kind(db).fold_with(db, folder))
    }
}

impl<'db> TyVisitable<'db> for ConstraintListId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.list(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ConstraintListId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(
            db,
            self.list(db)
                .iter()
                .map(|constraint| constraint.fold_with(db, folder))
                .collect::<Vec<_>>(),
        )
    }
}

impl<'db> TyVisitable<'db> for ConstPredicateInstId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.args(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ConstPredicateInstId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        let args: Vec<_> = self
            .args(db)
            .iter()
            .map(|arg| arg.fold_with(db, folder))
            .collect();
        Self::new(db, self.predicate(db), args)
    }
}

impl<'db> TyVisitable<'db> for ConstProofId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.predicate.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ConstProofId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            predicate: self.predicate.fold_with(db, folder),
        }
    }
}

impl<'db> TyVisitable<'db> for GeneratedImplId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.implementor.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for GeneratedImplId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            implementor: self.implementor.fold_with(db, folder),
        }
    }
}

impl<'db> TyVisitable<'db> for ErasedProofTermId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.constraint.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ErasedProofTermId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            constraint: self.constraint.fold_with(db, folder),
        }
    }
}

impl<'db> TyVisitable<'db> for EvidenceKind<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        match self {
            Self::Impl(implementor) => implementor.visit_with(visitor),
            Self::Assumption(constraint) => constraint.visit_with(visitor),
            Self::ConstProof(proof) => proof.visit_with(visitor),
            Self::GeneratedImpl(generated) => generated.visit_with(visitor),
            Self::EffectWitness(capability) => capability.visit_with(visitor),
            Self::Builtin(_) => {}
            Self::ErasedProofTerm(proof) => proof.visit_with(visitor),
        }
    }
}

impl<'db> TyFoldable<'db> for EvidenceKind<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        match self {
            Self::Impl(implementor) => Self::Impl(implementor.fold_with(db, folder)),
            Self::Assumption(constraint) => Self::Assumption(constraint.fold_with(db, folder)),
            Self::ConstProof(proof) => Self::ConstProof(proof.fold_with(db, folder)),
            Self::GeneratedImpl(generated) => Self::GeneratedImpl(generated.fold_with(db, folder)),
            Self::EffectWitness(capability) => {
                Self::EffectWitness(capability.fold_with(db, folder))
            }
            Self::Builtin(kind) => Self::Builtin(kind),
            Self::ErasedProofTerm(proof) => Self::ErasedProofTerm(proof.fold_with(db, folder)),
        }
    }
}

impl<'db> TyVisitable<'db> for EvidenceId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.kind(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for EvidenceId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(db, self.kind(db).fold_with(db, folder))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_db::HirAnalysisTestDb;

    #[test]
    fn constraint_application_pretty_prints_builtin_head() {
        let db = HirAnalysisTestDb::default();
        let head = ConstraintHeadId::new(
            &db,
            ConstraintHeadKind::Builtin(BuiltinConstraintHead::TypeEqual),
        );
        let application =
            ConstraintApplicationId::new(&db, head, vec![TyId::bool(&db), TyId::u256(&db)]);
        let constraint = ConstraintId::new(&db, ConstraintKind::ConstraintApplication(application));

        assert_eq!(constraint.pretty_print(&db), "TypeEqual<bool, u256>");
    }

    #[test]
    fn capability_constraints_are_not_trait_projection() {
        let db = HirAnalysisTestDb::default();
        let capability = EffectCapabilityId::new(
            &db,
            CapabilityMode::Read,
            EffectCapabilityKey::Compiler(CompilerCapabilityKind::Reflect(TyId::bool(&db))),
        );
        let constraint = ConstraintId::new(&db, ConstraintKind::EffectCapability(capability));
        let list = ConstraintListId::new(&db, vec![constraint]);

        assert!(list.trait_predicates(&db).list(&db).is_empty());
        assert_eq!(
            constraint.pretty_print(&db),
            "read capability Reflect<bool>"
        );
    }

    #[test]
    fn capability_mode_participates_in_identity_and_pretty_printing() {
        let db = HirAnalysisTestDb::default();
        let read = EffectCapabilityId::new(
            &db,
            CapabilityMode::Read,
            EffectCapabilityKey::Compiler(CompilerCapabilityKind::Reflect(TyId::bool(&db))),
        );
        let mut_ = EffectCapabilityId::new(
            &db,
            CapabilityMode::Mut,
            EffectCapabilityKey::Compiler(CompilerCapabilityKind::Reflect(TyId::bool(&db))),
        );

        assert_ne!(read, mut_);
        assert_eq!(read.pretty_print(&db), "read capability Reflect<bool>");
        assert_eq!(mut_.pretty_print(&db), "mut capability Reflect<bool>");
    }
}
