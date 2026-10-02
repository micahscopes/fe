//! One explicit input phase followed by one complete output phase.
//! Generated declarations are not visible while providers are evaluated.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use common::InputDb;
use hir::{
    analysis::{
        diagnostics::DiagnosticVoucher,
        semantic::{
            EvalOutcome, GenericSubst, SemConstScalar, SemConstValue,
            eval_body_owner_const_with_args,
        },
        ty::{
            const_ty::ConstTyData,
            corelib::resolve_lib_type_path,
            normalize::normalize_ty,
            trait_resolution::PredicateListId,
            ty_check::{BodyOwner, check_func_body},
            ty_def::{TyBase, TyData, TyId},
        },
    },
    hir_def::{CallableDef, HirIngot, WhereClauseOwner},
    span::LazySpan,
};
use url::Url;

use super::{RecordInput, construction, invocations};
use crate::DriverDataBase;

#[derive(Debug)]
pub struct ElaborationError {
    pub url: Url,
    pub message: String,
}

fn extent(db: &DriverDataBase, ty: TyId<'_>) -> Option<usize> {
    let TyData::ConstTy(value) = ty.data(db) else {
        return None;
    };
    let ConstTyData::Value(value) = value.data(db) else {
        return None;
    };
    let SemConstValue::Scalar {
        value: SemConstScalar::Int { value },
        ..
    } = value.value().value(db)
    else {
        return None;
    };
    usize::try_from(value).ok()
}

/// Bound the declared result before evaluation can allocate its arrays.
enum OutputShape {
    Function,
    Functions,
    Implementation,
}

fn output_shape<'db>(input: &RecordInput<'db>, ty: TyId<'db>) -> Option<OutputShape> {
    let db = input.db;
    let definition =
        resolve_lib_type_path(db, input.owner.scope(), "core::meta::FunctionDefinition")?;
    let implementation =
        resolve_lib_type_path(db, input.owner.scope(), "core::meta::RecordImplementation")?;
    let optional = resolve_lib_type_path(db, input.owner.scope(), "core::Option")?;
    let (definition_ty, count, batch) = if ty.is_array(db) {
        let count = ty.array_len(db)?;
        if count > 64 {
            return None;
        }
        let element = *ty.generic_args(db).first()?;
        if element.base_ty(db) != optional.base_ty(db) {
            return None;
        }
        (*element.generic_args(db).first()?, count, true)
    } else {
        (ty, 1, false)
    };
    let (base, args) = definition_ty.decompose_ty_app(db);
    let is_implementation = !batch && base == implementation.base_ty(db);
    if (!is_implementation && base != definition.base_ty(db)) || args.len() != 2 {
        return None;
    }
    let parameters = extent(db, args[0])?;
    let expressions = extent(db, args[1])?;
    if parameters > 8
        || !(1..=512).contains(&expressions)
        || count.saturating_mul(parameters + expressions + 7) > 4096
    {
        return None;
    }
    Some(if is_implementation {
        OutputShape::Implementation
    } else if batch {
        OutputShape::Functions
    } else {
        OutputShape::Function
    })
}

/// Prepare all explicit invocations in the resolved input set. The authored
/// database remains unchanged, including on failure. None avoids copying when
/// there are no invocations. Resolve semantic identities again in the result.
pub fn prepare(db: &DriverDataBase) -> Result<Option<DriverDataBase>, ElaborationError> {
    prepare_selected(db, None)
}

/// Prepare a selected file or ingot and its dependency closure. Other loaded
/// workspace members stay authored and their providers are not evaluated.
pub fn prepare_target(
    db: &DriverDataBase,
    target: &Url,
) -> Result<Option<DriverDataBase>, ElaborationError> {
    let root = if let Some(file) = db.workspace().get(db, target) {
        Some(db.top_mod(file).ingot(db))
    } else {
        db.workspace().containing_ingot(db, target.clone())
    }
    .ok_or_else(|| ElaborationError {
        url: target.clone(),
        message: "could not resolve generation target".into(),
    })?;
    let mut selected = HashSet::new();
    let mut pending = vec![root];
    while let Some(ingot) = pending.pop() {
        if selected.insert(ingot) {
            pending.extend(
                ingot
                    .resolved_external_ingots(db)
                    .iter()
                    .map(|(_, dependency)| *dependency),
            );
        }
    }
    prepare_selected(db, Some(&selected))
}

fn prepare_selected<'db>(
    db: &'db DriverDataBase,
    selected: Option<&HashSet<common::ingot::Ingot<'db>>>,
) -> Result<Option<DriverDataBase>, ElaborationError> {
    let mut edits = BTreeMap::<Url, Vec<(usize, String)>>::new();
    let mut check_urls = BTreeSet::new();
    let mut total_bytes = 0usize;
    for (url, file) in db.workspace().all_files(db).iter() {
        // This is only a negative lexical filter; all matching source goes
        // through the parser and ordinary lexical invocation resolution.
        if !url.path().ends_with(".fe") || !file.text(db).contains("generate") {
            continue;
        }
        if selected.is_some_and(|selected| !selected.contains(&db.top_mod(file).ingot(db))) {
            continue;
        }
        let error = |message: String| ElaborationError {
            url: url.clone(),
            message,
        };
        let requests = invocations(db, file)
            .map_err(|e| error(format!("invalid generation invocation: {:?}", e.kind)))?;
        for request in requests {
            let input = request
                .record_input(db)
                .map_err(|e| error(format!("invalid generation input: {e:?}")))?;
            let TyData::TyBase(TyBase::Func(CallableDef::Func(provider))) =
                request.provider.data(db)
            else {
                return Err(error("generation requires a const function".into()));
            };
            let parameters: Vec<_> = provider.params(db).collect();
            let record_ty = resolve_lib_type_path(db, provider.scope(), "core::meta::Record")
                .ok_or_else(|| error("builtin Record is missing".into()))?;
            let normalize =
                |ty| normalize_ty(db, ty, provider.scope(), PredicateListId::empty_list(db));
            let argument_ty = parameters.first().map(|parameter| {
                let ty = normalize(parameter.ty(db));
                ty.as_view(db).unwrap_or(ty)
            });
            let clause = WhereClauseOwner::Func(*provider).clause(db).id;
            if !provider.is_const(db)
                || provider.is_unsafe(db)
                || provider.is_method(db)
                || provider.has_effects(db)
                || !clause.predicates(db).is_empty()
                || !clause.const_predicates(db).is_empty()
                || !provider
                    .as_callable(db)
                    .is_some_and(|c| c.params(db).is_empty())
                || parameters.len() != 1
                || argument_ty != Some(record_ty)
            {
                return Err(error("generation entry must be a safe nongeneric module const function taking Record, without entry requirements or effects; use ordinary checked helper calls and scoped witnesses inside it".into()));
            }
            let shape = output_shape(&input, normalize(provider.return_ty(db)))
                .ok_or_else(|| error("generation result has an unsupported shape or exceeds its declared allocation limits".into()))?;
            let diagnostics = &check_func_body(db, *provider).0;
            if !diagnostics.is_empty() {
                let complete = diagnostics
                    .iter()
                    .map(|d| d.to_complete(db))
                    .collect::<Vec<_>>();
                return Err(error(db.format_complete_diagnostics(&complete)));
            }
            let argument = input
                .const_argument()
                .ok_or_else(|| error("could not issue record input".into()))?;
            let value = match eval_body_owner_const_with_args(
                db,
                BodyOwner::Func(*provider),
                GenericSubst::for_body_owner(db, BodyOwner::Func(*provider), vec![]),
                vec![argument],
            ) {
                EvalOutcome::Ready(value) => value,
                outcome => return Err(error(format!("generation evaluation failed: {outcome:?}"))),
            };
            let text = match shape {
                OutputShape::Functions => construction::render_functions(&input, value),
                OutputShape::Function => construction::render_function(&input, value),
                OutputShape::Implementation => construction::render_implementation(&input, value),
            }
            .map_err(|e| error(format!("invalid generated declaration: {e:?}")))?;
            total_bytes = total_bytes.saturating_add(text.len());
            if total_bytes > 1024 * 1024 {
                return Err(error("project generated-output limit exceeded".into()));
            }
            let span = request
                .owner
                .span()
                .resolve(db)
                .ok_or_else(|| error("generation owner has no source span".into()))?;
            let range: std::ops::Range<usize> = span.range.into();
            edits
                .entry(url.clone())
                .or_default()
                .push((range.end, text));
            check_urls.insert(url.clone());
            if let Some(provider_url) = provider.span().resolve(db).and_then(|s| s.file.url(db)) {
                check_urls.insert(provider_url);
            }
        }
    }
    if edits.is_empty() {
        return Ok(None);
    }
    let mut output = db.fork_inputs();
    for (url, mut insertions) in edits {
        let file = db.workspace().get(db, &url).expect("collected source file");
        let mut source = file.text(db).clone();
        insertions.sort_by_key(|(offset, _)| std::cmp::Reverse(*offset));
        for (offset, text) in insertions {
            source.insert_str(offset, &format!("\n{text}"));
        }
        output.workspace().update(&mut output, url, source);
    }
    let mut checked = HashSet::new();
    for url in check_urls {
        let file = output
            .workspace()
            .get(&output, &url)
            .expect("copied source file");
        if checked.insert(output.top_mod(file).ingot(&output))
            && let Some(message) = crate::checked_source::diagnostics(&output, file)
        {
            return Err(ElaborationError { url, message });
        }
    }
    Ok(Some(output))
}
