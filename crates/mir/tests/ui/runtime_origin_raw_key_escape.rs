use fe_mir::{RuntimeStmtOrigin, RuntimeTerminatorOrigin};

fn runtime_stmt_origin_does_not_expose_raw_key<'db>(origin: RuntimeStmtOrigin<'db>) {
    let _ = origin.key();
}

fn runtime_terminator_origin_does_not_expose_raw_key<'db>(
    origin: RuntimeTerminatorOrigin<'db>,
) {
    let _ = origin.key();
}

fn main() {}
