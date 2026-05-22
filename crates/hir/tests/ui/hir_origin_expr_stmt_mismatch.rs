use fe_hir::origin::{HirExprOrigin, HirStmtOrigin};

fn expects_expr<'db>(_: HirExprOrigin<'db>) {}

fn expects_stmt<'db>(_: HirStmtOrigin<'db>) {}

fn stmt_origin_is_not_an_expr_origin<'db>(origin: HirStmtOrigin<'db>) {
    expects_expr(origin);
}

fn expr_origin_is_not_a_stmt_origin<'db>(origin: HirExprOrigin<'db>) {
    expects_stmt(origin);
}

fn main() {}
