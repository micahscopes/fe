use crate::analysis::{
    HirAnalysisDb,
    ty::{
        generated::GeneratedImplId,
        trait_def::{does_impl_trait_conflict, generated_implementor_candidate},
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
    let generated_impl = generated_implementor_candidate(db, generated);
    authored_impls
        .iter()
        .any(|&authored| does_impl_trait_conflict(db, authored, generated_impl))
}

pub(super) fn generated_conflicting_generated_impl<'db>(
    db: &'db dyn HirAnalysisDb,
    generated: GeneratedImplId<'db>,
    candidates: &[GeneratedImplId<'db>],
) -> Option<GeneratedImplId<'db>> {
    let generated_impl = generated_implementor_candidate(db, generated);
    candidates.iter().copied().find(|&other| {
        if other == generated {
            return false;
        }
        let other_impl = generated_implementor_candidate(db, other);
        does_impl_trait_conflict(db, other_impl, generated_impl)
    })
}
