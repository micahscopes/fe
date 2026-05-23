use fe_hir::origin::{HirExprOrigin, HirStmtOrigin, SemanticOrigin};

fn hir_expr_origin_does_not_expose_raw_key<'db>(origin: HirExprOrigin<'db>) {
    let _ = origin.key();
}

fn hir_stmt_origin_does_not_expose_raw_key<'db>(origin: HirStmtOrigin<'db>) {
    let _ = origin.key();
}

fn semantic_origin_does_not_expose_raw_key<'db>(origin: SemanticOrigin<'db>) {
    let _ = origin.key();
}

fn main() {}
