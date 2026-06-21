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

// SEMANTIC-INSTANCE-KEY IDENTITY INVARIANT (rung 3.2 + cascade C3d): `ImplEnv`
// is a field of the `#[salsa::interned]` `SemanticInstanceKey`, so its
// `Hash`/`Eq` form part of that key's interning identity (and salsa interns over
// ALL fields of an embedded value — there is no per-field "exclude from
// identity" attribute). `selected_implementor` is the impl typeck's solver
// committed to at instantiation time, recorded so the MIR C1 rail consumes it as
// the resolution source and rung 3.3 can assert MIR re-resolution agrees.
//
// SOME-ONLY CASCADE EXCEPTION: `selected_implementor` folds into identity ONLY
// when `Some`. A `None` env hashes/compares/updates BYTE-IDENTICALLY to the
// pre-cascade behavior (where it was excluded entirely), so every instance that
// did not scope-select an impl — which today is every instance, since
// `const_ref.rs` carries `None` for non-scope-selected calls — interns exactly
// as before. A `Some(impl)` DOES fold the impl into identity: this is what makes
// the cascade observable, because a `with (<T as Trait>)` scoped call carries
// `Some(override)` while the same call outside carries `None`, and those two
// instances MUST mint distinct `SemanticInstanceKey`s (and distinct codegen
// symbols — see the matching Some-only discriminator in `stable_key.rs`) so they
// lower against different impls. Without folding `Some` in, the override and the
// default would collide on one key and one symbol (a miscompile). A `None` env
// remains observationally identical to one differing only by a `None`
// `selected_implementor` — that is the byte-identity floor the manual
// `PartialEq`/`Eq`/`Hash`/`Update` below preserve (hence a plain struct with
// manual impls instead of `#[salsa::interned]`/`#[derive]`, which would always
// fold the field). `stable_key.rs` carries the SAME Some-only discriminator so
// the codegen-symbol identity tracks the interning identity exactly.
#[derive(Debug, Clone)]
pub struct ImplEnv<'db> {
    normalization_scope: ScopeId<'db>,
    assumptions: PredicateListId<'db>,
    witnesses: Vec<TraitInstId<'db>>,
    /// The impl typeck's solver selected at instantiation time, when this
    /// `ImplEnv` belongs to a resolved trait-method instance whose call was
    /// SCOPE-SELECTED (`with (<T as Trait>)`). `None` for any call that was not
    /// scope-selected (today every non-cascade call). Folds into identity ONLY
    /// when `Some` (the Some-only cascade exception — see invariant above): a
    /// `None` env is byte-identical to the pre-cascade behavior; a `Some(impl)`
    /// makes the scope selection observable by minting a distinct key/symbol.
    /// Consumed by the MIR C1 rail (`classify.rs`) as the resolution source.
    selected_implementor: Option<ImplementorId<'db>>,
}

impl<'db> PartialEq for ImplEnv<'db> {
    fn eq(&self, other: &Self) -> bool {
        // `selected_implementor` folds into equality ONLY when `Some` — see the
        // SOME-ONLY CASCADE EXCEPTION in the IDENTITY INVARIANT above. Direct
        // `Option` equality gives exactly that: `None == None` (the byte-identity
        // floor — two non-scope-selected envs are equal), `Some(a) == Some(b)`
        // iff the SAME impl, and `Some != None` (a scope-selected override is a
        // distinct instance from the unscoped default).
        self.normalization_scope == other.normalization_scope
            && self.assumptions == other.assumptions
            && self.witnesses == other.witnesses
            && self.selected_implementor == other.selected_implementor
    }
}

impl<'db> Eq for ImplEnv<'db> {}

impl<'db> std::hash::Hash for ImplEnv<'db> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.normalization_scope.hash(state);
        self.assumptions.hash(state);
        self.witnesses.hash(state);
        // SOME-ONLY: hash `selected_implementor` ONLY when `Some`, so a `None`
        // env hashes BYTE-IDENTICALLY to the pre-cascade behavior (where the
        // field was excluded entirely). A `Some(impl)` mixes the impl in so it
        // lands on a distinct bucket from the `None` default. Consistent with
        // `PartialEq` (`None == None` skip-vs-skip; equal `Some`s hash equally).
        if let Some(implementor) = self.selected_implementor {
            implementor.hash(state);
        }
    }
}

unsafe impl<'db> Update for ImplEnv<'db> {
    unsafe fn maybe_update(old_pointer: *mut Self, new_value: Self) -> bool {
        let old_value = unsafe { &mut *old_pointer };
        // SOME-ONLY (consistent with `Eq`): an `ImplEnv` whose
        // `selected_implementor` is unchanged under the Some-only rule is NOT a
        // salsa change. `Option` equality encodes that exactly — `None`→`None`
        // is not a change (byte-identity floor), but `None`→`Some(impl)` or a
        // change of impl IS a real change (the cascade selection became
        // observable / shifted). On no-change we still refresh the stored value
        // (it is already equal, so this is a no-op write) and report "unchanged"
        // so downstream memoized results are not invalidated.
        if old_value.normalization_scope == new_value.normalization_scope
            && old_value.assumptions == new_value.assumptions
            && old_value.witnesses == new_value.witnesses
            && old_value.selected_implementor == new_value.selected_implementor
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
