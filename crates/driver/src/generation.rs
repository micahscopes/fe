//! Authored providers and cumulative immutable output admission.
//! This bounded adapter supports explicit providers on concrete root records.
use crate::DriverDataBase;
use common::{InputDb, file::File, ingot::IngotKind};
use hir::{
    analysis::{
        diagnostics::DiagnosticVoucher,
        semantic::{CtfeError, EvalFailure, EvalOutcome, SemConstValue, SemOrigin},
        ty::{
            corelib::resolve_lib_type_path,
            ty_check::{BodyOwner, check_func_body, checked_body_demands},
            ty_def::TyId,
        },
    },
    core::meta::RecordInput,
    hir_def::CallableDef,
    span::LazySpan,
};
use parser::ast::{self, prelude::*};
use salsa::Setter;

#[derive(Clone, Debug)]
pub struct GenerationError {
    pub file: url::Url,
    pub range: parser::TextRange,
    pub cause: String,
}

impl std::fmt::Display for GenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}..{}: {}",
            self.file,
            u32::from(self.range.start()),
            u32::from(self.range.end()),
            self.cause
        )
    }
}

fn diagnostic_error(
    db: &DriverDataBase,
    diagnostics: &[common::diagnostics::CompleteDiagnostic],
    mut fallback: GenerationError,
) -> GenerationError {
    if let Some(span) = diagnostics
        .iter()
        .find_map(|diagnostic| diagnostic.primary_span())
    {
        fallback.file = span.file.url(db).unwrap_or(fallback.file);
        fallback.range = span.range;
    }
    // Labels carry expected/given types even when a source transition lacks a
    // printable span. Do not let terminal rendering erase the original cause.
    let details = diagnostics
        .iter()
        .flat_map(|diagnostic| {
            std::iter::once(diagnostic.message.clone())
                .chain(
                    diagnostic
                        .sub_diagnostics
                        .iter()
                        .map(|label| label.message.clone()),
                )
                .chain(diagnostic.notes.iter().cloned())
        })
        .filter(|message| !message.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    fallback.cause = format!(
        "{}: {}\n{}",
        fallback.cause,
        details,
        db.format_complete_diagnostics(diagnostics)
    );
    fallback
}

fn body_errors(
    db: &DriverDataBase,
    function: hir::hir_def::Func<'_>,
) -> Vec<common::diagnostics::CompleteDiagnostic> {
    check_func_body(db, function)
        .0
        .iter()
        .map(|diagnostic| diagnostic.to_complete(db))
        .filter(|diagnostic| diagnostic.severity == common::diagnostics::Severity::Error)
        .collect()
}

fn evaluation_error<'db>(
    db: &'db DriverDataBase,
    mut owner: BodyOwner<'db>,
    error: &CtfeError<'db>,
    fallback: GenerationError,
) -> GenerationError {
    let mut error = error;
    while let CtfeError::CalleeError { callee, source, .. } = error {
        owner = callee.key(db).owner(db);
        error = source;
    }
    if let BodyOwner::Func(function, _) = owner {
        let complete = body_errors(db, function);
        if !complete.is_empty() {
            return diagnostic_error(
                db,
                &complete,
                GenerationError {
                    cause: "authored const helper diagnostics".into(),
                    ..fallback
                },
            );
        }
    }
    let (origin, cause) = match error {
        CtfeError::DivisionByZero { origin } => {
            (*origin, "division by zero in const context".to_string())
        }
        CtfeError::ArithmeticOverflow { origin } => {
            (*origin, "arithmetic overflow in const context".to_string())
        }
        CtfeError::InvalidOperation { origin, message } => (*origin, message.clone()),
        CtfeError::OutOfBounds { origin } => {
            (*origin, "constant index outside record or aggregate".into())
        }
        CtfeError::NonConstCall { origin } => (
            *origin,
            "cannot call non-const function in const context".to_string(),
        ),
        CtfeError::InvalidBorrow { origin } => (
            *origin,
            "invalid borrow in const evaluation context".to_string(),
        ),
        CtfeError::InvalidProviderUse { origin } => (
            *origin,
            "invalid effect provider use in const context".to_string(),
        ),
        CtfeError::InvalidBody { origin } => (
            *origin,
            "invalid body in const evaluation context".to_string(),
        ),
        CtfeError::NotConstEvaluable { origin } => (
            *origin,
            "expression cannot be evaluated at compile time".to_string(),
        ),
        CtfeError::RecursiveConst { origin } => {
            (*origin, "recursive const evaluation detected".to_string())
        }
        CtfeError::StepLimitExceeded { origin } => {
            (*origin, "existing CTFE step limit exceeded".into())
        }
        CtfeError::RecursionLimitExceeded { origin } => {
            (*origin, "existing CTFE recursion limit exceeded".into())
        }
        _ => return fallback,
    };
    let Some(body) = owner.body(db) else {
        return fallback;
    };
    let SemOrigin::Expr(expr) = origin else {
        return fallback;
    };
    let Some(span) = expr.span(body).resolve(db) else {
        return fallback;
    };
    GenerationError {
        file: span.file.url(db).unwrap_or(fallback.file),
        range: span.range,
        cause,
    }
}

/// The authored requests of one source file, frozen against the authored view.
struct FileRequests {
    requests: Vec<common::generation::FrozenGenerationRequest>,
    has_scoped_opening: bool,
    root_range: parser::TextRange,
}

/// Nothing is published when any input request fails. The returned database
/// owns the unchanged source text and the complete frozen output plan.
pub fn prepare_file(
    db: &DriverDataBase,
    file: File,
) -> Result<Option<DriverDataBase>, GenerationError> {
    let url = file.url(db).expect("indexed generation input");
    let error = |range, cause| GenerationError {
        file: url.clone(),
        range,
        cause,
    };
    if !db.generation_inputs().functions(db).is_empty()
        || !db.generation_inputs().requests(db).is_empty()
    {
        return Err(error(
            parser::TextRange::empty(0.into()),
            "generation requires the authored input view; output is checked once".into(),
        ));
    }
    let Some(found) = file_requests(db, file, true)? else {
        return Ok(None);
    };
    if found.requests.is_empty() && !found.has_scoped_opening {
        return Err(error(
            parser::TextRange::empty(0.into()),
            "generation request requires an authored concrete root record".into(),
        ));
    }
    admit_requests(
        db,
        &ViewBasis::Standalone(file),
        &[url],
        found.requests,
        Some(found.root_range),
    )
    .map(Some)
}

/// Prepare every authored request of the given ingots and of the ingots they
/// depend on, in one immutable view of the whole program. The authored
/// database is unchanged, also on failure. `None` means there is nothing to
/// generate, so consumers keep using the authored view.
///
/// Requests are evaluated against authored input exactly as for a standalone
/// file; scoped openings use the same dependency frontier. Output diagnostics
/// stay in the returned view, where each consumer reports them with their own
/// spans, instead of being collapsed into one preparation error.
pub fn prepare_ingots(
    db: &DriverDataBase,
    roots: &[url::Url],
) -> Result<Option<DriverDataBase>, GenerationError> {
    use hir::hir_def::HirIngot;
    let Some(first) = roots.first() else {
        return Ok(None);
    };
    if !db.generation_inputs().functions(db).is_empty()
        || !db.generation_inputs().requests(db).is_empty()
    {
        return Err(GenerationError {
            file: first.clone(),
            range: parser::TextRange::empty(0.into()),
            cause: "generation requires the authored input view; output is checked once".into(),
        });
    }
    // The selected ingots and their resolved dependency closure. The builtin
    // libraries carry no generation requests; standalone files are prepared
    // one at a time by `prepare_file`.
    let mut pending = roots
        .iter()
        .filter_map(|root| db.workspace().containing_ingot(db, root.clone()))
        .collect::<Vec<_>>();
    let mut seen = std::collections::HashSet::new();
    let mut files = std::collections::BTreeMap::new();
    while let Some(ingot) = pending.pop() {
        if matches!(
            ingot.kind(db),
            IngotKind::Core | IngotKind::Std | IngotKind::StandAlone
        ) || !seen.insert(ingot.base(db))
        {
            continue;
        }
        for (url, file) in ingot.files(db).iter() {
            if matches!(file.kind(db), Some(common::file::IngotFileKind::Source)) {
                files.insert(url, file);
            }
        }
        pending.extend(
            ingot
                .resolved_external_ingots(db)
                .iter()
                .map(|(_, dependency)| *dependency),
        );
    }
    let mut requests = Vec::new();
    let mut has_scoped_opening = false;
    for file in files.values() {
        if let Some(found) = file_requests(db, *file, false)? {
            requests.extend(found.requests);
            has_scoped_opening |= found.has_scoped_opening;
        }
    }
    if requests.is_empty() && !has_scoped_opening {
        return Ok(None);
    }
    let modules = files.into_keys().collect::<Vec<_>>();
    admit_requests(db, &ViewBasis::Package, &modules, requests, None).map(Some)
}

/// Replace an authored database with the prepared view of `roots` when they
/// request generation, so every later consumer reads the generated program.
pub fn prepare_ingots_in_place(
    db: &mut DriverDataBase,
    roots: &[url::Url],
) -> Result<(), GenerationError> {
    if let Some(prepared) = prepare_ingots(db, roots)? {
        *db = prepared;
    }
    Ok(())
}

/// Collect and evaluate the explicit requests of one authored file. `None`
/// means the file contains no generation syntax outside quotations.
fn file_requests(
    db: &DriverDataBase,
    file: File,
    standalone_only: bool,
) -> Result<Option<FileRequests>, GenerationError> {
    let url = file.url(db).expect("indexed generation input");
    let error = |range, cause| GenerationError {
        file: url.clone(),
        range,
        cause,
    };
    // Every request form spells `generate` or `with`; other files need no parse.
    let text = file.text(db);
    if !text.contains("generate") && !text.contains("with") {
        return Ok(None);
    }
    let (green, parse_errors) = parser::parse_source_file(text, parser::RecoveryMode::Recover);
    let root = ast::Root::cast(parser::SyntaxNode::new_root(green)).unwrap();
    let has_scoped_syntax = root
        .syntax()
        .descendants()
        .any(|node| ast::WithExpr::cast(node).is_some());
    // Ordinary lexical implementation choices do not require preparation or
    // the standalone generation adapter. Only actual provider demands do.
    let has_scoped_opening = has_scoped_syntax
        && parse_errors.is_empty()
        && !hir::analysis::ty::trait_resolution::generation::preparation_demands(
            db,
            db.top_mod(file),
        )
        .is_empty();
    if !has_scoped_opening
        && !root
            .syntax()
            .descendants()
            .filter_map(ast::ImplTrait::cast)
            .any(|request| {
                request.derive_provider().is_some()
                    && !request
                        .syntax()
                        .ancestors()
                        .any(|node| ast::QuoteExpr::cast(node).is_some())
            })
        && !root
            .syntax()
            .descendants()
            .filter_map(ast::NormalAttr::cast)
            .any(|attr| {
                attr.is_named("generate")
                    && !attr
                        .syntax()
                        .ancestors()
                        .any(|node| ast::QuoteExpr::cast(node).is_some())
            })
    {
        return Ok(None);
    }
    if !parse_errors.is_empty() {
        return Err(error(
            parser::TextRange::empty(0.into()),
            format!("authored parse errors: {parse_errors:?}"),
        ));
    }
    if standalone_only
        && file
            .containing_ingot(db)
            .is_none_or(|ingot| ingot.kind(db) != IngotKind::StandAlone)
    {
        return Err(error(
            parser::TextRange::empty(0.into()),
            "bounded generation currently requires a standalone source file".into(),
        ));
    }
    let module = db.top_mod(file);
    let mut requests = Vec::new();
    let mut consumed = std::collections::HashSet::new();
    for item in root.items().into_iter().flatten() {
        let attributes = match item.kind() {
            Some(ast::ItemKind::Struct(record)) => record.attr_list(),
            Some(ast::ItemKind::Enum(enum_)) => enum_.attr_list(),
            _ => continue,
        };
        let Some(attributes) = attributes else {
            continue;
        };
        for attribute in attributes.normal_attrs_named("generate") {
            let range = attribute.syntax().text_range();
            consumed.insert(range);
            let args = attribute
                .args()
                .map(|args| args.into_iter().collect::<Vec<_>>())
                .unwrap_or_default();
            if args.len() != 1 || args[0].value().is_some() {
                return Err(error(
                    range,
                    "use #[generate(provider_path)] with one ordinary const function".into(),
                ));
            }
            let path = args[0]
                .key()
                .ok_or_else(|| error(range, "generation provider must be an item path".into()))?;
            let segments = path.segments().map(|segment| {
                if segment.generic_args().is_some() || segment.qualified_type().is_some() {
                    return Err(error(segment.syntax().text_range(),
                        "generation provider path does not support generic or qualified segments".into()));
                }
                segment.ident().map(|ident| ident.text().to_string()).ok_or_else(|| {
                    error(segment.syntax().text_range(), "generation provider path requires identifier segments".into())
                })
            }).collect::<Result<Vec<_>, _>>()?;
            let segments = segments.iter().map(String::as_str).collect::<Vec<_>>();
            let resolved = hir::analysis::name_resolution::resolve_path(
                db,
                hir::hir_def::PathId::from_segments(db, &segments),
                module.scope(),
                hir::analysis::ty::trait_resolution::PredicateListId::empty_list(db),
                true,
            )
            .map_err(|_| error(range, "cannot resolve authored generation provider".into()))?;
            let hir::analysis::name_resolution::PathRes::Func(provider_ty) = resolved else {
                return Err(error(
                    range,
                    "generation provider must resolve to an ordinary function".into(),
                ));
            };
            let hir::analysis::ty::ty_def::TyData::TyBase(hir::analysis::ty::ty_def::TyBase::Func(
                CallableDef::Func(provider),
            )) = provider_ty.data(db)
            else {
                return Err(error(
                    range,
                    "generation provider must resolve to an ordinary function".into(),
                ));
            };
            let provider = *provider;
            if !hir::analysis::name_resolution::is_scope_visible_from(
                db,
                provider.scope(),
                module.scope(),
            ) {
                return Err(error(
                    range,
                    "generation provider is not visible from the request".into(),
                ));
            }
            let input = RecordInput::for_authored_item(db, module, &item).map_err(|cause| {
                error(
                    range,
                    format!("cannot issue concrete authored record: {cause:?}"),
                )
            })?;
            let (target_item, target_span) = match input.declaration() {
                hir::analysis::ty::adt_def::AdtRef::Struct(record) => {
                    (hir::hir_def::ItemKind::Struct(record), record.span().into())
                }
                hir::analysis::ty::adt_def::AdtRef::Enum(enumeration) => (
                    hir::hir_def::ItemKind::Enum(enumeration),
                    enumeration.span().into(),
                ),
            };
            if !input.ty().has_param(db) {
                hir::analysis::ty::ty_check::admit_ground_type_application(
                    db,
                    input.ty(),
                    target_item,
                    target_span,
                )
                .map_err(|failure| {
                    let diagnostics = failure
                        .vouchers()
                        .iter()
                        .map(|voucher| voucher.to_complete(db))
                        .collect::<Vec<_>>();
                    diagnostic_error(
                        db,
                        &diagnostics,
                        error(
                            range,
                            format!(
                                "generation target admission failed: {}",
                                failure.description()
                            ),
                        ),
                    )
                })?;
            }
            requests.push(evaluate_provider(db, file, provider, input, range)?);
        }
    }
    for request_ast in root.syntax().descendants().filter_map(ast::ImplTrait::cast) {
        let Some(path) = request_ast.derive_provider() else {
            continue;
        };
        let range = request_ast.syntax().text_range();
        if request_ast
            .syntax()
            .ancestors()
            .any(|node| ast::QuoteExpr::cast(node).is_some())
        {
            continue;
        }
        // A header in any module's item list is an ordinary module member, so
        // it sees that module's private names. Block-local headers are a
        // pending interface, never ignored.
        if request_ast
            .syntax()
            .ancestors()
            .any(|node| ast::BlockExpr::cast(node).is_some())
        {
            return Err(error(
                range,
                "native derive inside a function body is not supported yet; declare the header in a module".into(),
            ));
        }
        let request = hir::core::meta::authored_derive_request(db, module, &request_ast)
            .ok_or_else(|| {
                error(
                    range,
                    "cannot recover authored derive header identity".into(),
                )
            })?;
        let segments = path.segments().map(|segment| {
            if segment.generic_args().is_some() || segment.qualified_type().is_some() {
                return Err(error(segment.syntax().text_range(), "bounded native derive provider path does not support generic or qualified segments".into()));
            }
            segment.ident().map(|token| token.text().to_string())
                .ok_or_else(|| error(segment.syntax().text_range(), "invalid native derive provider path segment".into()))
        }).collect::<Result<Vec<_>, _>>()?;
        let segments = segments.iter().map(String::as_str).collect::<Vec<_>>();
        let resolved = hir::analysis::name_resolution::resolve_path(
            db,
            hir::hir_def::PathId::from_segments(db, &segments),
            request.scope(),
            hir::analysis::ty::trait_resolution::PredicateListId::empty_list(db),
            true,
        )
        .map_err(|_| {
            error(
                path.syntax().text_range(),
                "cannot resolve native derive provider type".into(),
            )
        })?;
        if let hir::analysis::name_resolution::PathRes::TyAlias(alias, _) = &resolved
            && !hir::analysis::name_resolution::is_scope_visible_from(
                db,
                alias.alias.scope(),
                request.scope(),
            )
        {
            return Err(error(
                path.syntax().text_range(),
                "native derive provider alias is not visible".into(),
            ));
        }
        let (hir::analysis::name_resolution::PathRes::Ty(provider_ty)
        | hir::analysis::name_resolution::PathRes::TyAlias(_, provider_ty)) = resolved
        else {
            return Err(error(
                path.syntax().text_range(),
                "native derive provider must name a type implementing Derive<G>".into(),
            ));
        };
        if let Some(scope) = provider_ty.as_scope(db)
            && !hir::analysis::name_resolution::is_scope_visible_from(db, scope, request.scope())
        {
            return Err(error(
                path.syntax().text_range(),
                "native derive provider type is not visible".into(),
            ));
        }
        let (provider, input) = hir::core::meta::native_derive_provider(db, request, provider_ty)
            .map_err(|cause| match cause {
            hir::core::meta::NativeDeriveProviderError::Message(message) => error(range, message),
            hir::core::meta::NativeDeriveProviderError::Provider(failure) => {
                let diagnostics = failure
                    .vouchers()
                    .iter()
                    .map(|voucher| voucher.to_complete(db))
                    .collect::<Vec<_>>();
                diagnostic_error(
                    db,
                    &diagnostics,
                    error(
                        range,
                        format!(
                            "provider application not admitted: {}",
                            failure.description()
                        ),
                    ),
                )
            }
            hir::core::meta::NativeDeriveProviderError::Target(failure) => {
                let diagnostics = failure
                    .vouchers()
                    .iter()
                    .map(|voucher| voucher.to_complete(db))
                    .collect::<Vec<_>>();
                diagnostic_error(
                    db,
                    &diagnostics,
                    error(
                        range,
                        format!(
                            "native derive target admission failed: {}",
                            failure.description()
                        ),
                    ),
                )
            }
        })?;
        let mut plan = evaluate_provider_instance(db, file, provider, input, range)?;
        plan.request_impl = Some(range);
        requests.push(plan);
    }
    for attribute in root
        .syntax()
        .descendants()
        .filter_map(ast::NormalAttr::cast)
    {
        if attribute.is_named("generate")
            && !attribute
                .syntax()
                .ancestors()
                .any(|node| ast::QuoteExpr::cast(node).is_some())
            && !consumed.contains(&attribute.syntax().text_range())
        {
            return Err(error(
                attribute.syntax().text_range(),
                "generation request requires an authored concrete root record".into(),
            ));
        }
    }
    Ok(Some(FileRequests {
        requests,
        has_scoped_opening,
        root_range: root.syntax().text_range(),
    }))
}

/// Where a prepared view takes its authored inputs from.
enum ViewBasis {
    /// Only the one standalone file, over the builtin libraries.
    Standalone(File),
    /// Every authored input of the database, including dependencies.
    Package,
}

/// One preparation transaction over `modules`. Explicit requests are already
/// frozen; scoped demands are discovered in the output view. A standalone
/// transaction also admits the whole output once, as before; a package view
/// is returned with its ordinary diagnostics for consumers to report.
fn admit_requests(
    db: &DriverDataBase,
    basis: &ViewBasis,
    modules: &[url::Url],
    mut requests: Vec<common::generation::FrozenGenerationRequest>,
    standalone_root: Option<parser::TextRange>,
) -> Result<DriverDataBase, GenerationError> {
    let error_at = |file: &url::Url, range, cause| GenerationError {
        file: file.clone(),
        range,
        cause,
    };
    let fallback_file = &modules[0];
    let whole_input = standalone_root.unwrap_or_else(|| parser::TextRange::empty(0.into()));
    let mut completed = std::collections::HashSet::new();
    // Every retry starts from unchanged authored text and the cumulative plan.
    // A bounded transaction never recursively treats generated output as input.
    for _ in 0..128 {
        refuse_mutual_generation(&requests)?;
        let output = prepared_view(db, basis, &requests);
        let output_module = |url: &url::Url| {
            let file = output
                .workspace()
                .get(&output, url)
                .expect("prepared view keeps every authored file");
            (file, output.top_mod(file))
        };
        for url in modules {
            let (_, module) = output_module(url);
            let _ = module.scope_graph(&output);
            let lowering_errors = hir::lower::generation_lowering_errors(&output, module);
            if let Some(first) = lowering_errors.first() {
                return Err(GenerationError {
                    file: first.file.url(&output).unwrap_or_else(|| url.clone()),
                    range: first.primary_range,
                    cause: first.message.clone(),
                });
            }
        }
        // A generated header is not eligible merely because it was lowered.
        // Validate conformance before retrying any goal against the new plan.
        use hir::diagnosable::Diagnosable;
        for request in &requests {
            let Some(scoped) = request.scoped_impl.as_ref() else {
                continue;
            };
            let (_, module) = output_module(&request.target_file);
            let implementation = module
                .all_impl_traits(&output)
                .iter()
                .copied()
                .find(|implementation| {
                    implementation
                        .selection_name(&output)
                        .is_some_and(|name| name.data(&output) == &scoped.selection_name)
                })
                .ok_or_else(|| {
                    error_at(
                        &scoped.demand.opening.file,
                        scoped.demand.opening.binding_range,
                        "generated scoped implementation was not constructed".into(),
                    )
                })?;
            let mut complete = implementation
                .diags(&output)
                .into_iter()
                .map(|diagnostic| diagnostic.to_complete(&output))
                .collect::<Vec<_>>();
            for method in implementation.methods(&output) {
                complete.extend(
                    method
                        .diags(&output)
                        .into_iter()
                        .map(|diagnostic| diagnostic.to_complete(&output)),
                );
            }
            if complete
                .iter()
                .any(|diagnostic| diagnostic.severity == common::diagnostics::Severity::Error)
            {
                return Err(diagnostic_error(
                    &output,
                    &complete,
                    error_at(
                        &scoped.demand.opening.file,
                        scoped.demand.opening.binding_range,
                        "generated implementation admission failed".into(),
                    ),
                ));
            }
        }
        let pending = modules
            .iter()
            .flat_map(|url| {
                hir::analysis::ty::trait_resolution::generation::preparation_demands(
                    &output,
                    output_module(url).1,
                )
            })
            .collect::<Vec<_>>();
        if !pending.is_empty() {
            let mut new_requests = Vec::new();
            let mut batch = std::collections::HashSet::new();
            let first_opening = pending[0].opening.clone();
            // Reject every invalid opening before any provider can run; the
            // order in which demands were found must never grant authority to
            // an earlier entry.
            let pending = pending
                .into_iter()
                .map(|pending| {
                    pending.demand.map_err(|cause| {
                        error_at(&pending.opening.file, pending.opening.binding_range, cause)
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            // Admission can itself depend on generated const-condition bodies.
            // Discover every such request and invalid opening before any CTFE.
            let mut pending = std::collections::VecDeque::from(pending);
            let mut ready = Vec::new();
            while let Some(demand) = pending.pop_front() {
                let file = demand.opening.file.clone();
                let range = demand.opening.binding_range;
                if !batch.insert(demand.clone()) {
                    continue;
                }
                if completed.contains(&demand) {
                    return Err(error_at(
                        &file,
                        range,
                        "generated implementation did not discharge its preparation dependency"
                            .into(),
                    ));
                }
                let request = hir::core::meta::rebind_generation_request(&output, &demand)
                    .map_err(|cause| error_at(&file, range, cause))?;
                let dependencies =
                    hir::analysis::ty::trait_resolution::generation::admission_demands(
                        &output, request,
                    );
                let blocked = !dependencies.is_empty();
                for dependency in &dependencies {
                    pending.push_back(dependency.demand.clone().map_err(|cause| {
                        error_at(
                            &dependency.opening.file,
                            dependency.opening.binding_range,
                            cause,
                        )
                    })?);
                }
                if !blocked {
                    ready.push((demand, request));
                }
            }
            for (demand, request) in ready {
                let file = demand.opening.file.clone();
                let range = demand.opening.binding_range;
                let opening = request.opening(&output);
                let origin = opening.scope(&output).item();
                let (key, input) = hir::core::meta::scoped_derive_provider(
                    &output,
                    opening,
                    request.goal(&output),
                    origin,
                    origin.span().into(),
                )
                .map_err(|cause| {
                    scoped_provider_error(
                        &output,
                        cause,
                        error_at(
                            &file,
                            range,
                            "scoped derive provider admission failed".into(),
                        ),
                    )
                })?;
                if provider_needs_generation(&output, key) {
                    continue;
                }
                let (opening_file, _) = output_module(&file);
                let mut plan =
                    evaluate_provider_instance(&output, opening_file, key, input, range)?;
                completed.insert(demand.clone());
                plan.scoped_impl = Some(common::generation::FrozenScopedImpl {
                    selection_name: scoped_selection_name(&demand),
                    demand,
                });
                new_requests.push(plan);
                // Publish one ready dependency before considering its dependents.
                break;
            }
            if new_requests.is_empty() {
                let (file, range) = match standalone_root {
                    Some(root) => (fallback_file.clone(), root),
                    None => (first_opening.file, first_opening.binding_range),
                };
                return Err(error_at(&file, range,
                    "scoped derive dependencies cannot make progress (cyclic or unresolved provider bodies)".into()));
            }
            requests.extend(new_requests);
            continue;
        }
        if let ViewBasis::Standalone(_) = basis {
            let (_, module) = output_module(fallback_file);
            let diagnostics = output.run_on_top_mod(module);
            if diagnostics.has_errors(&output) {
                return Err(diagnostic_error(
                    &output,
                    &diagnostics.finalize(&output),
                    error_at(
                        fallback_file,
                        whole_input,
                        "generated output admission failed".into(),
                    ),
                ));
            }
            drop(diagnostics);
        }
        return Ok(output);
    }
    Err(error_at(
        fallback_file,
        whole_input,
        "scoped generation preparation limit exceeded".into(),
    ))
}

/// Whether the provider instance `key`, or anything its evaluation runs, waits
/// for a generated implementation. Evaluation runs the bodies of the calls the
/// instance makes and of the constants it reads, so each is asked in turn.
fn provider_needs_generation<'db>(
    db: &'db DriverDataBase,
    key: hir::analysis::semantic::SemanticInstanceKey<'db>,
) -> bool {
    let mut pending = std::collections::VecDeque::from([key]);
    let mut seen = std::collections::HashSet::new();
    while let Some(key) = pending.pop_front() {
        if !seen.insert(key) {
            continue;
        }
        if !checked_body_demands(db, key.owner(db)).is_empty() {
            return true;
        }
        let instance = hir::analysis::semantic::SemanticInstance::new(db, key);
        for callee in instance.callees(db) {
            pending.push_back(callee.key);
        }
        pending.extend(instance.implicit_dependencies(db));
    }
    false
}

fn scoped_selection_name(demand: &common::generation::ScopedDeriveDemand) -> String {
    // Length-prefixed identity is injective; the NUL prefix is unspellable in
    // authored identifiers. No implementation authority comes from this name.
    let mut name = String::from("\0scoped-derive");
    for component in [
        demand.opening.file.as_str().to_owned(),
        format!("{:?}", demand.opening.binding_range),
        demand.trait_ref.file.as_str().to_owned(),
        format!("{:?}", demand.trait_ref.range),
        demand.target.target_file.as_str().to_owned(),
        format!("{:?}", demand.target.target_range),
        format!("{:?}", demand.mode),
    ] {
        use std::fmt::Write;
        write!(&mut name, ":{}:{}", component.len(), component).unwrap();
    }
    name
}

fn scoped_provider_error(
    db: &DriverDataBase,
    cause: hir::core::meta::NativeDeriveProviderError<'_>,
    fallback: GenerationError,
) -> GenerationError {
    let (failure, label) = match cause {
        hir::core::meta::NativeDeriveProviderError::Message(message) => {
            return GenerationError {
                cause: message,
                ..fallback
            };
        }
        hir::core::meta::NativeDeriveProviderError::Provider(failure) => {
            (failure, "provider application not admitted")
        }
        hir::core::meta::NativeDeriveProviderError::Target(failure) => {
            (failure, "scoped derive target admission failed")
        }
    };
    let complete = failure
        .vouchers()
        .iter()
        .map(|voucher| voucher.to_complete(db))
        .collect::<Vec<_>>();
    diagnostic_error(
        db,
        &complete,
        GenerationError {
            cause: format!("{label}: {}", failure.description()),
            ..fallback
        },
    )
}

/// Lowering a module's generated code reads the declarations of every other
/// module it names: the quote's producer scope, `${record}` targets and scoped
/// trait goals (`resolve_producer_scope`, `resolve_record_item`,
/// `resolve_trait_item`). When two modules both receive code that reads the
/// other, their lowerings depend on each other. Refuse that before publishing
/// rather than let lowering recurse; lowering authored declarations apart from
/// generated output would lift it (SIX-AREA-PLAN A4).
fn refuse_mutual_generation(
    requests: &[common::generation::FrozenGenerationRequest],
) -> Result<(), GenerationError> {
    use common::generation::{FrozenCode, FrozenHoleValue};
    fn code_modules<'a>(code: &'a FrozenCode, out: &mut Vec<&'a url::Url>) {
        match code {
            FrozenCode::Template {
                quote_file, holes, ..
            } => {
                out.push(quote_file);
                for hole in holes {
                    match hole {
                        FrozenHoleValue::Record(record) => out.push(&record.target_file),
                        FrozenHoleValue::Field(field) | FrozenHoleValue::FieldType(field) => {
                            out.push(&field.record.target_file)
                        }
                        FrozenHoleValue::Code(nested) => code_modules(nested, out),
                        FrozenHoleValue::Scalar(_) => {}
                    }
                }
            }
            FrozenCode::VariantBinders { record, .. }
            | FrozenCode::VariantBinderField { record, .. } => out.push(&record.target_file),
            FrozenCode::Empty { .. } => {}
        }
    }
    let mut reads: std::collections::BTreeMap<&url::Url, std::collections::BTreeSet<&url::Url>> =
        Default::default();
    for request in requests {
        let mut named = Vec::new();
        code_modules(&request.code, &mut named);
        if let Some(scoped) = &request.scoped_impl {
            named.push(&scoped.demand.trait_ref.file);
            named.push(&scoped.demand.target.target_file);
        }
        let entry = reads.entry(&request.target_file).or_default();
        entry.extend(named.into_iter().filter(|url| *url != &request.target_file));
    }
    // A module that reads another module which (transitively) reads it back.
    for &start in reads.keys() {
        let mut stack = vec![start];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(module) = stack.pop() {
            for &next in reads.get(module).into_iter().flatten() {
                if next == start {
                    let request = requests
                        .iter()
                        .find(|request| &request.target_file == start)
                        .expect("a reading module has a request");
                    let (file, range) = match &request.scoped_impl {
                        Some(scoped) => (
                            scoped.demand.opening.file.clone(),
                            scoped.demand.opening.binding_range,
                        ),
                        None => (
                            start.clone(),
                            request
                                .request_impl
                                .unwrap_or_else(|| parser::TextRange::empty(0.into())),
                        ),
                    };
                    return Err(GenerationError {
                        file,
                        range,
                        cause: format!(
                            "generated code in {start} and {module} reads each other's declarations; \
                             mutual generation between modules is not supported yet"
                        ),
                    });
                }
                if seen.insert(next) {
                    stack.push(next);
                }
            }
        }
    }
    Ok(())
}

fn prepared_view(
    db: &DriverDataBase,
    basis: &ViewBasis,
    requests: &[common::generation::FrozenGenerationRequest],
) -> DriverDataBase {
    let mut output = match basis {
        ViewBasis::Standalone(file) => {
            let url = file.url(db).expect("indexed generation input");
            let mut output = DriverDataBase::default();
            output
                .workspace()
                .touch(&mut output, url.clone(), Some(file.text(db).clone()));
            output
                .compiler_options()
                .set_recovery_mode(&mut output)
                .to(db.compiler_options().recovery_mode(db));
            output
                .compilation_settings()
                .set_profile(&mut output)
                .to(db.compilation_settings().profile(db).clone());
            output
        }
        ViewBasis::Package => db.fork_inputs(),
    };
    output
        .generation_inputs()
        .set_requests(&mut output)
        .to(requests.to_vec());
    output
}

fn evaluate_provider<'db>(
    db: &'db DriverDataBase,
    file: File,
    provider: hir::hir_def::Func<'db>,
    input: RecordInput<'db>,
    range: parser::TextRange,
) -> Result<common::generation::FrozenGenerationRequest, GenerationError> {
    let key = hir::analysis::semantic::generated_callee_key(
        db,
        input.scope(),
        hir::analysis::ty::trait_resolution::PredicateListId::empty_list(db),
        provider,
        None,
        &[],
    )
    .map_err(|_| GenerationError {
        file: file.url(db).expect("indexed generation input"),
        range,
        cause: "provider function could not be admitted".into(),
    })?;
    evaluate_provider_instance(db, file, key, input, range)
}

fn evaluate_provider_instance<'db>(
    db: &'db DriverDataBase,
    file: File,
    provider_key: hir::analysis::semantic::SemanticInstanceKey<'db>,
    input: RecordInput<'db>,
    range: parser::TextRange,
) -> Result<common::generation::FrozenGenerationRequest, GenerationError> {
    let BodyOwner::Func(provider, _) = provider_key.owner(db) else {
        return Err(GenerationError {
            file: file.url(db).unwrap(),
            range,
            cause: "provider is not a function instance".into(),
        });
    };
    let url = file.url(db).expect("indexed generation input");
    let error = |range, cause| GenerationError {
        file: url.clone(),
        range,
        cause,
    };
    if !provider.is_const(db)
        || provider.body(db).is_none()
        || !CallableDef::Func(provider).params(db).is_empty()
    {
        return Err(error(
            range,
            "provider must be an authored nongeneric const function".into(),
        ));
    }
    let carrier = resolve_lib_type_path(db, provider.scope(), "core::meta::Record")
        .ok_or_else(|| error(range, "missing Record carrier".into()))?;
    let code = resolve_lib_type_path(db, provider.scope(), "core::meta::Code")
        .ok_or_else(|| error(range, "missing Code carrier".into()))?;
    let item = resolve_lib_type_path(db, provider.scope(), "core::meta::Item")
        .ok_or_else(|| error(range, "missing Item kind".into()))?;
    let arguments = provider.arg_tys(db);
    if arguments.len() != 1
        || arguments[0]
            .instantiate_identity()
            .as_view(db)
            .unwrap_or(arguments[0].instantiate_identity())
            != carrier
        || provider
            .return_ty(db)
            .as_view(db)
            .unwrap_or(provider.return_ty(db))
            != TyId::app(db, code, item)
    {
        return Err(error(
            range,
            format!(
                "provider signature must be const fn(Record) -> Code<Item>: args={:?}, result={}, expected={}",
                arguments
                    .iter()
                    .map(|a| a.instantiate_identity().pretty_print(db))
                    .collect::<Vec<_>>(),
                provider.return_ty(db).pretty_print(db),
                TyId::app(db, code, item).pretty_print(db)
            ),
        ));
    }
    let diagnostics = &check_func_body(db, provider).0;
    if !body_errors(db, provider).is_empty() {
        for diagnostic in diagnostics {
            if let hir::analysis::ty::diagnostics::FuncBodyDiag::Body(
                hir::analysis::ty::diagnostics::BodyDiag::UndefinedVariable(span, name, _),
            ) = diagnostic
                && let Some(span) = span.resolve(db)
            {
                return Err(GenerationError {
                    file: span.file.url(db).unwrap_or(url.clone()),
                    range: span.range,
                    cause: format!(
                        "authored provider references undefined variable `{}`; generated output is not authored input",
                        name.data(db)
                    ),
                });
            }
        }
        let complete = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.to_complete(db))
            .collect::<Vec<_>>();
        return Err(diagnostic_error(
            db,
            &complete,
            error(range, "authored provider body diagnostics".into()),
        ));
    }
    let owner = provider_key.owner(db);
    let instance = hir::analysis::semantic::SemanticInstance::new(db, provider_key);
    let mut validation_diagnostics = Vec::new();
    let mut unresolved = Vec::new();
    {
        use hir::analysis::semantic::access::SemanticAccessCheckResult;
        let mut pending = std::collections::VecDeque::from([instance]);
        let mut seen = std::collections::HashSet::new();
        while let Some(current) = pending.pop_front() {
            if !seen.insert(current) {
                continue;
            }
            match hir::analysis::semantic::access::semantic_access_check_query(db, current) {
                SemanticAccessCheckResult::Ok { .. } => {
                    for callee in current.callees(db) {
                        pending.push_back(hir::analysis::semantic::SemanticInstance::new(
                            db, callee.key,
                        ));
                    }
                }
                SemanticAccessCheckResult::Err(diagnostic) => {
                    let diagnostic = diagnostic.diag(db);
                    let origin_owner = diagnostic.instance.key(db).owner(db);
                    let errors = match origin_owner {
                        BodyOwner::Func(function, _) => body_errors(db, function),
                        _ => Vec::new(),
                    };
                    if !errors.is_empty() {
                        validation_diagnostics.extend(errors);
                    } else {
                        validation_diagnostics.push(diagnostic.to_complete(db));
                    }
                }
                SemanticAccessCheckResult::Blocked(cause) => {
                    let origin_owner = current.key(db).owner(db);
                    let errors = match origin_owner {
                        BodyOwner::Func(function, _) => body_errors(db, function),
                        _ => Vec::new(),
                    };
                    if !errors.is_empty() {
                        validation_diagnostics.extend(errors);
                    } else {
                        unresolved.push(format!("Blocked: {cause:?}"));
                    }
                }
            }
        }
    }
    if !validation_diagnostics.is_empty() {
        return Err(diagnostic_error(
            db,
            &validation_diagnostics,
            error(
                range,
                "authored provider ownership/capability validation failed".into(),
            ),
        ));
    }
    if !unresolved.is_empty() {
        // Pending and Blocked are not a ready provider and are never
        // replaced with a successful quotation or an empty output.
        return Err(error(
            range,
            format!(
                "authored provider validation unresolved: {}",
                unresolved.join("; ")
            ),
        ));
    }
    let value = match hir::analysis::semantic::ctfe::force_const_computation(
        db,
        hir::analysis::semantic::ctfe::const_computation_for_instance(
            db,
            provider_key,
            vec![
                hir::core::meta::record_input_argument(db, input, owner.scope())
                    .map_err(|cause| error(range, cause.into()))?,
            ],
        ),
        hir::analysis::semantic::ctfe::CtfeConfig::default(),
    ) {
        EvalOutcome::Ready(value) => value.value(),
        EvalOutcome::Failed(cause) => {
            let fallback = error(range, format!("provider evaluation failed: {cause:?}"));
            return Err(match &cause {
                EvalFailure::Ctfe(cause) => evaluation_error(db, owner, cause, fallback),
                _ => fallback,
            });
        }
        EvalOutcome::Blocked(cause) => {
            return Err(error(
                range,
                format!("provider evaluation blocked: {cause:?}"),
            ));
        }
    };
    let SemConstValue::ItemCode { code, .. } = value.value(db) else {
        return Err(error(
            range,
            "provider returned an ordinary value rather than checked item code".into(),
        ));
    };
    code.freeze_request(db, file)
        .map_err(|cause| error(range, cause.into()))
}
