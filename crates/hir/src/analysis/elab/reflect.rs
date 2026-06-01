use crate::{
    analysis::{
        HirAnalysisDb,
        ty::{
            fold::{TyFoldable, TyFolder},
            ty_def::{PrimTy, TyBase, TyData, TyId},
            visitor::{TyVisitable, TyVisitor},
        },
    },
    hir_def::{EnumVariant, FieldParent, IdentId, VariantKind},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum ReflectedFieldOrigin<'db> {
    FieldParent(FieldParent<'db>),
    TupleVariant(EnumVariant<'db>),
}

impl<'db> ReflectedFieldOrigin<'db> {
    pub(crate) fn pretty_parent(self, db: &'db dyn HirAnalysisDb) -> String {
        match self {
            Self::FieldParent(parent) => parent
                .name(db)
                .map(|name| name.into_owned())
                .unwrap_or_else(|| "<unknown field parent>".to_string()),
            Self::TupleVariant(variant) => {
                let enum_name = variant
                    .enum_
                    .name(db)
                    .to_opt()
                    .map(|name| name.data(db))
                    .map_or("<unknown enum>", |name| name);
                let variant_name = variant.name(db).unwrap_or("<unknown variant>");
                format!("{enum_name}::{variant_name}")
            }
        }
    }

    pub(crate) fn field_span(self, index: usize) -> crate::span::DynLazySpan<'db> {
        match self {
            Self::FieldParent(parent) => parent.field_name_span(index),
            Self::TupleVariant(variant) => variant
                .enum_
                .variant_span(variant.idx as usize)
                .tuple_type()
                .elem_ty(index)
                .into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ReflectedField<'db> {
    pub(crate) parent: TyId<'db>,
    pub(crate) index: u32,
    pub(crate) name: IdentId<'db>,
    pub(crate) ty: TyId<'db>,
    pub(crate) origin: ReflectedFieldOrigin<'db>,
}

impl<'db> ReflectedField<'db> {
    pub(crate) fn field_ty(self, db: &'db dyn HirAnalysisDb) -> TyId<'db> {
        let field_ctor = TyId::new(db, TyData::TyBase(TyBase::Prim(PrimTy::Field)));
        TyId::app(db, TyId::app(db, field_ctor, self.parent), self.ty)
    }

    pub(crate) fn pretty_parent(self, db: &'db dyn HirAnalysisDb) -> String {
        match self.origin {
            ReflectedFieldOrigin::FieldParent(FieldParent::Struct(_)) => {
                self.parent.pretty_print(db).to_string()
            }
            ReflectedFieldOrigin::FieldParent(FieldParent::Contract(_))
            | ReflectedFieldOrigin::FieldParent(FieldParent::Variant(_))
            | ReflectedFieldOrigin::TupleVariant(_) => self.origin.pretty_parent(db),
        }
    }

    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        format!(
            "{}.{}: {}",
            self.pretty_parent(db),
            self.name.data(db),
            self.field_ty(db).pretty_print(db)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct ReflectedVariant<'db> {
    pub(crate) parent: TyId<'db>,
    pub(crate) index: u32,
    pub(crate) name: IdentId<'db>,
    pub(crate) variant: EnumVariant<'db>,
}

impl<'db> ReflectedVariant<'db> {
    pub(crate) fn pretty_print(self, db: &'db dyn HirAnalysisDb) -> String {
        format!("{}::{}", self.parent.pretty_print(db), self.name.data(db))
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
            origin: self.origin,
        }
    }
}

impl<'db> TyVisitable<'db> for ReflectedVariant<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.parent.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for ReflectedVariant<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            parent: self.parent.fold_with(db, folder),
            index: self.index,
            name: self.name,
            variant: self.variant,
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
            let source = FieldParent::Struct(struct_);
            Some(ReflectedField {
                parent: target,
                index: field.idx as u32,
                name: field.name(db)?,
                ty,
                origin: ReflectedFieldOrigin::FieldParent(source),
            })
        })
        .collect()
}

pub(super) fn reflect_enum_variants<'db>(
    db: &'db dyn HirAnalysisDb,
    target: TyId<'db>,
) -> Vec<ReflectedVariant<'db>> {
    let Some(crate::analysis::ty::adt_def::AdtRef::Enum(enum_)) = target.adt_ref(db) else {
        return Vec::new();
    };

    enum_
        .variants(db)
        .enumerate()
        .filter_map(|(index, variant)| {
            Some(ReflectedVariant {
                parent: target,
                index: index as u32,
                name: variant.name(db)?,
                variant: EnumVariant::new(enum_, index),
            })
        })
        .collect()
}

pub(super) fn reflect_variant_fields<'db>(
    db: &'db dyn HirAnalysisDb,
    reflected_variant: ReflectedVariant<'db>,
) -> Vec<ReflectedField<'db>> {
    let target = reflected_variant.parent;
    let Some(adt_def) = target.adt_def(db) else {
        return Vec::new();
    };
    let args = target.generic_args(db);
    let variant = reflected_variant.variant;
    let variant_idx = reflected_variant.index as usize;

    match variant.kind(db) {
        VariantKind::Unit => Vec::new(),
        VariantKind::Record(fields) => fields
            .data(db)
            .iter()
            .enumerate()
            .filter_map(|(field_idx, field)| {
                Some(ReflectedField {
                    parent: target,
                    index: field_idx as u32,
                    name: field.name.to_opt()?,
                    ty: crate::analysis::ty::ty_def::instantiate_adt_field_ty(
                        db,
                        adt_def,
                        variant_idx,
                        field_idx,
                        args,
                    ),
                    origin: ReflectedFieldOrigin::FieldParent(FieldParent::Variant(variant)),
                })
            })
            .collect(),
        VariantKind::Tuple(tuple) => (0..tuple.len(db))
            .map(|field_idx| ReflectedField {
                parent: target,
                index: field_idx as u32,
                name: IdentId::new(db, field_idx.to_string()),
                ty: crate::analysis::ty::ty_def::instantiate_adt_field_ty(
                    db,
                    adt_def,
                    variant_idx,
                    field_idx,
                    args,
                ),
                origin: ReflectedFieldOrigin::TupleVariant(variant),
            })
            .collect(),
    }
}
