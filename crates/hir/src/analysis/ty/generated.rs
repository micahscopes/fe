use crate::{
    analysis::{
        HirAnalysisDb,
        elab::{ElaborationCtfeContextId, ProviderOutputId, ReflectedField, RequirementOrigin},
        ty::{
            constraint::{ConstraintId, ConstraintListId},
            fold::{TyFoldable, TyFolder},
            trait_def::TraitInstId,
            ty_def::TyId,
            visitor::{TyVisitable, TyVisitor},
        },
    },
    hir_def::IdentId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum GeneratedImplSource<'db> {
    ProviderOutput(ProviderOutputId<'db>),
}

impl<'db> GeneratedImplSource<'db> {
    pub(crate) fn pretty_print(self, _db: &'db dyn HirAnalysisDb) -> &'static str {
        match self {
            Self::ProviderOutput(_) => "provider",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct GeneratedRequirement<'db> {
    pub(crate) constraint: ConstraintId<'db>,
    pub(crate) origin: RequirementOrigin<'db>,
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct GeneratedRequirementListId<'db> {
    #[return_ref]
    pub(crate) list: Vec<GeneratedRequirement<'db>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct GeneratedStructFieldInit<'db> {
    pub(crate) field: ReflectedField<'db>,
    pub(crate) value: GeneratedExprId<'db>,
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct GeneratedStructFieldInitListId<'db> {
    #[return_ref]
    pub(crate) list: Vec<GeneratedStructFieldInit<'db>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum GeneratedExprKind<'db> {
    BoolLiteral(bool),
    BoolAnd {
        lhs: GeneratedExprId<'db>,
        rhs: GeneratedExprId<'db>,
    },
    SelfRef {
        ty: TyId<'db>,
    },
    MethodArgRef {
        name: IdentId<'db>,
        ty: TyId<'db>,
    },
    FieldGet {
        base: GeneratedExprId<'db>,
        field: ReflectedField<'db>,
    },
    EqExpr {
        lhs: GeneratedExprId<'db>,
        rhs: GeneratedExprId<'db>,
    },
    DefaultCall {
        ty: TyId<'db>,
    },
    StructInit {
        target: TyId<'db>,
        fields: GeneratedStructFieldInitListId<'db>,
    },
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct GeneratedExprId<'db> {
    pub(crate) kind: GeneratedExprKind<'db>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum GeneratedMethodBodyKind<'db> {
    Expr(GeneratedExprId<'db>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct GeneratedMethod<'db> {
    pub(crate) name: IdentId<'db>,
    pub(crate) body: GeneratedMethodBodyKind<'db>,
}

#[salsa::interned]
#[derive(Debug)]
pub(crate) struct GeneratedMethodListId<'db> {
    #[return_ref]
    pub(crate) list: Vec<GeneratedMethod<'db>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) struct GeneratedImplId<'db> {
    pub(crate) context: ElaborationCtfeContextId<'db>,
    pub(crate) trait_inst: TraitInstId<'db>,
    pub(crate) source: GeneratedImplSource<'db>,
    pub(crate) requirements: GeneratedRequirementListId<'db>,
    pub(crate) methods: GeneratedMethodListId<'db>,
    pub(crate) obligations: ConstraintListId<'db>,
}

impl<'db> TyVisitable<'db> for GeneratedImplId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.trait_inst.visit_with(visitor);
        for requirement in self.requirements.list(visitor.db()) {
            requirement.visit_with(visitor);
        }
        self.methods.visit_with(visitor);
        self.obligations.visit_with(visitor);
    }
}

impl<'db> TyVisitable<'db> for GeneratedRequirement<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.constraint.visit_with(visitor);
        self.origin.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for GeneratedRequirement<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            constraint: self.constraint.fold_with(db, folder),
            origin: self.origin.fold_with(db, folder),
        }
    }
}

impl<'db> TyVisitable<'db> for GeneratedRequirementListId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        for requirement in self.list(visitor.db()) {
            requirement.visit_with(visitor);
        }
    }
}

impl<'db> TyFoldable<'db> for GeneratedRequirementListId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(
            db,
            self.list(db)
                .iter()
                .map(|requirement| requirement.fold_with(db, folder))
                .collect::<Vec<_>>(),
        )
    }
}

impl<'db> TyVisitable<'db> for GeneratedStructFieldInit<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.field.visit_with(visitor);
        self.value.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for GeneratedStructFieldInit<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            field: self.field.fold_with(db, folder),
            value: self.value.fold_with(db, folder),
        }
    }
}

impl<'db> TyVisitable<'db> for GeneratedStructFieldInitListId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        for field in self.list(visitor.db()) {
            field.visit_with(visitor);
        }
    }
}

impl<'db> TyFoldable<'db> for GeneratedStructFieldInitListId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(
            db,
            self.list(db)
                .iter()
                .map(|field| field.fold_with(db, folder))
                .collect::<Vec<_>>(),
        )
    }
}

impl<'db> TyVisitable<'db> for GeneratedExprKind<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        match self {
            Self::BoolLiteral(_) => {}
            Self::BoolAnd { lhs, rhs } => {
                lhs.visit_with(visitor);
                rhs.visit_with(visitor);
            }
            Self::SelfRef { ty } => ty.visit_with(visitor),
            Self::MethodArgRef { ty, .. } => ty.visit_with(visitor),
            Self::FieldGet { base, field } => {
                base.visit_with(visitor);
                field.visit_with(visitor);
            }
            Self::EqExpr { lhs, rhs } => {
                lhs.visit_with(visitor);
                rhs.visit_with(visitor);
            }
            Self::DefaultCall { ty } => ty.visit_with(visitor),
            Self::StructInit { target, fields } => {
                target.visit_with(visitor);
                fields.visit_with(visitor);
            }
        }
    }
}

impl<'db> TyFoldable<'db> for GeneratedExprKind<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        match self {
            Self::BoolLiteral(_) => self,
            Self::BoolAnd { lhs, rhs } => Self::BoolAnd {
                lhs: lhs.fold_with(db, folder),
                rhs: rhs.fold_with(db, folder),
            },
            Self::SelfRef { ty } => Self::SelfRef {
                ty: ty.fold_with(db, folder),
            },
            Self::MethodArgRef { name, ty } => Self::MethodArgRef {
                name,
                ty: ty.fold_with(db, folder),
            },
            Self::FieldGet { base, field } => Self::FieldGet {
                base: base.fold_with(db, folder),
                field: field.fold_with(db, folder),
            },
            Self::EqExpr { lhs, rhs } => Self::EqExpr {
                lhs: lhs.fold_with(db, folder),
                rhs: rhs.fold_with(db, folder),
            },
            Self::DefaultCall { ty } => Self::DefaultCall {
                ty: ty.fold_with(db, folder),
            },
            Self::StructInit { target, fields } => Self::StructInit {
                target: target.fold_with(db, folder),
                fields: fields.fold_with(db, folder),
            },
        }
    }
}

impl<'db> TyVisitable<'db> for GeneratedExprId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.kind(visitor.db()).visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for GeneratedExprId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(db, self.kind(db).fold_with(db, folder))
    }
}

impl<'db> TyVisitable<'db> for GeneratedMethodBodyKind<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        match self {
            Self::Expr(expr) => expr.visit_with(visitor),
        }
    }
}

impl<'db> TyFoldable<'db> for GeneratedMethodBodyKind<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        match self {
            Self::Expr(expr) => Self::Expr(expr.fold_with(db, folder)),
        }
    }
}

impl<'db> TyVisitable<'db> for GeneratedMethod<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        self.body.visit_with(visitor);
    }
}

impl<'db> TyFoldable<'db> for GeneratedMethod<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            name: self.name,
            body: self.body.fold_with(db, folder),
        }
    }
}

impl<'db> TyVisitable<'db> for GeneratedMethodListId<'db> {
    fn visit_with<V>(&self, visitor: &mut V)
    where
        V: TyVisitor<'db> + ?Sized,
    {
        for method in self.list(visitor.db()) {
            method.visit_with(visitor);
        }
    }
}

impl<'db> TyFoldable<'db> for GeneratedMethodListId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self::new(
            db,
            self.list(db)
                .iter()
                .map(|method| method.fold_with(db, folder))
                .collect::<Vec<_>>(),
        )
    }
}

impl<'db> TyFoldable<'db> for GeneratedImplId<'db> {
    fn super_fold_with<F>(self, db: &'db dyn HirAnalysisDb, folder: &mut F) -> Self
    where
        F: TyFolder<'db>,
    {
        Self {
            context: self.context,
            trait_inst: self.trait_inst.fold_with(db, folder),
            source: self.source,
            requirements: self.requirements.fold_with(db, folder),
            methods: self.methods.fold_with(db, folder),
            obligations: self.obligations.fold_with(db, folder),
        }
    }
}
