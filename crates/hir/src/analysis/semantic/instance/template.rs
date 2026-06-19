use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            const_ty::ConstTyData,
            fold::{TyFoldable, TyFolder},
            trait_def::{ImplementorId, TraitInstId},
            trait_resolution::PredicateListId,
            ty_check::{
                BodyOwner, EffectProviderSpecialization, TypedBody, check_anon_const_body,
                check_const_body, check_contract_init_body, check_contract_recv_arm_body,
                check_func_body,
            },
            ty_def::{TyData, TyId},
        },
    },
    hir_def::scope_graph::ScopeId,
};
use salsa::Update;

#[derive(Clone, Debug)]
pub struct TypedBodyTemplate<'db> {
    pub owner: BodyOwner<'db>,
    pub body: TypedBody<'db>,
}

pub fn typed_body_template<'db>(
    db: &'db dyn HirAnalysisDb,
    owner: BodyOwner<'db>,
) -> TypedBodyTemplate<'db> {
    let typed_body = match owner {
        BodyOwner::Func(func) => check_func_body(db, func).1.clone(),
        BodyOwner::Const(const_) => check_const_body(db, const_).1.clone(),
        BodyOwner::AnonConstBody { body, expected } => {
            check_anon_const_body(db, body, expected).1.clone()
        }
        BodyOwner::ContractInit { contract } => check_contract_init_body(db, contract).1.clone(),
        BodyOwner::ContractRecvArm {
            contract,
            recv_idx,
            arm_idx,
        } => check_contract_recv_arm_body(db, contract, recv_idx, arm_idx)
            .1
            .clone(),
    };

    TypedBodyTemplate {
        owner,
        body: typed_body,
    }
}

#[salsa::interned]
#[derive(Debug)]
pub struct GenericSubst<'db> {
    #[return_ref]
    pub generic_args: Vec<TyId<'db>>,
}

impl<'db> GenericSubst<'db> {
    pub fn empty(db: &'db dyn HirAnalysisDb) -> Self {
        Self::new(db, Vec::new())
    }
}

// SEMANTIC-INSTANCE-KEY IDENTITY INVARIANT (rung 3.2): `ImplEnv` is a field of
// the `#[salsa::interned]` `SemanticInstanceKey`, so its `Hash`/`Eq` form part
// of that key's interning identity (and salsa interns over ALL fields of an
// embedded value — there is no per-field "exclude from identity" attribute).
// `selected_implementor` is pure carry-context: it is the impl typeck's solver
// committed to at instantiation time, recorded so rung 3.3 can assert MIR
// re-resolution agrees. It is functionally determined by the rest of the key
// (the owner func + subst + assumptions already pin which impl is selected), so
// two `ImplEnv`s differing ONLY in `selected_implementor` are observationally
// identical and MUST intern to the same `SemanticInstanceKey`. It is therefore
// excluded from the manual `PartialEq`/`Eq`/`Hash`/`Update` below (hence a plain
// struct with manual impls instead of `#[salsa::interned]`/`#[derive]`, which
// would fold every field into the identity and shatter byte-identity). It is
// likewise NOT serialized into `stable_key.rs` (deferred), so no codegen symbol
// changes either.
#[derive(Debug, Clone)]
pub struct ImplEnv<'db> {
    normalization_scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
    witnesses: Vec<TraitInstId<'db>>,
    /// The impl typeck's solver selected at instantiation time, when this
    /// `ImplEnv` belongs to a resolved trait-method instance. `None` for any
    /// `ImplEnv` not built from a trait-method callable. Pure carry-context —
    /// excluded from identity (see invariant above), never consulted in rung
    /// 3.2; rung 3.3 asserts MIR re-resolution selects the same implementor.
    selected_implementor: Option<ImplementorId<'db>>,
}

impl<'db> PartialEq for ImplEnv<'db> {
    fn eq(&self, other: &Self) -> bool {
        // `selected_implementor` excluded — see IDENTITY INVARIANT above.
        self.normalization_scope == other.normalization_scope
            && self.assumptions == other.assumptions
            && self.witnesses == other.witnesses
    }
}

impl<'db> Eq for ImplEnv<'db> {}

impl<'db> std::hash::Hash for ImplEnv<'db> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // `selected_implementor` excluded — must stay consistent with `PartialEq`.
        self.normalization_scope.hash(state);
        self.assumptions.hash(state);
        self.witnesses.hash(state);
    }
}

unsafe impl<'db> Update for ImplEnv<'db> {
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        let old_value = unsafe { &mut *old_pointer };
        // `selected_implementor` excluded from the change decision (consistent
        // with `Eq`): an `ImplEnv` differing only in the carried implementor is
        // NOT a salsa change. We still refresh the stored value so the
        // carry-context never goes stale, but report "unchanged" so downstream
        // memoized results are not invalidated.
        if old_value.normalization_scope == new_value.normalization_scope
            && old_value.assumptions == new_value.assumptions
            && old_value.witnesses == new_value.witnesses
        {
            old_value.selected_implementor = new_value.selected_implementor;
            false
        } else {
            *old_value = new_value;
            true
        }
    }
}

impl<'db> ImplEnv<'db> {
    pub fn new(
        _db: &'db dyn HirAnalysisDb,
        normalization_scope: ScopeId<'db>,
        assumptions: PredicateListId<'db>,
        witnesses: Vec<TraitInstId<'db>>,
    ) -> Self {
        Self {
            normalization_scope,
            assumptions,
            witnesses,
            selected_implementor: None,
        }
    }

    pub fn empty(db: &'db dyn HirAnalysisDb, normalization_scope: ScopeId<'db>) -> Self {
        Self::new(
            db,
            normalization_scope,
            PredicateListId::empty_list(db),
            Vec::new(),
        )
    }

    /// The lexical scope used to normalize types within this instance.
    pub fn normalization_scope(&self, _db: &'db dyn HirAnalysisDb) -> ScopeId<'db> {
        self.normalization_scope
    }

    /// The `where`-clause / param-env assumptions in force for this instance.
    pub fn assumptions(&self, _db: &'db dyn HirAnalysisDb) -> PredicateListId<'db> {
        self.assumptions
    }

    /// The trait-instance witnesses carried by this instance.
    pub fn witnesses(&self, _db: &'db dyn HirAnalysisDb) -> &Vec<TraitInstId<'db>> {
        &self.witnesses
    }

    /// Records the impl typeck's solver selected at instantiation time. Pure
    /// carry-context: excluded from this `ImplEnv`'s identity, so it does not
    /// affect the `SemanticInstanceKey` it is embedded in (rung 3.2). Rung 3.3
    /// will assert MIR re-resolution selects the same implementor.
    pub fn with_selected_implementor(
        mut self,
        selected_implementor: Option<ImplementorId<'db>>,
    ) -> Self {
        self.selected_implementor = selected_implementor;
        self
    }

    /// The impl typeck committed to at instantiation time, if this `ImplEnv`
    /// belongs to a resolved trait-method instance. Consumed by the rung-3.3 MIR
    /// re-resolution determinism assertion (`classify.rs::resolve_runtime_call_key`).
    pub fn selected_implementor(&self, _db: &'db dyn HirAnalysisDb) -> Option<ImplementorId<'db>> {
        self.selected_implementor
    }
}

#[salsa::interned]
#[derive(Debug)]
pub struct EffectProviderSubst<'db> {
    #[return_ref]
    pub providers: Vec<EffectProviderSpecialization<'db>>,
}

impl<'db> EffectProviderSubst<'db> {
    pub fn empty(db: &'db dyn HirAnalysisDb) -> Self {
        Self::new(db, Vec::new())
    }
}

pub fn instantiate_typed_body<'db>(
    db: &'db dyn HirAnalysisDb,
    template: TypedBodyTemplate<'db>,
    subst: GenericSubst<'db>,
) -> TypedBody<'db> {
    instantiate_with_generic_args(db, template.body, subst.generic_args(db))
}

pub fn instantiate_with_generic_args<'db, T>(
    db: &'db dyn HirAnalysisDb,
    value: T,
    generic_args: &[TyId<'db>],
) -> T
where
    T: TyFoldable<'db>,
{
    let mut folder = GenericInstantiator { generic_args };
    value.fold_with(db, &mut folder)
}

struct GenericInstantiator<'a, 'db> {
    generic_args: &'a [TyId<'db>],
}

impl<'db> TyFolder<'db> for GenericInstantiator<'_, 'db> {
    fn fold_ty(&mut self, db: &'db dyn HirAnalysisDb, ty: TyId<'db>) -> TyId<'db> {
        match ty.data(db) {
            TyData::TyParam(param) => self.generic_args.get(param.idx).copied().unwrap_or(ty),
            TyData::ConstTy(const_ty) => {
                if let ConstTyData::TyParam(param, _) = const_ty.data(db)
                    && let Some(replacement) = self.generic_args.get(param.idx).copied()
                {
                    replacement
                } else {
                    ty.super_fold_with(db, self)
                }
            }
            _ => ty.super_fold_with(db, self),
        }
    }
}
