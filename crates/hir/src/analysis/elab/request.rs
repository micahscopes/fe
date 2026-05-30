use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{adt_def::AdtRef, constraint::ConstraintId, ty_def::TyId},
    },
    hir_def::{DeriveDecl, Enum, IdentId, ItemKind, Struct},
    span::DynLazySpan,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ElaborationTarget<'db> {
    Struct(Struct<'db>),
    Enum(Enum<'db>),
}

impl<'db> ElaborationTarget<'db> {
    pub(super) fn from_item(item: ItemKind<'db>) -> Option<Self> {
        match item {
            ItemKind::Struct(struct_) => Some(Self::Struct(struct_)),
            ItemKind::Enum(enum_) => Some(Self::Enum(enum_)),
            _ => None,
        }
    }

    pub(super) fn from_adt_ref(adt: AdtRef<'db>) -> Self {
        match adt {
            AdtRef::Struct(struct_) => Self::Struct(struct_),
            AdtRef::Enum(enum_) => Self::Enum(enum_),
        }
    }

    pub(super) fn item(self) -> ItemKind<'db> {
        match self {
            Self::Struct(struct_) => ItemKind::Struct(struct_),
            Self::Enum(enum_) => ItemKind::Enum(enum_),
        }
    }

    pub(super) fn scope(self) -> crate::hir_def::scope_graph::ScopeId<'db> {
        self.item().scope()
    }

    pub(super) fn attrs(
        self,
        db: &'db dyn HirAnalysisDb,
    ) -> Option<crate::hir_def::AttrListId<'db>> {
        self.item().attrs(db)
    }

    pub(super) fn attr_span(self) -> DynLazySpan<'db> {
        match self {
            Self::Struct(struct_) => struct_.span().attributes().into(),
            Self::Enum(enum_) => enum_.span().attributes().into(),
        }
    }

    pub(super) fn ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let adt = match self {
            Self::Struct(struct_) => struct_.as_adt(db),
            Self::Enum(enum_) => enum_.as_adt(db),
        };
        let mut ty = TyId::adt(db, adt);
        for &param in adt.params(db) {
            ty = TyId::app(db, ty, param);
        }
        ty
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ElaborationOrigin<'db> {
    DeriveAttr { attr_index: u32, arg_index: u32 },
    DeriveDecl(DeriveDecl<'db>),
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct ElaborationRequestId<'db> {
    pub(super) target: ElaborationTarget<'db>,
    pub(super) goal: ConstraintId<'db>,
    pub(super) selected_provider: Option<IdentId<'db>>,
    pub(super) origin: ElaborationOrigin<'db>,
}

impl<'db> ElaborationRequestId<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        let mut summary = format!(
            "{} requested for {}",
            self.goal(db).pretty_print(db),
            self.target(db).ty(db).pretty_print(db)
        );
        if let Some(provider) = self.selected_provider(db) {
            summary.push_str(" using ");
            summary.push_str(provider.data(db));
        }
        summary
    }

    pub(super) fn span(self, db: &'db dyn HirAnalysisDb) -> DynLazySpan<'db> {
        match self.origin(db) {
            ElaborationOrigin::DeriveAttr { .. } => self.target(db).attr_span(),
            ElaborationOrigin::DeriveDecl(decl) => decl.span().into(),
        }
    }
}

impl<'db> ElaborationOrigin<'db> {
    pub(super) fn pretty_print(self) -> &'static str {
        match self {
            Self::DeriveAttr { .. } => "derive attribute",
            Self::DeriveDecl(_) => "derive declaration",
        }
    }
}
