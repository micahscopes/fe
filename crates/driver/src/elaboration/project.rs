//! One explicit input phase followed by one complete output phase.
//! Generated declarations are not visible while providers are evaluated.

use std::collections::{BTreeSet, HashSet};

use common::InputDb;
use hir::{
    analysis::{
        diagnostics::DiagnosticVoucher,
        semantic::{EvalOutcome, GenericSubst, SemConstScalar, eval_body_owner_const_with_args},
        ty::{
            normalize::normalize_ty,
            trait_resolution::PredicateListId,
            ty_check::{BodyOwner, check_func_body},
            ty_def::{TyBase, TyData, TyId},
        },
    },
    core::meta::{FrozenGeneration, FrozenMetaValue, FrozenOutputShape, FrozenOwner, MetaItem},
    hir_def::{CallableDef, HirIngot, ItemKind, WhereClauseOwner},
    span::LazySpan,
};
use url::Url;

use super::{RecordInput, invocations};
use crate::DriverDataBase;

#[derive(Debug)]
pub struct ElaborationError {
    pub url: Box<Url>,
    pub message: String,
    /// Byte range in the authored file, usually the `#[generate]` attribute.
    pub span: Option<std::ops::Range<usize>>,
}

impl ElaborationError {
    pub fn position(&self, db: &DriverDataBase) -> Option<(usize, usize)> {
        let source = db.workspace().get(db, &self.url)?.text(db);
        let prefix = source.get(..self.span.as_ref()?.start)?;
        let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
        let column = prefix.rsplit('\n').next()?.chars().count() + 1;
        Some((line, column))
    }

    pub fn render(&self, db: &DriverDataBase) -> String {
        match self.position(db) {
            Some((line, column)) => {
                format!("{}:{line}:{column}: {}", self.url, self.message)
            }
            None => format!("{}: {}", self.url, self.message),
        }
    }
}

fn extent(db: &DriverDataBase, ty: TyId<'_>) -> Option<usize> {
    let TyData::ConstTy(value) = ty.data(db) else {
        return None;
    };
    usize::try_from(value.integer_value(db)?).ok()
}

/// Bound the declared result before evaluation can allocate its arrays.
#[derive(Clone, Copy)]
enum OutputShape {
    Function,
    Functions,
    Implementation,
}

fn output_shape<'db>(input: &RecordInput<'db>, ty: TyId<'db>) -> Result<OutputShape, String> {
    let db = input.db;
    let missing = || "generation result has an unsupported shape".to_owned();
    let definition = input
        .meta_type(MetaItem::FunctionDefinition)
        .ok_or_else(missing)?;
    let implementation = input
        .meta_type(MetaItem::RecordImplementation)
        .ok_or_else(missing)?;
    let optional = input.meta_type(MetaItem::Option).ok_or_else(missing)?;
    let (definition_ty, count, batch) = if ty.is_array(db) {
        let count = ty.array_len(db).ok_or_else(missing)?;
        if count > 64 {
            return Err(format!("generated functions limit exceeded: {count} > 64"));
        }
        let element = *ty.generic_args(db).first().ok_or_else(missing)?;
        if element.base_ty(db) != optional.base_ty(db) {
            return Err(missing());
        }
        (
            *element.generic_args(db).first().ok_or_else(missing)?,
            count,
            true,
        )
    } else {
        (ty, 1, false)
    };
    let (base, args) = definition_ty.decompose_ty_app(db);
    let is_implementation = !batch && base == implementation.base_ty(db);
    if (!is_implementation && base != definition.base_ty(db)) || args.len() != 2 {
        return Err(missing());
    }
    let parameters = extent(db, args[0]).ok_or_else(missing)?;
    let expressions = extent(db, args[1]).ok_or_else(missing)?;
    if parameters > 8 {
        return Err(format!(
            "function parameters limit exceeded: {parameters} > 8"
        ));
    }
    if expressions == 0 {
        return Err("generated expression body must not be empty".into());
    }
    if expressions > 512 {
        return Err(format!(
            "body expressions limit exceeded: {expressions} > 512"
        ));
    }
    let allocated = count.saturating_mul(parameters + expressions + 7);
    if allocated > 4096 {
        return Err(format!(
            "declared output slots limit exceeded: {allocated} > 4096"
        ));
    }
    Ok(if is_implementation {
        OutputShape::Implementation
    } else if batch {
        OutputShape::Functions
    } else {
        OutputShape::Function
    })
}

/// One structural budget for the frozen provider result. Schema names and
/// pretty-printer formatting do not affect whether a declaration is admitted.
/// Each value node costs eight units, plus its scalar payload bytes.
fn description_units(value: &FrozenMetaValue) -> usize {
    let payload = match value {
        FrozenMetaValue::Unit | FrozenMetaValue::Structural { .. } => 0,
        FrozenMetaValue::Scalar { value, .. } => match value {
            SemConstScalar::Bool(_) => 1,
            SemConstScalar::Int { value } => value.to_signed_bytes_le().len(),
            SemConstScalar::Bytes(bytes) => bytes.len(),
        },
        FrozenMetaValue::Tuple(values) | FrozenMetaValue::Array(values) => {
            values.iter().fold(0usize, |sum, value| {
                sum.saturating_add(description_units(value))
            })
        }
        FrozenMetaValue::Record { fields, .. } => fields.values().fold(0usize, |sum, value| {
            sum.saturating_add(description_units(value))
        }),
        FrozenMetaValue::Variant { fields, .. } => fields.iter().fold(0usize, |sum, value| {
            sum.saturating_add(description_units(value))
        }),
    };
    8usize.saturating_add(payload)
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
        url: Box::new(target.clone()),
        message: "could not resolve generation target".into(),
        span: None,
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
    let mut check_urls = BTreeSet::new();
    let mut total_units = 0usize;
    let mut frozen_requests = Vec::new();
    for (url, file) in db.workspace().all_files(db).iter() {
        // This is only a negative lexical filter; all matching source goes
        // through the parser and ordinary lexical invocation resolution.
        if !url.path().ends_with(".fe") || !file.text(db).contains("generate") {
            continue;
        }
        if selected.is_some_and(|selected| !selected.contains(&db.top_mod(file).ingot(db))) {
            continue;
        }
        let error_at = |message: String, span| ElaborationError {
            url: Box::new(url.clone()),
            message,
            span,
        };
        let requests = invocations(db, file).map_err(|e| {
            let span = match e.item {
                ItemKind::Struct(owner) => owner
                    .span()
                    .attributes()
                    .attr(e.attribute_index)
                    .resolve(db),
                item => item.span().resolve(db),
            }
            .map(|span| span.range.into());
            error_at(format!("invalid generation invocation: {:?}", e.kind), span)
        })?;
        for request in requests {
            let site = request
                .owner
                .span()
                .attributes()
                .attr(request.attribute_index)
                .resolve(db)
                .map(|span| span.range.into());
            let error = |message: String| error_at(message, site.clone());
            let input = request
                .record_input(db)
                .map_err(|e| error(format!("invalid generation input: {e:?}")))?;
            let TyData::TyBase(TyBase::Func(CallableDef::Func(provider))) =
                request.provider.data(db)
            else {
                return Err(error("generation requires a const function".into()));
            };
            let parameters: Vec<_> = provider.params(db).collect();
            let record_ty = MetaItem::Record
                .resolve(db, provider.scope())
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
            let shape = output_shape(&input, normalize(provider.return_ty(db))).map_err(error)?;
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
                EvalOutcome::Blocked(info) => {
                    return Err(error(format!(
                        "generation evaluation remained dependent: {info:?}"
                    )));
                }
                EvalOutcome::Failed(failure) => {
                    return Err(error(format!("generation evaluation failed: {failure:?}")));
                }
            };
            // Freeze the checked value before the output HIR query reads it.
            let frozen = input
                .freeze_value(value)
                .map_err(|e| error(format!("invalid generated declaration: {e}")))?;
            let units = description_units(&frozen);
            if units > 65536 {
                return Err(error(format!(
                    "invalid generated declaration: generated description units limit exceeded: {units} > 65536"
                )));
            }
            total_units = total_units.saturating_add(units);
            if total_units > 1048576 {
                return Err(error(format!(
                    "project generated description units limit exceeded: {total_units} > 1048576"
                )));
            }
            let provider_identity = FrozenOwner {
                file: provider
                    .span()
                    .resolve(db)
                    .and_then(|span| span.file.url(db))
                    .ok_or_else(|| error("generation provider file is missing".into()))?,
                path: provider
                    .scope()
                    .pretty_path(db)
                    .ok_or_else(|| error("generation provider path is missing".into()))?,
            };
            let frozen_shape = match shape {
                OutputShape::Function => FrozenOutputShape::Function,
                OutputShape::Functions => FrozenOutputShape::Functions,
                OutputShape::Implementation => FrozenOutputShape::Implementation,
            };
            let span = request
                .owner
                .span()
                .resolve(db)
                .ok_or_else(|| error("generation owner has no source span".into()))?;
            let range: std::ops::Range<usize> = span.range.into();
            frozen_requests.push(FrozenGeneration {
                target: input
                    .frozen_owner()
                    .map_err(|e| error(format!("invalid generation owner: {e}")))?,
                target_span_start: range.start,
                site_span: site,
                provider: provider_identity,
                attribute_index: request.attribute_index,
                shape: frozen_shape,
                value: frozen,
            });
            check_urls.insert(url.clone());
            if let Some(provider_url) = provider.span().resolve(db).and_then(|s| s.file.url(db)) {
                check_urls.insert(provider_url);
            }
        }
    }
    if frozen_requests.is_empty() {
        return Ok(None);
    }
    let mut output = db.fork_inputs();
    output.publish_generation(frozen_requests);
    let mut checked = HashSet::new();
    for url in check_urls {
        let file = output
            .workspace()
            .get(&output, &url)
            .expect("copied source file");
        if let Some(error) = hir::lower::generated_hir_errors(&output, output.top_mod(file)).first()
        {
            return Err(ElaborationError {
                url: Box::new(url),
                message: format!("invalid generated declaration: {}", error.message),
                span: error.site_span.clone(),
            });
        }
        if checked.insert(output.top_mod(file).ingot(&output))
            && let Some(message) = crate::checked_source::diagnostics(&output, file)
        {
            return Err(ElaborationError {
                url: Box::new(url),
                message,
                span: None,
            });
        }
    }
    Ok(Some(output))
}
