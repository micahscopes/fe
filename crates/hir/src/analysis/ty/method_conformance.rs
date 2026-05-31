use common::indexmap::{IndexMap, IndexSet};

use crate::{
    analysis::HirAnalysisDb,
    hir_def::{Func, IdentId, Trait},
};

pub(crate) fn required_trait_methods<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_: Trait<'db>,
) -> IndexMap<IdentId<'db>, Func<'db>> {
    trait_
        .method_defs(db)
        .into_iter()
        .filter(|(_, method)| method.body(db).is_none())
        .collect()
}

pub(crate) fn missing_required_method_names<'db>(
    db: &'db dyn HirAnalysisDb,
    trait_: Trait<'db>,
    provided: impl IntoIterator<Item = IdentId<'db>>,
) -> Vec<IdentId<'db>> {
    let provided = provided.into_iter().collect::<IndexSet<_>>();
    required_trait_methods(db, trait_)
        .keys()
        .copied()
        .filter(|name| !provided.contains(name))
        .collect()
}
