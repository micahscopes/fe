//! Rendering of bounded authored scalar programs. Ordinary output checking owns
//! operation typing and arithmetic semantics; this boundary checks references,
//! nominal protocol identity and resource limits.
use super::{
    FunctionTemplate, GenerationError, GenerationErrorKind, imports,
    value::{self, ValueType},
};
use crate::DriverDataBase;
use common::stdlib::HasBuiltinCore;
use hir::analysis::{
    semantic::{SemConstId, SemConstScalar, SemConstValue},
    ty::{
        const_ty::ConstTyData,
        corelib::resolve_lib_type_path,
        ty_def::{TyData, TyId},
    },
};
use hir::hir_def::HirIngot;

const MAX_NODES: usize = 64;
fn error(message: &str) -> GenerationError {
    GenerationError::new(GenerationErrorKind::Protocol, message)
}
fn length(db: &DriverDataBase, ty: TyId<'_>) -> Result<usize, GenerationError> {
    let TyData::ConstTy(value) = ty.data(db) else {
        return Err(error("scalar body requires a concrete node count"));
    };
    let ConstTyData::Value(value) = value.data(db) else {
        return Err(error("scalar body requires a concrete node count"));
    };
    let SemConstValue::Scalar {
        value: SemConstScalar::Int { value: n },
        ..
    } = value.value().value(db).clone()
    else {
        return Err(error("scalar body requires a concrete node count"));
    };
    let n = usize::try_from(n).map_err(|_| error("scalar body node count is out of range"))?;
    if !(1..=MAX_NODES).contains(&n) {
        return Err(GenerationError::new(
            GenerationErrorKind::Limit,
            "scalar body requires one through 64 nodes",
        ));
    }
    Ok(n)
}
pub(super) fn result_type(db: &DriverDataBase, ty: TyId<'_>) -> Result<ValueType, GenerationError> {
    let designated = resolve_lib_type_path(
        db,
        db.builtin_core().root_mod(db).scope(),
        "core::meta::ScalarBody",
    )
    .ok_or_else(|| error("builtin ScalarBody is missing"))?;
    let (base, args) = ty.decompose_ty_app(db);
    if base != designated.base_ty(db) || args.len() != 2 {
        return Err(error("body must use builtin ScalarBody<T, N>"));
    }
    length(db, args[1])?;
    let result = value::read_type(db, args[0])?;
    if !matches!(result, ValueType::Bool | ValueType::U256) {
        return Err(error("scalar body result must be bool or u256"));
    }
    Ok(result)
}
fn index(db: &DriverDataBase, value: SemConstId<'_>) -> Result<usize, GenerationError> {
    match value.value(db).clone() {
        SemConstValue::Scalar {
            ty,
            value: SemConstScalar::Int { value },
        } if ty == TyId::u256(db) => {
            usize::try_from(value).map_err(|_| error("scalar body index is out of range"))
        }
        _ => Err(error("scalar body index must be a concrete u256")),
    }
}
pub(super) fn render(
    db: &DriverDataBase,
    body: SemConstId<'_>,
    template: &FunctionTemplate,
    max_bytes: usize,
) -> Result<String, GenerationError> {
    let SemConstValue::Struct { ty, fields } = body.value(db).clone() else {
        return Err(error("scalar body must be a concrete descriptor"));
    };
    result_type(db, ty)?;
    if fields.len() != 2 {
        return Err(error("scalar body descriptor has invalid fields"));
    }
    let n = length(db, ty.generic_args(db)[1])?;
    let SemConstValue::Array { elems: nodes, .. } = fields[0].value(db).clone() else {
        return Err(error("scalar body nodes must be a fixed array"));
    };
    if nodes.len() != n {
        return Err(error("scalar body node count mismatch"));
    }
    let result = index(db, fields[1])?;
    if result >= n {
        return Err(error("scalar body result index is out of range"));
    }
    let names = imports::template_param_names(template)
        .map_err(|e| GenerationError::new(GenerationErrorKind::Template, e.message))?;
    let mut prefix = "__scalar_node_".to_owned();
    while names.iter().flatten().any(|name| name.starts_with(&prefix)) {
        prefix.push('_');
    }
    let designated = resolve_lib_type_path(
        db,
        db.builtin_core().root_mod(db).scope(),
        "core::meta::ScalarOp",
    )
    .ok_or_else(|| error("builtin ScalarOp is missing"))?;
    let mut output = "{\n".to_owned();
    for (i, node) in nodes.iter().enumerate() {
        let SemConstValue::Enum {
            ty,
            variant,
            fields,
        } = node.value(db).clone()
        else {
            return Err(error("scalar body node must be a concrete operation"));
        };
        if ty != designated {
            return Err(error("scalar body operation has the wrong nominal type"));
        }
        let reference = |value| -> Result<String, GenerationError> {
            let j = index(db, value)?;
            if j >= i {
                return Err(error("scalar body references must point to earlier nodes"));
            }
            Ok(format!("{prefix}{j}"))
        };
        // Variant identity comes from the designated core enum. The source
        // compiler, not the renderer, decides whether operands type-check.
        let (annotation, expression) = match (variant.0, fields.as_ref()) {
            (0, [v]) => (": u256", value::expression(db, *v, &ValueType::U256, 128)?),
            (1, [v]) => (": bool", value::expression(db, *v, &ValueType::Bool, 16)?),
            (2 | 3, [v]) => {
                let name = names
                    .get(index(db, *v)?)
                    .and_then(|n| n.as_ref())
                    .ok_or_else(|| error("scalar body parameter has no usable name or index"))?;
                (
                    if variant.0 == 2 { ": u256" } else { ": bool" },
                    name.clone(),
                )
            }
            (4..=6, [a, b]) => (
                if variant.0 == 4 { ": u256" } else { ": bool" },
                format!(
                    "{} {} {}",
                    reference(*a)?,
                    match variant.0 {
                        4 => "+",
                        5 => "<",
                        _ => "==",
                    },
                    reference(*b)?
                ),
            ),
            (7, [v]) => (": bool", format!("!{}", reference(*v)?)),
            _ => return Err(error("scalar body operation has invalid fields")),
        };
        let line = format!("let {prefix}{i}{annotation} = {expression}\n");
        if output
            .len()
            .checked_add(line.len())
            .is_none_or(|n| n > max_bytes)
        {
            return Err(GenerationError::new(
                GenerationErrorKind::Limit,
                "generated source byte limit exceeded",
            ));
        }
        output.push_str(&line);
    }
    let tail = format!("{prefix}{result}\n}}");
    if output
        .len()
        .checked_add(tail.len())
        .is_none_or(|n| n > max_bytes)
    {
        return Err(GenerationError::new(
            GenerationErrorKind::Limit,
            "generated source byte limit exceeded",
        ));
    }
    output.push_str(&tail);
    Ok(output)
}
