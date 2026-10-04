//! Authored program declarations for the v1 model fragment, without resolution.
use super::*;
use hir::hir_def::{
    ConstGenericArgValue, GenericArg, GenericParam, GenericParamOwner, PathId, PathKind,
    TopLevelMod, TraitRefId, TypeBound, TypeId, TypeKind,
};
use std::collections::BTreeMap;

type Parameters = BTreeMap<String, &'static str>;
const MAX_TERM_DEPTH: usize = 64;

/// Read declaration shapes. Function use records are encoded separately.
pub fn declaration_records<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    module: TopLevelMod<'db>,
    diagnostics: &[common::diagnostics::CompleteDiagnostic],
) -> Vec<Value> {
    module.children_non_nested(db).filter_map(|item| {
        if matches!(item, ItemKind::Use(_)) { return None; }
        let result = declaration(db,item,diagnostics).and_then(|mut record| {
            let region: DynLazySpan = match item {
                ItemKind::Func(function) => function.span().sig().into(),
                _ => item.span().into(),
            };
            let excluded = match item {
                ItemKind::Trait(trait_) => trait_.methods(db).map(|m|m.span().into()).collect(),
                ItemKind::ImplTrait(impl_) => impl_.methods(db).filter_map(|m|m.body(db).map(|b|b.span().into())).collect(),
                _ => vec![],
            };
            record["span_aliases"] = json!(span_aliases(db,region,&excluded,diagnostics,None)?);
            Ok(record)
        });
        Some(match result {
            Ok(record) => record,
            Err(reason) => json!({"v":1,"kind":"item","item":"out_of_fragment","what":item.kind_name(),"reason":reason}),
        })
    }).collect()
}

fn declaration<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    item: ItemKind<'db>,
    diagnostics: &[common::diagnostics::CompleteDiagnostic],
) -> Result<Value, String> {
    if let ItemKind::Const(value) = item {
        return Ok(
            json!({"v":1,"kind":"item","item":"value","name":value.name(db).to_opt().ok_or("unnamed constant")?.data(db)}),
        );
    }
    let owner = GenericParamOwner::from_item_opt(item).ok_or("declaration is outside format v1")?;
    let (params, env) = parameters(db, owner)?;
    let (bounds, conds) = clause(db, owner, &env)?;
    match item {
        ItemKind::Func(function) => {
            if function.params(db).next().is_some() {
                return Err("function value parameters are outside this v1 projection".into());
            }
            let body = function.body(db).ok_or("function has no body")?;
            let (_, typed) = hir::analysis::ty::ty_check::check_func_body(db, function);
            let uses = body_uses(db, body, body.expr(db), &env, typed, diagnostics, 0)?;
            Ok(
                json!({"v":1,"kind":"item","item":"fn","name":function.name(db).to_opt().ok_or("unnamed function")?.data(db),"params":params,"const":function.is_const(db),"bounds":bounds,"conds":conds,"body":uses}),
            )
        }
        ItemKind::Struct(struct_) => {
            if !struct_.hir_fields(db).data(db).is_empty() {
                return Err("struct fields are outside the v1 model fragment".into());
            }
            Ok(
                json!({"v":1,"kind":"item","item":"struct","name":struct_.name(db).to_opt().ok_or("unnamed struct")?.data(db),"params":params,"bounds":bounds,"conds":conds}),
            )
        }
        ItemKind::Trait(trait_) => {
            if trait_.super_trait_refs(db).next().is_some() {
                return Err("super-traits are outside the v1 model fragment".into());
            }
            if trait_.assoc_types(db).next().is_some() {
                return Err("associated types are outside format v1".into());
            }
            let consts = trait_.assoc_consts(db).map(|constant| {
                if constant.has_default(db) { return Err("associated constant defaults are outside format v1"); }
                let ty = constant.hir_ty(db).ok_or("associated constant has no type")?;
                let kind = constant_kind(db,ty)?;
                Ok(json!({"name":constant.name(db).ok_or("unnamed associated constant")?.data(db),"kind":kind}))
            }).collect::<Result<Vec<_>,&str>>()?;
            let methods = trait_
                .methods(db)
                .map(|method| method_record(db, method, diagnostics))
                .collect::<Result<Vec<_>, _>>()?;
            if !bounds.is_empty() || !conds.is_empty() {
                return Err("trait-level requirements are outside format v1".into());
            }
            Ok(
                json!({"v":1,"kind":"item","item":"trait","name":trait_.name(db).to_opt().ok_or("unnamed trait")?.data(db),"params":params,"assoc_consts":consts,"methods":methods,"fixed":false}),
            )
        }
        ItemKind::ImplTrait(impl_) => {
            let reference = impl_
                .hir_trait_ref(db)
                .to_opt()
                .ok_or("missing trait reference")?;
            let (name, args) = trait_reference(db, reference, &env, 0)?;
            let self_ty = syntax_type(
                db,
                impl_.hir_type_ref(db).to_opt().ok_or("missing impl type")?,
                &env,
                0,
            )?;
            let consts = impl_.hir_consts(db).iter().map(|constant| {
                let body = constant.value.to_opt().ok_or("missing associated constant value")?;
                Ok(json!({"name":constant.name.to_opt().ok_or("unnamed associated constant")?.data(db),"value":syntax_expr(db,body,body.expr(db),&env,0)?}))
            }).collect::<Result<Vec<_>,String>>()?;
            let methods = impl_
                .methods(db)
                .map(|method| method_record(db, method, diagnostics))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(
                json!({"v":1,"kind":"item","item":"impl","index":impl_.declaration_index(db),"name":impl_.selection_name(db).map(|name|name.data(db)),"params":params,"trait":name,"targs":args,"self":self_ty,"bounds":bounds,"conds":conds,"consts":consts,"methods":methods}),
            )
        }
        _ => Err("declaration is outside format v1".into()),
    }
}

fn method_record<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    method: hir::hir_def::Func<'db>,
    diagnostics: &[common::diagnostics::CompleteDiagnostic],
) -> Result<Value, String> {
    let owner = GenericParamOwner::Func(method);
    let (params, env) = parameters(db, owner)?;
    if !params.is_empty() {
        return Err("generic methods are outside format v1".into());
    }
    let (bounds, conds) = clause(db, owner, &env)?;
    if !bounds.is_empty() {
        return Err("method bounds are outside format v1".into());
    }
    let mut record =
        json!({"name":method.name(db).to_opt().ok_or("unnamed method")?.data(db),"conds":conds});
    let aliases = span_aliases(db, method.span().sig().into(), &[], diagnostics, None)?;
    if !aliases.is_empty() {
        record["span_aliases"] = json!(aliases);
    }
    Ok(record)
}

fn parameters<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    owner: GenericParamOwner<'db>,
) -> Result<(Vec<Value>, Parameters), String> {
    let mut chain = vec![owner];
    while let Some(parent) = chain.last().unwrap().parent(db) {
        chain.push(parent);
    }
    let mut env = Parameters::new();
    for scope in chain.iter().rev() {
        for parameter in scope.params(db) {
            let name = parameter
                .name()
                .to_opt()
                .ok_or("unnamed parameter")?
                .data(db)
                .clone();
            let kind = match parameter.param {
                GenericParam::Type(_) => "type",
                GenericParam::Const(constant) => constant_kind(
                    db,
                    constant
                        .ty
                        .to_opt()
                        .ok_or("constant parameter has no type")?,
                )?,
            };
            env.insert(name, kind);
        }
    }
    let params = owner
        .params(db)
        .map(|parameter| {
            let name = parameter
                .name()
                .to_opt()
                .ok_or("unnamed parameter")?
                .data(db);
            Ok(json!({"name":name,"kind":env[name]}))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((params, env))
}

fn constant_kind<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    ty: TypeId<'db>,
) -> Result<&'static str, &'static str> {
    match ty.pretty_print(db).as_str() {
        "bool" => Ok("bool"),
        "usize" => Ok("nat"),
        _ => Err("constant kind is outside the v1 fragment (bool/usize)"),
    }
}

fn clause<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    owner: GenericParamOwner<'db>,
    env: &Parameters,
) -> Result<(Vec<Value>, Vec<Value>), String> {
    let clause = owner
        .where_clause_owner()
        .ok_or("owner has no where clause")?
        .where_clause(db);
    let mut bounds = vec![];
    for parameter in owner.params(db) {
        if let GenericParam::Type(parameter) = parameter.param {
            let name = parameter.name.to_opt().ok_or("unnamed parameter")?.data(db);
            append_bounds(
                db,
                json!({"var":name,"kind":"type"}),
                &parameter.bounds,
                env,
                &mut bounds,
            )?;
        }
    }
    for (_, predicate) in clause.type_predicates(db) {
        let self_ty = syntax_type(
            db,
            predicate.ty.to_opt().ok_or("missing bound subject")?,
            env,
            0,
        )?;
        append_bounds(db, self_ty, &predicate.bounds, env, &mut bounds)?;
    }
    let conds = clause
        .const_predicates(db)
        .iter()
        .map(|body| syntax_expr(db, *body, body.expr(db), env, 0))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((bounds, conds))
}

fn append_bounds<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    self_ty: Value,
    bounds: &[TypeBound<'db>],
    env: &Parameters,
    output: &mut Vec<Value>,
) -> Result<(), String> {
    for bound in bounds {
        let TypeBound::Trait(reference) = bound else {
            return Err("kind bound is outside format v1".into());
        };
        let (name, args) = trait_reference(db, *reference, env, 0)?;
        output.push(json!({"self":self_ty,"trait":name,"args":args}));
    }
    Ok(())
}

fn trait_reference<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    reference: TraitRefId<'db>,
    env: &Parameters,
    depth: usize,
) -> Result<(String, Vec<Value>), String> {
    let path = reference.path(db).to_opt().ok_or("missing trait path")?;
    simple_path(db, path, env, depth)
}

fn simple_path<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    path: PathId<'db>,
    env: &Parameters,
    depth: usize,
) -> Result<(String, Vec<Value>), String> {
    if depth >= MAX_TERM_DEPTH {
        return Err("v1 source term exceeds depth 64".into());
    }
    if path.parent(db).is_some() {
        return Err("qualified name is outside this v1 source projection".into());
    }
    let PathKind::Ident {
        ident,
        generic_args,
    } = path.kind(db)
    else {
        return Err("generated path is outside format v1".into());
    };
    let name = ident.to_opt().ok_or("missing path name")?.data(db).clone();
    let args = generic_args
        .data(db)
        .iter()
        .map(|argument| match argument {
            GenericArg::Type(argument) => syntax_type(
                db,
                argument
                    .ty
                    .to_opt()
                    .ok_or("missing generic type argument")?,
                env,
                depth + 1,
            ),
            GenericArg::Const(argument) => match argument.value {
                ConstGenericArgValue::Expr(body) => {
                    let body = body.to_opt().ok_or("missing constant argument")?;
                    syntax_expr(db, body, body.expr(db), env, depth + 1)
                }
                _ => Err("constant hole is outside format v1".into()),
            },
            _ => Err("associated type argument is outside format v1".into()),
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((name, args))
}

fn syntax_type<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    ty: TypeId<'db>,
    env: &Parameters,
    depth: usize,
) -> Result<Value, String> {
    if depth >= MAX_TERM_DEPTH {
        return Err("v1 source term exceeds depth 64".into());
    }
    match ty.data(db) {
        TypeKind::Path(path) => {
            let path = path.to_opt().ok_or("missing type path")?;
            if let Some((goal, name)) = qualified_member(db, path, env, depth + 1)? {
                return Ok(
                    json!({"assoc":goal["self"],"trait":goal["trait"],"targs":goal["args"],"c":name}),
                );
            }
            let (name, args) = simple_path(db, path, env, depth + 1)?;
            if let Some(kind) = env.get(&name) {
                if !args.is_empty() {
                    return Err("applied parameter is outside format v1".into());
                }
                Ok(json!({"var":name,"kind":kind}))
            } else {
                Ok(json!({"con":name,"args":args}))
            }
        }
        TypeKind::Primitive(primitive) => Ok(json!({"con":primitive.name(db).data(db),"args":[]})),
        TypeKind::Tuple(types) if types.data(db).is_empty() => Ok(json!({"con":"()","args":[]})),
        _ => Err("type is outside the v1 source fragment".into()),
    }
}

fn syntax_expr<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    body: Body<'db>,
    expr: hir::hir_def::ExprId,
    env: &Parameters,
    depth: usize,
) -> Result<Value, String> {
    if depth >= MAX_TERM_DEPTH {
        return Err("v1 source term exceeds depth 64".into());
    }
    match expr.data(db, body) {
        hir::hir_def::Partial::Present(Expr::Block(statements)) if statements.len() == 1 => {
            let hir::hir_def::Partial::Present(hir::hir_def::Stmt::Expr(value)) =
                statements[0].data(db, body)
            else {
                return Err("constant block is outside the v1 source fragment".into());
            };
            syntax_expr(db, body, *value, env, depth + 1)
        }
        hir::hir_def::Partial::Present(Expr::Lit(LitKind::Bool(value))) => {
            Ok(json!({"bool":value}))
        }
        hir::hir_def::Partial::Present(Expr::Lit(LitKind::Int(value))) => Ok(
            json!({"nat":value.data(db).to_string().parse::<u64>().map_err(|_|"natural literal exceeds encoder range")?}),
        ),
        hir::hir_def::Partial::Present(Expr::Path(path)) => {
            let path = path.to_opt().ok_or("missing expression path")?;
            if let Some((goal, name)) = qualified_member(db, path, env, depth + 1)? {
                return Ok(
                    json!({"assoc":goal["self"],"trait":goal["trait"],"targs":goal["args"],"c":name}),
                );
            }
            let (name, args) = simple_path(db, path, env, depth + 1)?;
            if !args.is_empty() {
                return Err("applied expression name is outside format v1".into());
            }
            Ok(json!({"var":name}))
        }
        hir::hir_def::Partial::Present(Expr::Bin(a, b, op)) => {
            let op = binary_operator(*op)?;
            Ok(
                json!({"op":op,"a":syntax_expr(db,body,*a,env,depth+1)?,"b":syntax_expr(db,body,*b,env,depth+1)?}),
            )
        }
        hir::hir_def::Partial::Present(Expr::Un(value, UnOp::Not)) => {
            Ok(json!({"not":syntax_expr(db,body,*value,env,depth+1)?}))
        }
        _ => Err("expression is outside the v1 source fragment".into()),
    }
}

fn qualified_member<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    path: PathId<'db>,
    env: &Parameters,
    depth: usize,
) -> Result<Option<(Value, String)>, String> {
    let Some(parent) = path.parent(db) else {
        return Ok(None);
    };
    let PathKind::QualifiedType { type_, trait_ } = parent.kind(db) else {
        return Ok(None);
    };
    let PathKind::Ident {
        ident,
        generic_args,
    } = path.kind(db)
    else {
        return Err("missing qualified member".into());
    };
    if !generic_args.data(db).is_empty() {
        return Err("generic member is outside format v1".into());
    }
    let (trait_name, args) = trait_reference(db, trait_, env, depth)?;
    let self_ty = syntax_type(db, type_, env, depth)?;
    Ok(Some((
        json!({"self":self_ty,"trait":trait_name,"args":args}),
        ident
            .to_opt()
            .ok_or("unnamed qualified member")?
            .data(db)
            .clone(),
    )))
}

fn body_uses<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    body: Body<'db>,
    expr: hir::hir_def::ExprId,
    env: &Parameters,
    typed: &TypedBody<'db>,
    diagnostics: &[common::diagnostics::CompleteDiagnostic],
    depth: usize,
) -> Result<Vec<Value>, String> {
    if depth >= MAX_TERM_DEPTH {
        return Err("v1 use nesting exceeds depth 64".into());
    }
    let mut uses = vec![];
    match expr.data(db, body) {
        hir::hir_def::Partial::Present(Expr::Block(statements)) => {
            for statement in statements {
                let value = match statement.data(db, body) {
                    hir::hir_def::Partial::Present(hir::hir_def::Stmt::Let(_, _, Some(value)))
                    | hir::hir_def::Partial::Present(hir::hir_def::Stmt::Expr(value)) => *value,
                    _ => return Err("statement is outside the v1 use fragment".into()),
                };
                let mut children = body_uses(db, body, value, env, typed, diagnostics, depth + 1)?;
                if matches!(
                    statement.data(db, body),
                    hir::hir_def::Partial::Present(hir::hir_def::Stmt::Let(_, Some(_), _))
                ) && let [use_] = children.as_mut_slice()
                {
                    let aliases = span_aliases(
                        db,
                        statement.span(body).into_let_stmt().ty().into(),
                        &[],
                        diagnostics,
                        Some(typed),
                    )?;
                    let current = use_["span_aliases"]
                        .as_array_mut()
                        .ok_or("authored use has no span aliases")?;
                    for alias in aliases {
                        if !current.contains(&alias) {
                            current.push(alias);
                        }
                    }
                }
                uses.extend(children);
            }
        }
        hir::hir_def::Partial::Present(Expr::Lit(_)) => {}
        hir::hir_def::Partial::Present(Expr::Call(callee, args)) => {
            if !args.is_empty() {
                return Err("value arguments are outside the v1 use fragment".into());
            }
            let hir::hir_def::Partial::Present(Expr::Path(path)) = callee.data(db, body) else {
                return Err("non-path call is outside format v1".into());
            };
            let path = path.to_opt().ok_or("missing call path")?;
            if let Some((goal, name)) = qualified_member(db, path, env, 0)? {
                uses.push(json!({"use":"method","goal":goal,"method":name,"qualified":true}));
            } else if path.parent(db).is_some() {
                let instance = typed
                    .callable_expr(expr)
                    .and_then(|callable| callable.trait_inst)
                    .ok_or("unqualified call has no checked trait instance")?;
                let name = path.ident(db).to_opt().ok_or("unnamed method")?.data(db);
                uses.push(json!({"use":"method","goal":goal(db,instance)?,"method":name,"qualified":false}));
            } else {
                let (name, args) = simple_path(db, path, env, 0)?;
                uses.push(json!({"use":"call","fn":name,"args":args}));
            }
        }
        hir::hir_def::Partial::Present(Expr::Path(path)) => {
            let (goal, name) =
                qualified_member(db, path.to_opt().ok_or("missing constant path")?, env, 0)?
                    .ok_or("unqualified value use is outside this v1 projection")?;
            uses.push(json!({"use":"const","goal":goal,"c":name}));
        }
        hir::hir_def::Partial::Present(Expr::RecordInit(path, fields)) => {
            if !fields.is_empty() {
                return Err("record fields are outside format v1".into());
            }
            let (name, args) =
                simple_path(db, path.to_opt().ok_or("missing record path")?, env, 0)?;
            uses.push(json!({"use":"type","type":{"con":name,"args":args}}));
        }
        hir::hir_def::Partial::Present(Expr::With(bindings, inner)) => {
            let names = bindings
                .iter()
                .map(|binding| {
                    if binding.key_path.is_some() {
                        return Err("keyed opening is outside format v1".into());
                    }
                    let hir::hir_def::Partial::Present(Expr::Path(path)) =
                        binding.value.data(db, body)
                    else {
                        return Err("evidence opening requires the Step 2 record".into());
                    };
                    let (name, args) =
                        simple_path(db, path.to_opt().ok_or("missing opening path")?, env, 0)?;
                    if !args.is_empty() {
                        return Err("applied opening is outside format v1".into());
                    }
                    Ok(name)
                })
                .collect::<Result<Vec<_>, String>>()?;
            let inner = body_uses(db, body, *inner, env, typed, diagnostics, depth + 1)?;
            if names.len() == 1 {
                uses.push(json!({"use":"with","name":names[0],"body":inner}));
            } else {
                uses.push(json!({"use":"with_list","names":names,"body":inner}));
            }
        }
        _ => return Err("expression is outside the v1 use fragment".into()),
    }
    if !matches!(
        expr.data(db, body),
        hir::hir_def::Partial::Present(Expr::Block(_))
    ) {
        let construct_span = source_site(db, &expr.span(body).into())?;
        let span = match expr.data(db, body) {
            hir::hir_def::Partial::Present(Expr::With(bindings, _)) if bindings.len() == 1 => {
                source_site(db, &bindings[0].value.span(body).into())?
            }
            _ => construct_span.clone(),
        };
        for use_ in &mut uses {
            use_["span"] = span.clone();
            let excluded = match expr.data(db, body) {
                hir::hir_def::Partial::Present(Expr::With(_, inner)) => {
                    vec![inner.span(body).into()]
                }
                _ => vec![],
            };
            use_["span_aliases"] = json!(span_aliases(
                db,
                expr.span(body).into(),
                &excluded,
                diagnostics,
                Some(typed)
            )?);
            if use_["use"] == "with" {
                use_["construct_span"] = construct_span.clone();
            }
        }
    }
    Ok(uses)
}

// Alias actual reported/retained coordinates to the smallest authored use
// region. Nested with bodies are handled by their own use records.
fn span_aliases<'db>(
    db: &'db dyn SpannedHirAnalysisDb,
    region: DynLazySpan<'db>,
    excluded: &[DynLazySpan<'db>],
    diagnostics: &[common::diagnostics::CompleteDiagnostic],
    typed: Option<&TypedBody<'db>>,
) -> Result<Vec<Value>, String> {
    let region = region
        .resolve(db)
        .ok_or("authored region has no source span")?;
    let excluded = excluded
        .iter()
        .filter_map(|s| s.resolve(db))
        .collect::<Vec<_>>();
    let mut points = diagnostics
        .iter()
        .filter_map(|d| d.primary_span())
        .collect::<Vec<_>>();
    if let Some(typed) = typed {
        points.extend(
            typed
                .decisions()
                .condition_entries()
                .filter_map(|(_, c)| c.span().resolve(db)),
        );
        points.extend(
            typed
                .decisions()
                .choice_entries(db)
                .filter_map(|c| c.span().resolve(db)),
        );
        if let Some(body) = typed.body() {
            points.extend(
                typed
                    .with_choices()
                    .filter_map(|c| c.expression().span(body).resolve(db)),
            );
        }
    }
    let mut offsets = std::collections::BTreeSet::new();
    let mut result = vec![];
    for point in points {
        let inside = |r: &common::diagnostics::Span| {
            point.file == r.file && r.range.contains(point.range.start())
        };
        if inside(&region) && !excluded.iter().any(inside) && offsets.insert(point.range.start()) {
            result.push(span_site(db, &point)?);
        }
    }
    Ok(result)
}
