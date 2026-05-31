use common::indexmap::IndexMap;

use crate::analysis::{
    HirAnalysisDb,
    ty::{
        binder::Binder,
        generated::GeneratedImplId,
        trait_def::{ImplementorId, ImplementorOrigin, does_impl_trait_conflict},
        trait_lower::collect_trait_impls,
    },
};

pub(super) fn generated_conflicts_with_authored_impl<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
) -> bool {
    let request = generated.context.request(db);
    let ingot = request.target(db).item().top_mod(db).ingot(db);
    let Some(authored_impls) = collect_trait_impls(db, ingot).get(&generated.trait_inst.def(db))
    else {
        return false;
    };
    let generated_impl = Binder::bind(ImplementorId::new(
        db,
        generated.trait_inst,
        generated.trait_inst.self_ty(db).generic_args(db).to_vec(),
        IndexMap::new(),
        ImplementorOrigin::Generated(generated),
    ));
    authored_impls
        .iter()
        .any(|&authored| does_impl_trait_conflict(db, authored, generated_impl))
}

pub(super) fn generated_conflicts_with_generated_impl<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
    candidates: &[GeneratedImplId<'db>],
) -> bool {
    let generated_impl = Binder::bind(ImplementorId::new(
        db,
        generated.trait_inst,
        generated.trait_inst.self_ty(db).generic_args(db).to_vec(),
        IndexMap::new(),
        ImplementorOrigin::Generated(generated),
    ));
    candidates.iter().copied().any(|other| {
        if other == generated {
            return false;
        }
        let other_impl = Binder::bind(ImplementorId::new(
            db,
            other.trait_inst,
            other.trait_inst.self_ty(db).generic_args(db).to_vec(),
            IndexMap::new(),
            ImplementorOrigin::Generated(other),
        ));
        does_impl_trait_conflict(db, other_impl, generated_impl)
    })
}
