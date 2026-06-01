use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            fold::{TyFoldable, TyFolder},
            visitor::{TyVisitable, TyVisitor},
        },
    },
    span::DynLazySpan,
};

use super::ReflectedField;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum RequirementOrigin<'db> {
    ReflectedField(ReflectedField<'db>),
    ProviderCode,
    #[cfg(test)]
    Synthetic,
}

impl<'db> TyVisitable<'db> for RequirementOrigin<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        match self {
            Self::ReflectedField(field) => field.visit_with(visitor),
            Self::ProviderCode => {}
            #[cfg(test)]
            Self::Synthetic => {}
        }
    }
}

impl<'db> TyFoldable<'db> for RequirementOrigin<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        match self {
            Self::ReflectedField(field) => Self::ReflectedField(field.fold_with(db, folder)),
            Self::ProviderCode => self,
            #[cfg(test)]
            Self::Synthetic => self,
        }
    }
}

impl<'db> RequirementOrigin<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        match self {
            RequirementOrigin::ReflectedField(field) => {
                format!(
                    "from field {}.{}",
                    field.parent.pretty_print(db),
                    field.name.data(db)
                )
            }
            RequirementOrigin::ProviderCode => "from provider code".to_string(),
            #[cfg(test)]
            RequirementOrigin::Synthetic => "from synthetic requirement".to_string(),
        }
    }

    pub(crate) fn diagnostic_span(self, db: &'db dyn HirAnalysisDb) -> Option<DynLazySpan<'db>> {
        match self {
            RequirementOrigin::ReflectedField(field) => field
                .parent
                .field_parent(db)
                .map(|parent| parent.field_name_span(field.index as usize)),
            RequirementOrigin::ProviderCode => None,
            #[cfg(test)]
            RequirementOrigin::Synthetic => None,
        }
    }
}
