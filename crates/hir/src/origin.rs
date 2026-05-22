use common::{
    diagnostics::Span,
    origin::{OriginExportKey, OriginExportKind, OriginExportOwnerKey},
};
use cranelift_entity::EntityRef;
use salsa::Update;

use crate::{
    SpannedHirDb,
    analysis::{
        HirAnalysisDb,
        diagnostics::SpannedHirAnalysisDb,
        semantic::{SemOrigin, SemanticInstanceKey},
    },
    hir_def::{Body, ExprId, StmtId},
    span::{DynLazySpan, LazySpan, expr::LazyExprSpan, stmt::LazyStmtSpan},
};

common::define_origin_owner_key! {
    pub struct HirOriginBodyOwnerKey;
}

common::define_origin_owner_key! {
    pub struct SemanticOriginInstanceOwnerKey;
}

pub trait SemanticOriginOwnerKey: OriginExportOwnerKey {}

impl SemanticOriginOwnerKey for SemanticOriginInstanceOwnerKey {}

common::define_origin_key_type! {
    /// Origin key for a HIR expression. The expression ID is only meaningful inside
    /// its owning HIR body.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
    pub struct HirExprOrigin<'db> {
        owner: Body<'db> => body,
        local: ExprId => expr
    }
}

impl<'db> HirExprOrigin<'db> {
    pub fn export_key(self, stable_body_key: &HirOriginBodyOwnerKey) -> OriginExportKey {
        OriginExportKey::new(
            OriginExportKind::HirExpr,
            stable_body_key.as_str(),
            self.expr().index().to_string(),
        )
    }

    pub fn lazy_span(self) -> LazyExprSpan<'db> {
        self.expr().span(self.body())
    }

    pub fn resolve_source_span(self, db: &dyn SpannedHirDb) -> Option<Span> {
        self.lazy_span().resolve(db)
    }
}

common::define_origin_key_type! {
    /// Origin key for a HIR statement. Statement and expression IDs are intentionally
    /// distinct origin types even though both are body-local integers.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
    pub struct HirStmtOrigin<'db> {
        owner: Body<'db> => body,
        local: StmtId => stmt
    }
}

impl<'db> HirStmtOrigin<'db> {
    pub fn export_key(self, stable_body_key: &HirOriginBodyOwnerKey) -> OriginExportKey {
        OriginExportKey::new(
            OriginExportKind::HirStmt,
            stable_body_key.as_str(),
            self.stmt().index().to_string(),
        )
    }

    pub fn lazy_span(self) -> LazyStmtSpan<'db> {
        self.stmt().span(self.body())
    }

    pub fn resolve_source_span(self, db: &dyn SpannedHirDb) -> Option<Span> {
        self.lazy_span().resolve(db)
    }
}

/// Typed HIR origin node for consumers that need to carry expression and
/// statement origins in one collection without erasing their kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
pub enum HirOriginNode<'db> {
    Expr(HirExprOrigin<'db>),
    Stmt(HirStmtOrigin<'db>),
    Semantic(SemanticOrigin<'db>),
}

common::define_origin_graph_type! {
    pub struct HirOriginGraph<'db>(HirOriginNode<'db>);
}

common::define_origin_key_type! {
    /// Origin key for semantic IR nodes. `SemOrigin` can contain body-local expr or
    /// stmt IDs, so it is only meaningful inside a semantic instance.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Update)]
    pub struct SemanticOrigin<'db> {
        owner: SemanticInstanceKey<'db> => instance,
        local: SemOrigin<'db> => origin
    }
}

impl<'db> SemanticOrigin<'db> {
    pub fn export_key<K: SemanticOriginOwnerKey + ?Sized>(
        self,
        stable_instance_key: &K,
    ) -> OriginExportKey {
        OriginExportKey::new(
            OriginExportKind::Semantic,
            stable_instance_key.as_str(),
            sem_origin_local_key(self.origin()),
        )
    }

    pub fn export_local_key(self) -> String {
        sem_origin_local_key(self.origin())
    }

    pub fn lazy_span(self, db: &'db dyn HirAnalysisDb) -> DynLazySpan<'db> {
        match self.origin() {
            SemOrigin::Expr(expr) => self
                .instance()
                .owner(db)
                .body(db)
                .map(|body| expr.span(body).into())
                .unwrap_or_else(DynLazySpan::invalid),
            SemOrigin::Stmt(stmt) => self
                .instance()
                .owner(db)
                .body(db)
                .map(|body| stmt.span(body).into())
                .unwrap_or_else(DynLazySpan::invalid),
            SemOrigin::Body(owner) => owner
                .body(db)
                .map(|body| body.span().into())
                .unwrap_or_else(DynLazySpan::invalid),
            SemOrigin::Synthetic => DynLazySpan::invalid(),
        }
    }

    pub fn resolve_source_span(self, db: &'db dyn SpannedHirAnalysisDb) -> Option<Span> {
        self.lazy_span(db).resolve(db)
    }
}

fn sem_origin_local_key(origin: SemOrigin<'_>) -> String {
    match origin {
        SemOrigin::Expr(expr) => format!("expr:{}", expr.index()),
        SemOrigin::Stmt(stmt) => format!("stmt:{}", stmt.index()),
        SemOrigin::Body(_) => "body".to_string(),
        SemOrigin::Synthetic => "synthetic".to_string(),
    }
}
