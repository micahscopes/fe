//! Format-v1 projections of retained compiler decisions. No decision is solved
//! again here: unsupported terms must be reported instead of silently omitted.
use hir::{
    analysis::{
        diagnostics::SpannedHirAnalysisDb,
        semantic::{SemConstScalar, SemConstValue},
        ty::{
            const_ty::ConstTyData,
            trait_def::{ImplementorOrigin, TraitInstId},
            ty_check::{TraitChoiceVia, TypedBody},
            ty_def::{PrimTy, TyBase, TyData, TyId},
        },
    },
    span::LazySpan,
};
use serde_json::{Value, json};

/// Encode the body's accepted goal answers, including their authored use spans.
/// The answer's implementation identity comes only from the retained record.
pub fn goal_records<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    body: &TypedBody<'db>,
) -> Result<Vec<Value>, String> {
    body.decisions()
        .choice_entries(db)
        .map(|choice| {
            let span = choice.span().resolve(db).ok_or("goal has no source span")?;
            let offset = usize::from(span.range.start());
            let text = span.file.text(db);
            let prefix = text.get(..offset).ok_or("invalid goal source offset")?;
            let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
            let col = prefix.rsplit('\n').next().unwrap().chars().count() + 1;
            let via = match choice.answer_origin(db) {
                ImplementorOrigin::Assumption => json!("bound"),
                ImplementorOrigin::Hir(impl_) => {
                    if impl_
                        .span()
                        .resolve(db)
                        .is_none_or(|declaration| declaration.file != span.file)
                    {
                        return Err("implementation from another file is outside format v1".into());
                    }
                    match choice.via() {
                        TraitChoiceVia::Implementation(_) => {
                            json!({"impl": impl_.declaration_index(db)})
                        }
                        TraitChoiceVia::Selected(_) => {
                            json!({"selected": impl_.declaration_index(db)})
                        }
                    }
                }
                ImplementorOrigin::VirtualContract(_) => {
                    return Err("virtual contract answer is outside format v1".into());
                }
            };
            Ok(json!({
                "v": 1, "kind": "goal", "site": {"line": line, "col": col},
                "goal": goal(db, choice.goal())?, "via": via,
            }))
        })
        .collect()
}

fn goal<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    instance: TraitInstId<'db>,
) -> Result<Value, String> {
    let name = instance.def(db).name(db).to_opt().ok_or("unnamed trait")?;
    let args = instance.args(db);
    Ok(json!({
        "self": term(db, instance.self_ty(db))?, "trait": name.data(db),
        "args": args.iter().skip(1).map(|ty| term(db, *ty)).collect::<Result<Vec<_>, _>>()?,
    }))
}

fn term<'db>(db: &'db dyn SpannedHirAnalysisDb, ty: TyId<'db>) -> Result<Value, String> {
    match ty.data(db) {
        TyData::TyParam(param) => Ok(json!({"var": param.name.data(db), "kind": "type"})),
        TyData::ConstTy(constant) => match constant.data(db) {
            ConstTyData::TyParam(param, ty) => {
                let kind = if matches!(ty.data(db), TyData::TyBase(TyBase::Prim(PrimTy::Bool))) {
                    "bool"
                } else {
                    "nat"
                };
                Ok(json!({"var": param.name.data(db), "kind": kind}))
            }
            ConstTyData::Value(value) => match value.value().value(db) {
                SemConstValue::Scalar {
                    value: SemConstScalar::Bool(value),
                    ..
                } => Ok(json!({"bool": value})),
                SemConstValue::Scalar {
                    value: SemConstScalar::Int { value },
                    ..
                } => {
                    let value = value
                        .to_string()
                        .parse::<u64>()
                        .map_err(|_| "integer is outside the v1 encoder's natural-number range")?;
                    Ok(json!({"nat": value}))
                }
                _ => Err("aggregate constant is outside format v1".into()),
            },
            _ => Err("unevaluated type argument needs a format-v1 term projection".into()),
        },
        TyData::TyBase(_) | TyData::TyApp(_, _) => {
            let base = ty.base_ty(db);
            if !matches!(base.data(db), TyData::TyBase(_)) {
                return Err("higher-kinded type argument is outside format v1".into());
            }
            Ok(json!({
                "con": base.pretty_print(db),
                "args": ty.generic_args(db).iter().map(|ty| term(db, *ty)).collect::<Result<Vec<_>, _>>()?,
            }))
        }
        _ => Err("type argument is outside format v1".into()),
    }
}
