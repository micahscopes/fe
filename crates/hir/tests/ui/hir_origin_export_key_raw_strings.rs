use fe_hir::origin::{HirExprOrigin, HirStmtOrigin, SemanticOrigin};

fn expr_export_rejects_raw_owner<'db>(origin: HirExprOrigin<'db>) {
    let _ = origin.export_key("body:test");
}

fn stmt_export_rejects_raw_owner<'db>(origin: HirStmtOrigin<'db>) {
    let _ = origin.export_key("body:test");
}

fn semantic_export_rejects_raw_owner<'db>(origin: SemanticOrigin<'db>) {
    let _ = origin.export_key("semantic:test");
}

fn main() {}
