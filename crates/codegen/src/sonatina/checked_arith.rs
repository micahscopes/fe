use sonatina_ir::{builder::FunctionBuilder, func_cursor::InstInserter, ValueId};

#[derive(Clone, Copy)]
pub(super) enum CheckedArithOp {
    Add,
    Sub,
    Mul,
}

/// Insert one signed or unsigned checked integer operation.
///
/// Callers retain operand coercion and decide how the overflow flag fails for
/// their target. Sonatina retains the width-specific overflow semantics.
pub(super) fn insert_checked_arith(
    builder: &mut FunctionBuilder<InstInserter>,
    op: CheckedArithOp,
    signed: bool,
    lhs: ValueId,
    rhs: ValueId,
) -> [ValueId; 2] {
    match (op, signed) {
        (CheckedArithOp::Add, false) => builder.insert_uaddo(lhs, rhs),
        (CheckedArithOp::Sub, false) => builder.insert_usubo(lhs, rhs),
        (CheckedArithOp::Mul, false) => builder.insert_umulo(lhs, rhs),
        (CheckedArithOp::Add, true) => builder.insert_saddo(lhs, rhs),
        (CheckedArithOp::Sub, true) => builder.insert_ssubo(lhs, rhs),
        (CheckedArithOp::Mul, true) => builder.insert_smulo(lhs, rhs),
    }
}
