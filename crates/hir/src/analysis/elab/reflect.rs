use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            fold::{TyFoldable, TyFolder},
            ty_def::{PrimTy, TyBase, TyData, TyId},
            visitor::{TyVisitable, TyVisitor},
        },
    },
    hir_def::{FieldParent, IdentId},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ReflectedField<'db> {
    pub(crate) parent: TyId<'db>,
    pub(crate) index: u32,
    pub(crate) name: IdentId<'db>,
    pub(crate) ty: TyId<'db>,
}

impl<'db> ReflectedField<'db> {
    pub(crate) fn field_ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let field_ctor = TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::Field)));
        TyId::app(db, TyId::app(db, field_ctor, self.parent), self.ty)
    }

    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        format!(
            "{}.{}: {}",
            self.parent.pretty_print(db),
            self.name.data(db),
            self.field_ty(db).pretty_print(db)
        )
    }
}

impl<'db> TyVisitable<'db> for ReflectedField<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.parent.visit_with(visitor);
        self.ty.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ReflectedField<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            parent: self.parent.fold_with(db, folder),
            index: self.index,
            name: self.name,
            ty: self.ty.fold_with(db, folder),
        }
    }
}

pub(super) fn reflect_struct_fields<'db>(
    db: &'db dyn HirAnalysisDb,
    target: TyId<'db>,
) -> Vec<ReflectedField<'db>> {
    let Some(FieldParent::Struct(struct_)) = target.field_parent(db) else {
        return Vec::new();
    };

    let field_tys = target.field_types(db);
    FieldParent::Struct(struct_)
        .fields(db)
        .zip(field_tys)
        .filter_map(|(field, ty)| {
            Some(ReflectedField {
                parent: target,
                index: field.idx as u32,
                name: field.name(db)?,
                ty,
            })
        })
        .collect()
}
