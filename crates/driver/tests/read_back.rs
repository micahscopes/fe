use hir::analysis::semantic::GenericSubst;
use std::collections::HashMap;

use common::InputDb;
use dir_test::{Fixture, dir_test};
use fe_driver::{
    DriverDataBase,
    elaboration::{
        prepare,
        read_back::{ReadBackError, read_back},
    },
};
use hir::{
    HirDb,
    core::meta::{
        FrozenCode, FrozenCodeHole, FrozenCodeNode, FrozenMetaValue, FrozenQuoteSignatureType,
        FrozenTemplateSource,
    },
    hir_def::{
        Body, Cond, CondId, Expr, ExprId, FieldIndex, Func, Partial, Pat, PatId, Stmt, StmtId,
    },
};
use salsa::Setter;
use url::Url;

#[path = "support/read_back.rs"]
mod read_back_support;

#[path = "support/quote_round_trip.rs"]
mod quote_round_trip;

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_view", glob: "*.fe")]
fn fe_const_functions_inspect_checked_bodies(fixture: Fixture<&str>) {
    use hir::analysis::{
        semantic::{EvalOutcome, SemConstScalar, SemConstValue, eval_body_owner_const_with_args},
        ty::ty_check::BodyOwner,
    };
    let (db, url) = fixture_db(fixture);
    for name in [
        "polynomial",
        "bindings",
        "calls",
        "record",
        "control",
        "aggregates",
    ] {
        let value = fe_driver::elaboration::read_back::read_back_value(
            &db,
            named_function(&db, &url, name),
        )
        .unwrap_or_else(|error| panic!("{name} did not read back: {error:?}"));
        let consumer = named_function(&db, &url, &format!("verify_{name}"));
        let outcome = eval_body_owner_const_with_args(
            &db,
            BodyOwner::Func(consumer),
            GenericSubst::for_body_owner(&db, BodyOwner::Func(consumer), vec![]),
            vec![value],
        );
        let EvalOutcome::Ready(result) = outcome else {
            panic!("{name} inspection failed: {outcome:?}");
        };
        assert!(
            matches!(
                result.value(&db),
                SemConstValue::Scalar {
                    value: SemConstScalar::Bool(true),
                    ..
                }
            ),
            "{name} failed its Fe assertions"
        );
    }
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_view_rejected", glob: "*.fe")]
fn invalid_code_inspection_is_an_error(fixture: Fixture<&str>) {
    use hir::analysis::{
        semantic::{EvalOutcome, eval_body_owner_const_with_args},
        ty::ty_check::BodyOwner,
    };
    let expected = fixture
        .content()
        .lines()
        .find_map(|line| line.strip_prefix("// expect-error: "))
        .expect("fixture error expectation")
        .to_owned();
    let (db, url) = fixture_db(fixture);
    let value = fe_driver::elaboration::read_back::read_back_value(
        &db,
        named_function(&db, &url, "subject"),
    )
    .expect("subject reads back");
    let consumer = named_function(&db, &url, "reject");
    let outcome = eval_body_owner_const_with_args(
        &db,
        BodyOwner::Func(consumer),
        GenericSubst::for_body_owner(&db, BodyOwner::Func(consumer), vec![]),
        vec![value],
    );
    assert!(
        matches!(outcome, EvalOutcome::Failed(_)),
        "inspection must fail: {outcome:?}"
    );
    assert!(
        format!("{outcome:?}").contains(&expected),
        "expected {expected:?}, got {outcome:?}"
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_view_edits", glob: "initial.fe")]
fn code_inspection_tracks_edits_and_restoration(fixture: Fixture<&str>) {
    use hir::analysis::{
        semantic::{EvalOutcome, eval_body_owner_const_with_args},
        ty::ty_check::BodyOwner,
    };
    let url = Url::from_file_path(fixture.path()).expect("fixture URL");
    let evaluate = |db: &DriverDataBase| {
        let value = fe_driver::elaboration::read_back::read_back_value(
            db,
            named_function(db, &url, "subject"),
        )
        .expect("edited subject reads back");
        let consumer = named_function(db, &url, "inspect");
        let (errors, _) = hir::analysis::ty::ty_check::check_func_body(db, consumer);
        assert!(errors.is_empty(), "inspector must type check: {errors:?}");
        let outcome = eval_body_owner_const_with_args(
            db,
            BodyOwner::Func(consumer),
            GenericSubst::for_body_owner(db, BodyOwner::Func(consumer), vec![]),
            vec![value],
        );
        let EvalOutcome::Ready(result) = outcome else {
            panic!("edited inspection failed: {outcome:?}");
        };
        result.pretty_print(db)
    };
    let mut warm = DriverDataBase::default();
    for (source, expected) in [
        (*fixture.content(), "(true, 3, true)"),
        (
            include_str!("fixtures/read_back_view_edits/edited.fe"),
            "(false, 9, false)",
        ),
        (*fixture.content(), "(true, 3, true)"),
    ] {
        warm.workspace()
            .update(&mut warm, url.clone(), source.to_owned());
        let mut fresh = DriverDataBase::default();
        fresh
            .workspace()
            .update(&mut fresh, url.clone(), source.to_owned());
        assert_eq!(evaluate(&warm), expected);
        assert_eq!(evaluate(&fresh), expected);
    }
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_view_transport", glob: "*.fe")]
fn inspected_code_reuses_the_quotation_carrier(fixture: Fixture<&str>) {
    use fe_driver::elaboration::{construction::ConstructionError, invocations};
    use hir::analysis::{
        semantic::{
            CodeNodeSite, EvalOutcome, SemConstId, SemConstValue, StructuralHandle,
            eval_body_owner_const_with_args,
        },
        ty::ty_check::BodyOwner,
    };
    let (authored, url) = fixture_db(fixture);
    let subject = named_function(&authored, &url, "subject");
    let original = read_back(&authored, subject).expect("source body");
    let argument = fe_driver::elaboration::read_back::read_back_value(&authored, subject)
        .expect("source Code<Expr>");
    let evaluate = |name| {
        let consumer = named_function(&authored, &url, name);
        let (errors, _) = hir::analysis::ty::ty_check::check_func_body(&authored, consumer);
        assert!(errors.is_empty(), "{name} must type check: {errors:?}");
        let outcome = eval_body_owner_const_with_args(
            &authored,
            BodyOwner::Func(consumer),
            GenericSubst::for_body_owner(&authored, BodyOwner::Func(consumer), vec![]),
            vec![argument],
        );
        let EvalOutcome::Ready(value) = outcome else {
            panic!("{name} failed: {outcome:?}");
        };
        value
    };
    assert_eq!(
        evaluate("forward"),
        argument,
        "Fe keeps the same code value"
    );
    let file = authored
        .workspace()
        .get(&authored, &url)
        .expect("source file");
    let invocation = invocations(&authored, file)
        .expect("invocation")
        .pop()
        .expect("target");
    let input = invocation.record_input(&authored).expect("target input");
    let frozen = input
        .freeze_value(evaluate("publish"))
        .expect("checked code freezes");
    let cursor = evaluate("cursor");
    assert_eq!(
        input.freeze_value(cursor),
        Err(ConstructionError::Authority),
        "a read-only cursor cannot become generated code"
    );

    let &SemConstValue::Structural {
        ty,
        handle: StructuralHandle::CodeNode { code, .. },
    } = cursor.value(&authored)
    else {
        panic!("compiler-issued cursor");
    };
    let forged = SemConstId::new(
        &authored,
        SemConstValue::Structural {
            ty,
            handle: StructuralHandle::CodeNode {
                code,
                site: CodeNodeSite::Expr(ExprId::from_u32(u32::MAX - 1)),
            },
        },
    );
    let outcome = eval_body_owner_const_with_args(
        &authored,
        BodyOwner::Func(named_function(&authored, &url, "inspect_cursor")),
        GenericSubst::for_body_owner(
            &authored,
            BodyOwner::Func(named_function(&authored, &url, "inspect_cursor")),
            vec![],
        ),
        vec![forged],
    );
    assert!(
        matches!(outcome, EvalOutcome::Failed(_)),
        "forged cursor must error: {outcome:?}"
    );

    let mut output = prepare(&authored)
        .expect("generation")
        .expect("generated view");
    let module = HirDb::generated_module(&output, &url).expect("frozen generation");
    let mut requests = module.requests(&output).clone();
    assert_eq!(requests.len(), 1);
    requests[0].value = frozen;
    module.set_requests(&mut output).to(requests);
    let copied = checked_copied_body(&output, &url, "Fe-inspected checked body");
    assert_eq!(
        checked_body_shape(&authored, subject, &original.nodes[original.root]),
        checked_body_shape(
            &output,
            named_function(&output, &url, "copied"),
            &copied.nodes[copied.root]
        ),
    );
}

fn fixture_db(fixture: Fixture<&str>) -> (DriverDataBase, Url) {
    let url = Url::from_file_path(fixture.path()).expect("fixture URL");
    let mut db = DriverDataBase::default();
    db.workspace()
        .update(&mut db, url.clone(), fixture.content().to_string());
    (db, url)
}

#[test]
fn field_handles_cannot_be_spliced_as_patterns() {
    let source = r#"
mod author {
    use core::meta::{Code, Item, Record, field, variant}
    pub const fn emit(record: Record) -> Code<Item> {
        let f = field(variant(record, 0), 0)
        quote {
            fn generated(_ value: ${record}) -> bool {
                match value { ${f} => true, _ => false }
            }
        }
    }
}
#[generate(ingot::author::emit)]
enum Coin { Heads(u256) }
"#;
    let mut db = DriverDataBase::default();
    db.workspace().update(
        &mut db,
        Url::parse("file:///quote_pattern_wrong_kind.fe").unwrap(),
        source.to_string(),
    );
    let error = match prepare(&db) {
        Err(error) => error,
        Ok(_) => panic!("a Field cannot supply a pattern fragment"),
    };
    let rendered = error
        .rendered()
        .expect("provider checking renders its diagnostic");
    assert!(
        rendered.contains("this splice cannot stand for a pattern"),
        "{rendered}"
    );
    assert!(
        rendered.contains("expected `Code<Pat>`, found `Field`"),
        "{rendered}"
    );
}

fn named_function<'db>(db: &'db DriverDataBase, url: &Url, wanted: &str) -> Func<'db> {
    let file = db.workspace().get(db, url).expect("fixture file");
    db.top_mod(file)
        .all_funcs(db)
        .iter()
        .copied()
        .find(|func| {
            func.name(db)
                .to_opt()
                .is_some_and(|name| name.data(db) == wanted)
        })
        .unwrap_or_else(|| panic!("missing function {wanted}"))
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back",
    glob: "read_back_pure.fe",
)]
fn read_back_preserves_checked_binding_classes(fixture: Fixture<&str>) {
    let (db, url) = fixture_db(fixture);
    let literal = read_back(&db, named_function(&db, &url, "literal")).expect("literal body");
    let node = &literal.nodes[literal.root];
    assert_eq!(
        node.template.as_ref().unwrap().source,
        FrozenTemplateSource::CheckedBody
    );
    assert!(node.open_uses.is_empty());
    assert!(node.locals.is_empty());

    let params = read_back(&db, named_function(&db, &url, "params")).expect("parameter body");
    let mut indices = params.nodes[params.root]
        .open_uses
        .iter()
        .map(|use_| use_.index)
        .collect::<Vec<_>>();
    indices.sort();
    assert_eq!(indices, [0, 1]);

    let lets = read_back(&db, named_function(&db, &url, "lets")).expect("let body");
    let local_use_counts = lets.nodes[lets.root]
        .locals
        .iter()
        .map(|local| local.uses.len())
        .collect::<Vec<_>>();
    assert_eq!(local_use_counts, [1, 1]);

    let call = read_back(&db, named_function(&db, &url, "call")).expect("call body");
    let roots = &call.nodes[call.root].rooted_funcs;
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].path, "helper");

    let method =
        read_back(&db, named_function(&db, &url, "method_and_field")).expect("method body");
    assert_eq!(method.nodes[method.root].selected_methods.len(), 1);
    let record = read_back(&db, named_function(&db, &url, "record")).expect("record body");
    assert_eq!(record.nodes[record.root].record_types.len(), 1);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back",
    glob: "read_back_binders_roots.fe",
)]
fn read_back_keeps_local_and_root_identity(fixture: Fixture<&str>) {
    let (db, url) = fixture_db(fixture);
    let local = read_back(&db, named_function(&db, &url, "shadowing")).expect("local body");
    let local_node = &local.nodes[local.root];
    assert_eq!(local_node.locals.len(), 1);
    assert_eq!(local_node.locals[0].uses.len(), 1);
    assert!(local_node.rooted_funcs.is_empty());

    let rooted = read_back(&db, named_function(&db, &url, "rooted")).expect("rooted call");
    let roots = &rooted.nodes[rooted.root].rooted_funcs;
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].path, "lib::helper");
}

fn inject_read_back_body(output: &mut DriverDataBase, url: &Url, original: &FrozenCode) {
    let input = HirDb::generated_module(output, url).expect("frozen generation");
    let mut requests = input.requests(output).clone();
    let [request] = requests.as_mut_slice() else {
        panic!("expected one generation request");
    };
    let FrozenMetaValue::Code(code) = &mut request.value else {
        panic!("expected item quote");
    };
    let child = match code.nodes[code.root].holes.as_slice() {
        [FrozenCodeHole::Code(index, ..)] => *index,
        other => panic!("expected one expression fragment hole, got {other:?}"),
    };
    code.nodes[child] = original.nodes[original.root].clone();
    input.set_requests(output).to(requests);
}

fn checked_copied_body(output: &DriverDataBase, url: &Url, context: &str) -> FrozenCode {
    let file = output.workspace().get(output, url).expect("fixture file");
    let module = output.top_mod(file);
    let errors = hir::lower::generated_hir_errors(output, module);
    assert!(
        errors.is_empty(),
        "copied body did not lower in {context}: {errors:?}"
    );
    assert!(
        hir::lower::generated_quote_hygiene_errors(output, module).is_empty(),
        "copied body lost checked names"
    );
    let copied = named_function(output, url, "copied");
    let (diagnostics, _) = hir::analysis::ty::ty_check::check_func_body(output, copied);
    assert!(
        diagnostics.is_empty(),
        "copied body does not type check: {diagnostics:?}"
    );
    read_back(output, copied).expect("copied body reads back")
}

fn checked_body_shape<'db>(
    db: &'db DriverDataBase,
    func: Func<'db>,
    node: &FrozenCodeNode,
) -> String {
    let body = func.body(db).expect("checked body");
    fn expr<'db>(
        db: &'db DriverDataBase,
        body: Body<'db>,
        id: ExprId,
        node: &FrozenCodeNode,
        binders: &mut HashMap<PatId, usize>,
    ) -> String {
        match body.exprs(db)[id].unwrap_ref() {
            Expr::Lit(value) => format!("lit({})", value.pretty_print(db)),
            Expr::Path(_) => {
                if let Some(use_) = node.open_uses.iter().find(|use_| use_.site == id) {
                    return format!("param({})", use_.index);
                }
                if let Some(use_) = node.generic_uses.iter().find(|use_| use_.site == id) {
                    return format!("generic({})", use_.index);
                }
                if let Some(use_) = node.qualified_consts.iter().find(|use_| use_.site == id) {
                    let root = node
                        .rooted_traits
                        .iter()
                        .find(|root| root.site == use_.trait_site)
                        .expect("checked constant trait");
                    let Expr::Path(Partial::Present(path)) = body.exprs(db)[id].unwrap_ref() else {
                        panic!("checked constant path");
                    };
                    return format!(
                        "constant({:?},{:?},{})",
                        root.owner,
                        use_.receiver,
                        path.ident(db).unwrap().data(db)
                    );
                }
                for local in &node.locals {
                    if local.uses.contains(&id) {
                        let index = binders
                            .get(&local.binder)
                            .expect("local use follows its checked binder");
                        return format!("local({index})");
                    }
                }
                if let Some(root) = node.rooted_funcs.iter().find(|root| root.site == id) {
                    return format!("root({:?},{:?})", root.owner, root.type_arguments);
                }
                panic!("checked path lacks a read-back classification");
            }
            Expr::Bin(lhs, rhs, op) => format!(
                "bin({op:?},{},{})",
                expr(db, body, *lhs, node, binders),
                expr(db, body, *rhs, node, binders)
            ),
            Expr::Un(value, op) => format!("un({op:?},{})", expr(db, body, *value, node, binders)),
            Expr::Assign(lhs, rhs) => format!(
                "assign({},{})",
                expr(db, body, *lhs, node, binders),
                expr(db, body, *rhs, node, binders)
            ),
            Expr::AugAssign(lhs, rhs, op) => format!(
                "augassign({op:?},{},{})",
                expr(db, body, *lhs, node, binders),
                expr(db, body, *rhs, node, binders)
            ),
            Expr::Cast(value, _) => {
                let (_, ty) = node
                    .cast_primitives
                    .iter()
                    .find(|(site, _)| *site == id)
                    .expect("checked cast type");
                format!("cast({ty:?},{})", expr(db, body, *value, node, binders))
            }
            Expr::Call(callee, args) => format!(
                "call({},{:?})",
                expr(db, body, *callee, node, binders),
                args.iter()
                    .map(|arg| (
                        arg.label_eagerly(db, body)
                            .map(|label| label.data(db).clone()),
                        expr(db, body, arg.expr, node, binders)
                    ))
                    .collect::<Vec<_>>()
            ),
            Expr::RecordInit(_, fields) => {
                let (_, ty) = node
                    .record_types
                    .iter()
                    .find(|(site, _)| *site == id)
                    .expect("checked record constructor");
                format!(
                    "record({ty:?},{:?})",
                    fields
                        .iter()
                        .map(|field| (
                            field
                                .label_eagerly(db, body)
                                .map(|label| label.data(db).clone()),
                            expr(db, body, field.expr, node, binders),
                        ))
                        .collect::<Vec<_>>()
                )
            }
            Expr::MethodCall(receiver, _, _, args) => {
                let (_, selected) = node
                    .selected_methods
                    .iter()
                    .find(|(site, _)| *site == id)
                    .expect("checked method selection");
                format!(
                    "method({selected:?},{},{:?})",
                    expr(db, body, *receiver, node, binders),
                    args.iter()
                        .map(|arg| (
                            arg.label_eagerly(db, body)
                                .map(|label| label.data(db).clone()),
                            expr(db, body, arg.expr, node, binders),
                        ))
                        .collect::<Vec<_>>()
                )
            }
            Expr::Field(base, field) => {
                let field = match field.unwrap_ref() {
                    FieldIndex::Ident(name) => name.data(db).clone(),
                    FieldIndex::Index(index) => index.data(db).to_string(),
                    FieldIndex::Hole(_) => panic!("unexpanded field hole"),
                };
                format!("field({},{field})", expr(db, body, *base, node, binders))
            }
            Expr::Tuple(values) => format!(
                "tuple({:?})",
                values
                    .iter()
                    .map(|value| expr(db, body, *value, node, binders))
                    .collect::<Vec<_>>()
            ),
            Expr::Array(values) => format!(
                "array({:?})",
                values
                    .iter()
                    .map(|value| expr(db, body, *value, node, binders))
                    .collect::<Vec<_>>()
            ),
            Expr::If(condition, then, otherwise) => format!(
                "if({},{},{:?})",
                cond(db, body, *condition, node, binders),
                expr(db, body, *then, node, binders),
                otherwise.map(|value| expr(db, body, value, node, binders))
            ),
            Expr::Match(scrutinee, Partial::Present(arms)) => format!(
                "match({},{:?})",
                expr(db, body, *scrutinee, node, binders),
                arms.iter()
                    .map(|arm| (
                        pat(db, body, arm.pat(), node, binders),
                        expr(db, body, arm.body(), node, binders)
                    ))
                    .collect::<Vec<_>>()
            ),
            Expr::Block(statements) => format!(
                "block({:?})",
                statements
                    .iter()
                    .map(|statement| stmt(db, body, *statement, node, binders))
                    .collect::<Vec<_>>()
            ),
            other => panic!("body shape does not yet cover {other:?}"),
        }
    }
    fn cond<'db>(
        db: &'db DriverDataBase,
        body: Body<'db>,
        id: CondId,
        node: &FrozenCodeNode,
        binders: &mut HashMap<PatId, usize>,
    ) -> String {
        match body.conds(db)[id].unwrap_ref() {
            Cond::Expr(value) => expr(db, body, *value, node, binders),
            Cond::Bin(lhs, rhs, op) => format!(
                "condition({op:?},{},{})",
                cond(db, body, *lhs, node, binders),
                cond(db, body, *rhs, node, binders)
            ),
            other => panic!("body shape does not yet cover {other:?}"),
        }
    }
    fn stmt<'db>(
        db: &'db DriverDataBase,
        body: Body<'db>,
        id: StmtId,
        node: &FrozenCodeNode,
        binders: &mut HashMap<PatId, usize>,
    ) -> String {
        match body.stmts(db)[id].unwrap_ref() {
            Stmt::Let(pattern, ty, value) => {
                let annotation = ty.map(|_| {
                    &node
                        .local_types
                        .iter()
                        .find(|local| local.stmt == id)
                        .expect("checked local annotation")
                        .ty
                });
                let pattern = pat(db, body, *pattern, node, binders);
                format!(
                    "let({pattern},{annotation:?},{:?})",
                    value.map(|value| expr(db, body, value, node, binders))
                )
            }
            Stmt::Expr(value) => format!("stmt({})", expr(db, body, *value, node, binders)),
            Stmt::Return(value) => format!(
                "return({:?})",
                value.map(|value| expr(db, body, value, node, binders))
            ),
            Stmt::While(condition, block) => format!(
                "while({},{})",
                cond(db, body, *condition, node, binders),
                expr(db, body, *block, node, binders)
            ),
            Stmt::For(pattern, iterable, block, unroll) => {
                let iterable = expr(db, body, *iterable, node, binders);
                let pattern = pat(db, body, *pattern, node, binders);
                format!(
                    "for({pattern},{iterable},{unroll:?},{})",
                    expr(db, body, *block, node, binders)
                )
            }
            Stmt::Break => "break".into(),
            Stmt::Continue => "continue".into(),
        }
    }
    fn pat<'db>(
        db: &'db DriverDataBase,
        body: Body<'db>,
        id: PatId,
        node: &FrozenCodeNode,
        binders: &mut HashMap<PatId, usize>,
    ) -> String {
        match body.pats(db)[id].unwrap_ref() {
            Pat::Path(_, mutable) => {
                assert!(
                    node.locals.iter().any(|local| local.binder == id),
                    "checked local binder"
                );
                let index = binders.len();
                assert!(binders.insert(id, index).is_none(), "unique local binder");
                format!("binder({index},{mutable})")
            }
            Pat::Tuple(parts) => format!(
                "tuplepat({:?})",
                parts
                    .iter()
                    .map(|part| pat(db, body, *part, node, binders))
                    .collect::<Vec<_>>()
            ),
            Pat::WildCard => "wildcard".into(),
            Pat::Rest => "rest".into(),
            Pat::Lit(Partial::Present(literal)) => {
                format!("patternlit({})", literal.pretty_print(db))
            }
            Pat::Or(lhs, rhs) => format!(
                "orpat({},{})",
                pat(db, body, *lhs, node, binders),
                pat(db, body, *rhs, node, binders)
            ),
            other => panic!("body shape does not yet cover {other:?}"),
        }
    }
    let mut root = body.expr(db);
    while let Expr::Block(statements) = body.exprs(db)[root].unwrap_ref() {
        let [statement] = statements.as_slice() else {
            break;
        };
        let Stmt::Expr(value) = body.stmts(db)[*statement].unwrap_ref() else {
            break;
        };
        root = *value;
    }
    expr(db, body, root, node, &mut HashMap::new())
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back",
    glob: "read_back_pure.fe",
)]
fn checked_body_fragment_round_trips_through_generation(fixture: Fixture<&str>) {
    let copied = transplanted_fixture(&fixture, "read_back_runtime/read_back_pure.fe");
    let original = &copied
        .bodies
        .iter()
        .find(|(name, _)| name == "lets")
        .unwrap()
        .1;
    let reflected = read_back(
        &copied.output,
        read_back_support::named_function(&copied.output, &copied.url, "copied_lets"),
    )
    .expect("copied let body reads back");
    let before = &original.nodes[original.root];
    let after = &reflected.nodes[reflected.root];
    assert_eq!(before.open_uses.len(), after.open_uses.len());
    assert_eq!(
        before
            .locals
            .iter()
            .map(|local| local.uses.len())
            .collect::<Vec<_>>(),
        after
            .locals
            .iter()
            .map(|local| local.uses.len())
            .collect::<Vec<_>>(),
    );
    assert_eq!(before.rooted_funcs.len(), after.rooted_funcs.len());
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back",
    glob: "read_back_pure.fe",
)]
fn pure_first_order_bodies_round_trip(fixture: Fixture<&str>) {
    assert_structural_round_trips(
        fixture,
        "read_back_runtime/read_back_pure.fe",
        &[
            "literal",
            "params",
            "unary_ops",
            "cast",
            "call",
            "method_and_field",
            "tuple",
            "record",
            "block_and_if",
        ],
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_loops",
    glob: "*.fe",
)]
fn structured_loops_round_trip(fixture: Fixture<&str>) {
    assert_structural_round_trips(
        fixture,
        "read_back_runtime/read_back_loops.fe",
        &["assignment", "nested_while", "early_exits", "array_for"],
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_parameters",
    glob: "*.fe",
)]
fn checked_parameter_identity_survives_reordered_names(fixture: Fixture<&str>) {
    assert_structural_round_trips(
        fixture,
        "read_back_adapters/parameters_reordered.fe",
        &["ordered"],
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_parameters",
    glob: "*.fe",
)]
fn checked_parameter_identity_survives_new_names(fixture: Fixture<&str>) {
    assert_structural_round_trips(
        fixture,
        "read_back_adapters/parameters_fresh.fe",
        &["ordered"],
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_parameters_missing",
    glob: "*.fe",
)]
fn missing_checked_parameter_is_rejected(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let original = read_back(&authored, named_function(&authored, &url, "ordered"))
        .expect("source parameters read back");
    let mut output = fe_driver::elaboration::prepare(&authored)
        .expect("generation succeeds")
        .expect("generation request exists");
    inject_read_back_body(&mut output, &url, &original);
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let errors = hir::lower::generated_hir_errors(&output, output.top_mod(file));
    assert!(
        errors.iter().any(|error| error
            .failure
            .to_string()
            .contains("read-back parameter has no destination binding")),
        "missing parameter must reject during transplantation: {errors:?}"
    );
}

fn transplanted_fixture(
    fixture: &Fixture<&str>,
    adapter: &str,
) -> read_back_support::TransplantedFixture {
    let adapter = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(adapter);
    read_back_support::transplant_fixture(fixture.path(), fixture.content(), &adapter)
}

fn assert_structural_round_trips(fixture: Fixture<&str>, adapter: &str, cases: &[&str]) {
    let copied = transplanted_fixture(&fixture, adapter);
    for &name in cases {
        let original = &copied
            .bodies
            .iter()
            .find(|(body, _)| body == name)
            .unwrap()
            .1;
        let source = read_back_support::named_function(&copied.authored, &copied.url, name);
        let destination = read_back_support::named_function(
            &copied.output,
            &copied.url,
            &format!("copied_{name}"),
        );
        let reflected = read_back(&copied.output, destination).expect("copied body reads back");
        let before = &original.nodes[original.root];
        let after = &reflected.nodes[reflected.root];
        assert_eq!(
            checked_body_shape(&copied.authored, source, before),
            checked_body_shape(&copied.output, destination, after),
            "{name}"
        );
        assert_eq!(before.open_uses.len(), after.open_uses.len(), "{name}");
        assert_eq!(before.locals.len(), after.locals.len(), "{name}");
        assert_eq!(
            before
                .local_types
                .iter()
                .map(|local| &local.ty)
                .collect::<Vec<_>>(),
            after
                .local_types
                .iter()
                .map(|local| &local.ty)
                .collect::<Vec<_>>(),
            "{name}"
        );
        assert_eq!(
            before
                .rooted_funcs
                .iter()
                .map(|root| &root.owner)
                .collect::<Vec<_>>(),
            after
                .rooted_funcs
                .iter()
                .map(|root| &root.owner)
                .collect::<Vec<_>>(),
            "{name}"
        );
        assert_eq!(
            before
                .selected_methods
                .iter()
                .map(|(_, owner)| owner)
                .collect::<Vec<_>>(),
            after
                .selected_methods
                .iter()
                .map(|(_, owner)| owner)
                .collect::<Vec<_>>(),
            "{name}"
        );
        assert_eq!(
            before
                .record_types
                .iter()
                .map(|(_, ty)| ty)
                .collect::<Vec<_>>(),
            after
                .record_types
                .iter()
                .map(|(_, ty)| ty)
                .collect::<Vec<_>>(),
            "{name}"
        );
        assert_eq!(
            before
                .cast_primitives
                .iter()
                .map(|(_, primitive)| primitive)
                .collect::<Vec<_>>(),
            after
                .cast_primitives
                .iter()
                .map(|(_, primitive)| primitive)
                .collect::<Vec<_>>(),
            "{name}"
        );
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back",
    glob: "read_back_binders_roots.fe",
)]
fn checked_root_survives_conflicting_destination_name(fixture: Fixture<&str>) {
    let copied = transplanted_fixture(&fixture, "read_back_runtime/read_back_binders_roots.fe");
    let original = &copied
        .bodies
        .iter()
        .find(|(name, _)| name == "rooted")
        .unwrap()
        .1;
    let reflected = read_back(
        &copied.output,
        read_back_support::named_function(&copied.output, &copied.url, "copied_rooted"),
    )
    .expect("copied rooted call reads back");
    let roots = &reflected.nodes[reflected.root].rooted_funcs;
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0].path, "lib::helper");
    assert_eq!(
        roots[0].owner,
        original.nodes[original.root].rooted_funcs[0].owner,
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_cast",
    glob: "read_back_cast_shadow.fe",
)]
fn checked_cast_survives_conflicting_destination_alias(fixture: Fixture<&str>) {
    let (original, reflected) = round_trip_in_shadowing_destination(&fixture, "cast_alias");
    let before = &original.nodes[original.root];
    let after = &reflected.nodes[reflected.root];
    assert_eq!(before.cast_primitives.len(), 1);
    assert_eq!(after.cast_primitives.len(), 1);
    assert_eq!(before.cast_primitives[0].1, after.cast_primitives[0].1);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_cast",
    glob: "read_back_cast_shadow.fe",
)]
fn checked_local_annotation_survives_conflicting_destination_alias(fixture: Fixture<&str>) {
    let (original, reflected) = round_trip_in_shadowing_destination(&fixture, "annotated_alias");
    let before = &original.nodes[original.root];
    let after = &reflected.nodes[reflected.root];
    assert_eq!(before.local_types.len(), 1);
    assert_eq!(after.local_types.len(), 1);
    assert_eq!(before.local_types[0].ty, after.local_types[0].ty);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_records",
    glob: "*.fe",
)]
fn checked_records_and_annotations_survive_destination_shadowing(fixture: Fixture<&str>) {
    for name in [
        "record_scalar",
        "record_generic",
        "annotated_tuple",
        "annotated_unit",
    ] {
        let (before, after) = round_trip_in_shadowing_destination(&fixture, name);
        let before = &before.nodes[before.root];
        let after = &after.nodes[after.root];
        assert_eq!(before.local_types[0].ty, after.local_types[0].ty, "{name}");
        if name.starts_with("record_") {
            assert_eq!(before.record_types.len(), 1, "{name}");
            assert_eq!(before.record_types[0].1, after.record_types[0].1, "{name}");
            let FrozenQuoteSignatureType::Nominal {
                root, arguments, ..
            } = &before.record_types[0].1
            else {
                panic!("record must retain nominal identity");
            };
            assert!(root.path.starts_with("source::"));
            if name == "record_generic" {
                assert_eq!(
                    arguments,
                    &[FrozenQuoteSignatureType::Tuple(vec![
                        FrozenQuoteSignatureType::Primitive(
                            hir::analysis::ty::ty_def::PrimTy::U256
                        ),
                        FrozenQuoteSignatureType::Primitive(
                            hir::analysis::ty::ty_def::PrimTy::U256
                        ),
                    ])]
                );
            }
        }
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_methods",
    glob: "*.fe",
)]
fn checked_method_calls_survive_conflicting_destination_traits(fixture: Fixture<&str>) {
    for name in ["through_inherent", "through_trait"] {
        let (before, after) = round_trip_in_shadowing_destination(&fixture, name);
        let before = &before.nodes[before.root];
        let after = &after.nodes[after.root];
        assert_eq!(before.selected_methods.len(), 1);
        assert_eq!(before.selected_methods[0].1, after.selected_methods[0].1);
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "pending/quote_call_[!a]*.fe",
)]
fn quoted_non_copy_method_receivers_use_the_checked_call_record(fixture: Fixture<&str>) {
    // Alias signatures and associated-const Self paths have separate pending fixes.
    let (authored, url) = fixture_db(fixture);
    let output = fe_driver::elaboration::prepare(&authored)
        .expect("provider succeeds")
        .expect("generation request exists");
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let module = output.top_mod(file);
    let errors = hir::lower::generated_hir_errors(&output, module);
    assert!(
        errors.is_empty(),
        "quoted method body must lower: {errors:?}"
    );
    let errors = hir::lower::generated_quote_hygiene_errors(&output, module);
    assert!(
        errors.is_empty(),
        "ordinary checked receiver must retain meaning: {errors:?}"
    );
    for func in module.all_funcs(&output) {
        let (diagnostics, _) = hir::analysis::ty::ty_check::check_func_body(&output, *func);
        assert!(
            diagnostics.is_empty(),
            "method consumer must check: {diagnostics:?}"
        );
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_methods",
    glob: "*.fe",
)]
fn checked_method_owner_cannot_be_replaced_by_destination_trait(fixture: Fixture<&str>) {
    let (authored, url) = shadowing_destination(&fixture);
    let mut original = read_back(&authored, named_function(&authored, &url, "through_trait"))
        .expect("source method reads back");
    let destination = read_back(&authored, named_function(&authored, &url, "ordinary"))
        .expect("destination method reads back");
    assert_ne!(
        original.nodes[original.root].selected_methods[0].1,
        destination.nodes[destination.root].selected_methods[0].1
    );
    original.nodes[original.root].selected_methods[0].1 = destination.nodes[destination.root]
        .selected_methods[0]
        .1
        .clone();
    let mut output = fe_driver::elaboration::prepare(&authored)
        .expect("generation succeeds")
        .expect("generation request exists");
    inject_read_back_body(&mut output, &url, &original);
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let errors = hir::lower::generated_quote_hygiene_errors(&output, output.top_mod(file));
    assert!(
        errors.iter().any(|error| error.failure
            == hir::lower::GeneratedQuoteFailure::Hygiene(
                hir::lower::QuoteHygieneFault::MethodCallChangedSelectionInOutput,
            )),
        "forged method declaration must fail owner validation: {errors:?}"
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_generic_calls", glob: "*.fe")]
fn explicit_call_types_survive_destination_shadowing(fixture: Fixture<&str>) {
    for function in ["call_primitive", "call_nominal"] {
        let (original, copied) = round_trip_in_shadowing_destination(&fixture, function);
        let arguments = |code: FrozenCode| {
            code.nodes[code.root]
                .rooted_funcs
                .iter()
                .map(|root| {
                    root.type_arguments
                        .clone()
                        .expect("explicit call arguments")
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(arguments(original), arguments(copied), "{function}");
    }
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_generic_calls", glob: "*.fe")]
fn call_type_argument_cannot_keep_another_nominal_owners_authority(fixture: Fixture<&str>) {
    let (authored, url) = shadowing_destination(&fixture);
    let mut original = read_back(&authored, named_function(&authored, &url, "call_nominal"))
        .expect("authored call reads back");
    let arguments = original.nodes[original.root].rooted_funcs[0]
        .type_arguments
        .as_mut()
        .expect("explicit call arguments");
    let FrozenQuoteSignatureType::Nominal { root, .. } = &mut arguments[0] else {
        panic!("expected nominal argument");
    };
    root.path = "dest::Wrapped".into();
    let mut output = prepare(&authored)
        .expect("generation succeeds")
        .expect("generation request exists");
    inject_read_back_body(&mut output, &url, &original);
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let errors = hir::lower::generated_quote_hygiene_errors(&output, output.top_mod(file));
    assert!(
        errors.iter().any(|error| error.failure
            == hir::lower::GeneratedQuoteFailure::Hygiene(
                hir::lower::QuoteHygieneFault::NominalTypeChangedMeaningInOutput,
            )),
        "conflicting nominal argument must fail owner validation: {errors:?}"
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_methods",
    glob: "*.fe",
)]
fn missing_checked_method_selection_is_rejected(fixture: Fixture<&str>) {
    let (authored, url) = shadowing_destination(&fixture);
    let mut original = read_back(&authored, named_function(&authored, &url, "through_trait"))
        .expect("source method reads back");
    original.nodes[original.root].selected_methods.clear();
    let mut output = fe_driver::elaboration::prepare(&authored)
        .expect("generation succeeds")
        .expect("generation request exists");
    inject_read_back_body(&mut output, &url, &original);
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let errors = hir::lower::generated_hir_errors(&output, output.top_mod(file));
    assert!(
        errors.iter().any(|error| error
            .failure
            .to_string()
            .contains("method has no checked selection")),
        "missing method selection must fail closed: {errors:?}"
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_cast",
    glob: "read_back_cast_shadow.fe",
)]
fn unsupported_local_annotation_is_explicit(fixture: Fixture<&str>) {
    let (db, url) = fixture_db(fixture);
    assert_eq!(
        read_back(&db, named_function(&db, &url, "unsupported_const_array")),
        Err(ReadBackError::UnsupportedType),
    );
}

fn round_trip_in_shadowing_destination(
    fixture: &Fixture<&str>,
    function: &str,
) -> (FrozenCode, FrozenCode) {
    let (authored, url) = shadowing_destination(fixture);
    let original = read_back(&authored, named_function(&authored, &url, function))
        .expect("authored body reads back");
    let mut output = fe_driver::elaboration::prepare(&authored)
        .expect("generation succeeds")
        .expect("generation request exists");
    inject_read_back_body(&mut output, &url, &original);
    let reflected = checked_copied_body(&output, &url, function);
    assert_eq!(
        checked_body_shape(
            &authored,
            named_function(&authored, &url, function),
            &original.nodes[original.root]
        ),
        checked_body_shape(
            &output,
            named_function(&output, &url, "copied"),
            &reflected.nodes[reflected.root]
        ),
        "{function}"
    );
    (original, reflected)
}

fn shadowing_destination(fixture: &Fixture<&str>) -> (DriverDataBase, Url) {
    let url = Url::from_file_path(fixture.path()).expect("fixture URL");
    let source = fixture.content().replace(
        "mod dest {\n",
        &format!(
            "mod dest {{\n{}",
            include_str!("fixtures/read_back_adapters/cast_target.fe")
        ),
    );
    assert_ne!(source, *fixture.content(), "destination module is missing");
    let provider = include_str!("fixtures/read_back_adapters/cast_provider.fe");
    let mut authored = DriverDataBase::default();
    authored
        .workspace()
        .update(&mut authored, url.clone(), format!("{source}\n{provider}"));
    (authored, url)
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_records",
    glob: "*.fe",
)]
fn conflicting_record_path_cannot_keep_another_owners_authority(fixture: Fixture<&str>) {
    let (authored, url) = shadowing_destination(&fixture);
    let mut original = read_back(&authored, named_function(&authored, &url, "record_scalar"))
        .expect("authored record reads back");
    let (_, FrozenQuoteSignatureType::Nominal { root, .. }) =
        &mut original.nodes[original.root].record_types[0]
    else {
        panic!("expected nominal record");
    };
    root.path = "dest::Point".into();
    let mut output = fe_driver::elaboration::prepare(&authored)
        .expect("generation succeeds")
        .expect("generation request exists");
    inject_read_back_body(&mut output, &url, &original);
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let module = output.top_mod(file);
    let errors = hir::lower::generated_quote_hygiene_errors(&output, module);
    assert!(
        errors.iter().any(|error| error.failure
            == hir::lower::GeneratedQuoteFailure::Hygiene(
                hir::lower::QuoteHygieneFault::NominalTypeChangedMeaningInOutput,
            )),
        "conflicting nominal root must fail owner validation: {errors:?}"
    );
}

fn assert_read_back_unsupported(fixture: Fixture<&str>) {
    let (db, url) = fixture_db(fixture);
    assert_eq!(
        read_back(&db, named_function(&db, &url, "bump")),
        Err(ReadBackError::Effectful),
    );
    read_back(&db, named_function(&db, &url, "pure_twin")).expect("pure twin");
}

fn assert_read_back_round_trip(fixture: Fixture<&str>) {
    let adapter = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/read_back_runtime")
        .join(
            std::path::Path::new(fixture.path())
                .file_name()
                .expect("fixture file name"),
        );
    let copied = read_back_support::transplant_fixture(fixture.path(), fixture.content(), &adapter);
    for (name, original) in &copied.bodies {
        let source = read_back_support::named_function(&copied.authored, &copied.url, name);
        let destination = read_back_support::named_function(
            &copied.output,
            &copied.url,
            &format!("copied_{name}"),
        );
        let reflected = read_back(&copied.output, destination).expect("copied body reads back");
        assert_eq!(
            checked_body_shape(&copied.authored, source, &original.nodes[original.root]),
            checked_body_shape(
                &copied.output,
                destination,
                &reflected.nodes[reflected.root]
            ),
            "{name} changed structure"
        );
    }
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back", glob: "*.fe")]
fn read_back_round_trip(fixture: Fixture<&str>) {
    assert_read_back_round_trip(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_matches", glob: "*.fe")]
fn ordinary_match_bodies_round_trip(fixture: Fixture<&str>) {
    assert_read_back_round_trip(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_enums", glob: "*.fe")]
fn enum_bodies_round_trip(fixture: Fixture<&str>) {
    let adapter = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/read_back_runtime/enums.fe");
    let copied = read_back_support::transplant_fixture(fixture.path(), fixture.content(), &adapter);
    assert_eq!(copied.bodies.len(), 2);
    quote_round_trip::assert_round_trip(&copied.authored, &copied.output, &copied.url);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_struct_patterns", glob: "*.fe")]
fn struct_pattern_bodies_round_trip(fixture: Fixture<&str>) {
    let adapter = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/read_back_runtime/struct_patterns.fe");
    let copied = read_back_support::transplant_fixture(fixture.path(), fixture.content(), &adapter);
    assert_eq!(copied.bodies.len(), 2);
    quote_round_trip::assert_round_trip(&copied.authored, &copied.output, &copied.url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_match_body.fe",
)]
fn ordinary_quoted_match_is_checked(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("match quote generates")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_checked_roots.fe",
)]
fn quoted_enum_patterns_preserve_checked_owners(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("enum quote generates without changing pattern owners")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_target.fe",
)]
fn quoted_enum_targets_are_checked(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("enum target quote generates")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_reflection.fe",
)]
fn quoted_enum_reflection_is_checked(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("enum reflection quote generates")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_arm_composition.fe",
)]
fn quoted_long_arm_composition_is_checked(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("enum binder quote generates")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "pending/quote_string_*.fe",
)]
#[dir_test_attr(#[ignore = "pre-RD86 string splice transport is pending"])]
fn quoted_string_splices_are_checked(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("string splice generates")
        .unwrap();
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_binder_hygiene.fe",
)]
fn quoted_enum_destination_constants_preserve_binders(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("hygienic enum quote generates")
        .unwrap();
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_generic.fe",
)]
fn quoted_enum_payload_where_bounds_are_checked(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("enum payload where bounds generate")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_generic.fe",
)]
fn enum_payload_where_comparison_rejects_changed_predicate(fixture: Fixture<&str>) {
    use hir::core::meta::{FrozenQuoteFieldType, FrozenQuoteWherePredicate};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("payload bounds generate")
        .unwrap();
    quote_round_trip::assert_round_trip(&authored, &output, &url);
    let input = HirDb::generated_module(&output, &url).unwrap();
    for change in ["parameter", "trait", "missing"] {
        let mut expected = input.requests(&output).clone();
        let FrozenMetaValue::Code(code) = &mut expected[0].value else {
            panic!("code result");
        };
        let index = code
            .nodes
            .iter()
            .position(|node| {
                node.where_predicates.iter().any(|predicate| {
                    matches!(predicate, Some(FrozenQuoteWherePredicate::Bound { .. }))
                })
            })
            .expect("payload predicate node");
        let node = &mut code.nodes[index];
        match change {
            "parameter" => {
                let slot = node
                    .where_predicates
                    .iter()
                    .find_map(|predicate| match predicate {
                        Some(FrozenQuoteWherePredicate::Bound {
                            ty: FrozenQuoteSignatureType::Hole(slot),
                            ..
                        }) => Some(*slot),
                        _ => None,
                    })
                    .expect("payload type hole");
                let FrozenCodeHole::Value(_, Some(FrozenQuoteFieldType::TargetParam(parameter))) =
                    &mut node.holes[slot as usize]
                else {
                    panic!("target parameter hole");
                };
                *parameter = 1;
            }
            "trait" => node.rooted_traits[0].owner.path.push_str("::wrong_trait"),
            "missing" => *node = FrozenCodeNode::empty(None),
            _ => unreachable!(),
        }
        let failure = catch_unwind(AssertUnwindSafe(|| {
            quote_round_trip::assert_round_trip_against(&authored, &output, &url, &expected);
        }))
        .expect_err("changed predicate must not compare equal");
        let message = failure
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| failure.downcast_ref::<&str>().copied())
            .unwrap_or("");
        let reason = match change {
            "parameter" => "impl where payload parameter index",
            "trait" => "impl where trait identity",
            _ => "impl where predicate count",
        };
        assert!(message.contains(reason), "{change}: {message}");
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_eq.fe",
)]
fn quoted_enum_arm_fold_is_checked(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("enum binder quote generates")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_eq.fe",
)]
fn enum_arm_fold_comparison_rejects_missing_arm(fixture: Fixture<&str>) {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored).expect("arm quote generates").unwrap();
    quote_round_trip::assert_round_trip(&authored, &output, &url);
    let input = HirDb::generated_module(&output, &url).unwrap();
    let mut expected = input.requests(&output).clone();
    let FrozenMetaValue::Code(code) = &mut expected[0].value else {
        panic!("arm fixture emits code");
    };
    let (index, previous) = code
        .nodes
        .iter()
        .enumerate()
        .rev()
        .find_map(|(index, node)| {
            let [
                FrozenCodeHole::Code(previous, ..),
                FrozenCodeHole::Code(lhs, ..),
                ..,
            ] = node.holes.as_slice()
            else {
                return None;
            };
            code.nodes[*lhs]
                .variant_binding
                .as_ref()
                .filter(|binding| binding.field.is_none() && binding.variant_index == 3)
                .map(|_| (index, *previous))
        })
        .expect("last folded arm");
    code.nodes[index] = code.nodes[previous].clone();
    let failure = catch_unwind(AssertUnwindSafe(|| {
        quote_round_trip::assert_round_trip_against(&authored, &output, &url, &expected);
    }))
    .expect_err("omitted final arm must not compare equal");
    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(message.contains("arm list length changed"), "{message}");
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_generic_binders.fe",
)]
fn quoted_generic_enum_group_binders_are_checked(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("enum binder quote generates")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_binders.fe",
)]
fn quoted_enum_group_binders_are_checked(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("enum binder quote generates")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_binders.fe",
)]
fn enum_group_binders_comparison_rejects_changed_coordinates(fixture: Fixture<&str>) {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored).expect("binder quote generates").unwrap();
    quote_round_trip::assert_round_trip(&authored, &output, &url);
    let input = HirDb::generated_module(&output, &url).unwrap();
    for change in ["owner", "variant", "group", "field"] {
        let mut expected = input.requests(&output).clone();
        let FrozenMetaValue::Code(code) = &mut expected[0].value else {
            panic!("binder fixture emits code");
        };
        let binding = code
            .nodes
            .iter_mut()
            .filter_map(|node| node.variant_binding.as_mut())
            .find(|binding| {
                binding.variant_index == 0
                    && binding.group == 0
                    && binding.field.is_some() == matches!(change, "group" | "field")
            })
            .expect("selected compiler-issued coordinate");
        match change {
            "owner" => binding.owner.owner.path.push_str("::wrong_owner"),
            "variant" => binding.variant_index = 2,
            "group" => binding.group = 1,
            "field" => binding.field = Some(1 - binding.field.unwrap()),
            _ => unreachable!(),
        }
        let failure = catch_unwind(AssertUnwindSafe(|| {
            quote_round_trip::assert_round_trip_against(&authored, &output, &url, &expected);
        }))
        .expect_err("changed binder coordinate must not compare equal");
        let message = failure
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| failure.downcast_ref::<&str>().copied())
            .unwrap_or("");
        let reason = match change {
            "owner" => "binder pattern owner identity",
            "variant" => "binder variant index",
            _ => "selected payload field must use its checked binder identity",
        };
        assert!(message.contains(reason), "{change}: {message}");
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_struct_checked_roots.fe",
)]
fn quoted_struct_patterns_preserve_checked_owners(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("struct pattern quote generates")
        .unwrap();
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_struct_checked_roots.fe",
)]
fn struct_comparison_rejects_changed_owner(fixture: Fixture<&str>) {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored).expect("struct quote generates").unwrap();
    quote_round_trip::assert_round_trip(&authored, &output, &url);
    let input = HirDb::generated_module(&output, &url).unwrap();
    for pattern in [true, false] {
        let mut expected = input.requests(&output).clone();
        let FrozenMetaValue::Code(code) = &mut expected[0].value else {
            panic!("struct quote uses code");
        };
        let root = code
            .nodes
            .iter_mut()
            .flat_map(|node| &mut node.rooted_constructors)
            .find(|root| {
                root.variant_index.is_none()
                    && matches!(
                        root.site,
                        hir::core::meta::FrozenQuoteConstructorSite::Pat(_)
                    ) == pattern
            })
            .expect("checked struct root");
        let (module, _) = root
            .owner
            .owner
            .path
            .rsplit_once("::")
            .expect("nominal owner path");
        root.owner.owner.path = format!("{module}::destination::Point");
        let failure = catch_unwind(AssertUnwindSafe(|| {
            quote_round_trip::assert_round_trip_against(&authored, &output, &url, &expected);
        }))
        .expect_err("different nominal owner must not compare equal");
        let message = failure
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| failure.downcast_ref::<&str>().copied())
            .unwrap_or("");
        assert!(
            message.contains("selected constructor owner identity"),
            "wrong failure: {message}"
        );
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_checked_roots.fe",
)]
fn enum_comparison_rejects_changed_owner_and_index(fixture: Fixture<&str>) {
    use hir::core::meta::FrozenQuoteConstructorSite;
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored).expect("enum quote generates").unwrap();
    quote_round_trip::assert_round_trip(&authored, &output, &url);
    let input = HirDb::generated_module(&output, &url).unwrap();
    for pattern in [true, false] {
        for change_owner in [true, false] {
            let mut expected = input.requests(&output).clone();
            let FrozenMetaValue::Code(code) = &mut expected[0].value else {
                panic!("enum quote uses the code carrier");
            };
            let root = code
                .nodes
                .iter_mut()
                .flat_map(|node| &mut node.rooted_constructors)
                .find(|root| matches!(root.site, FrozenQuoteConstructorSite::Pat(_)) == pattern)
                .expect("both pattern and constructor enum roots");
            let diagnostic = if change_owner {
                root.owner.owner.path.push_str("::destination");
                "selected constructor owner identity"
            } else {
                root.variant_index = Some((root.variant_index.unwrap() + 1) % 3);
                "selected variant index"
            };
            let failure = catch_unwind(AssertUnwindSafe(|| {
                quote_round_trip::assert_round_trip_against(&authored, &output, &url, &expected);
            }))
            .expect_err("comparison must reject changed enum identity");
            let message = failure
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| failure.downcast_ref::<&str>().copied())
                .unwrap_or("");
            assert!(
                message.contains(diagnostic),
                "wrong comparison failure: {message}"
            );
        }
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_enum_checked_roots.fe",
)]
fn forged_enum_roots_are_rejected(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    for missing_owner in [true, false] {
        let mut output = prepare(&authored).expect("enum quote generates").unwrap();
        let input = HirDb::generated_module(&output, &url).unwrap();
        let mut requests = input.requests(&output).clone();
        let FrozenMetaValue::Code(code) = &mut requests[0].value else {
            panic!("enum quote uses code");
        };
        let root = code
            .nodes
            .iter_mut()
            .flat_map(|node| &mut node.rooted_constructors)
            .next()
            .expect("retained enum roots");
        let diagnostic = if missing_owner {
            root.owner.owner.path.push_str("::Missing");
            "quoted constructor owner is absent"
        } else {
            root.variant_index = Some(u16::MAX);
            "quoted variant index is outside"
        };
        input.set_requests(&mut output).to(requests);
        let file = output.workspace().get(&output, &url).unwrap();
        let errors = hir::lower::generated_hir_errors(&output, output.top_mod(file));
        assert!(
            errors
                .iter()
                .any(|error| error.failure.to_string().contains(diagnostic)),
            "forged root must return a generation diagnostic: {errors:?}"
        );
    }
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_bound_constants", glob: "*.fe")]
fn checked_const_binders_and_traits_round_trip(fixture: Fixture<&str>) {
    assert_read_back_round_trip(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_symbolic", glob: "*.fe")]
fn checked_symbolic_types_round_trip(fixture: Fixture<&str>) {
    assert_read_back_round_trip(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_symbolic", glob: "*.fe")]
fn symbolic_types_cannot_capture_a_missing_destination_binder(fixture: Fixture<&str>) {
    let adapter = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/read_back_runtime/symbolic.fe");
    for (provider, forged, expected) in [
        (
            "read_back_annotation",
            FrozenQuoteSignatureType::OwnerGeneric(0),
            "owner generic has no destination binding",
        ),
        (
            "read_back_owner_call",
            FrozenQuoteSignatureType::FunctionGeneric(1),
            "function generic index is missing",
        ),
        (
            "read_back_owner_call",
            FrozenQuoteSignatureType::OwnerGeneric(2),
            "owner generic has no destination binding",
        ),
    ] {
        let mut copied =
            read_back_support::transplant_fixture(fixture.path(), fixture.content(), &adapter);
        let input =
            HirDb::generated_module(&copied.output, &copied.url).expect("frozen generation");
        let mut requests = input.requests(&copied.output).clone();
        let request = requests
            .iter_mut()
            .find(|request| request.provider.named_path().unwrap().ends_with(provider))
            .unwrap();
        let FrozenMetaValue::Code(code) = &mut request.value else {
            panic!("item quote");
        };
        let child = code.nodes[code.root]
            .holes
            .iter()
            .find_map(|hole| match hole {
                FrozenCodeHole::Code(child, ..) => Some(*child),
                FrozenCodeHole::Value(_, _) => None,
            })
            .expect("body fragment");
        if provider == "read_back_annotation" {
            code.nodes[child].local_types[0].ty = forged;
        } else {
            code.nodes[child].rooted_funcs[0]
                .type_arguments
                .as_mut()
                .unwrap()[0] = forged;
        }
        input.set_requests(&mut copied.output).to(requests);
        let file = copied
            .output
            .workspace()
            .get(&copied.output, &copied.url)
            .unwrap();
        let errors = hir::lower::generated_hir_errors(&copied.output, copied.output.top_mod(file));
        assert!(
            errors
                .iter()
                .any(|error| error.failure.to_string().contains(expected)),
            "{provider}: {errors:?}"
        );
    }
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_bound_constants", glob: "*.fe")]
fn fe_read_back_distinguishes_const_binders_and_constants(fixture: Fixture<&str>) {
    use hir::analysis::semantic::{
        EvalOutcome, SemConstScalar, SemConstValue, eval_body_owner_const_with_args,
    };
    use hir::analysis::ty::ty_check::BodyOwner;
    let (db, url) = fixture_db(fixture);
    for (subject, visitor) in [
        ("digits", "verify_generic_code"),
        ("qualified", "verify_constant_code"),
    ] {
        let value = fe_driver::elaboration::read_back::read_back_value(
            &db,
            named_function(&db, &url, subject),
        )
        .expect("checked constant body");
        let result = eval_body_owner_const_with_args(
            &db,
            BodyOwner::Func(named_function(&db, &url, visitor)),
            GenericSubst::for_body_owner(
                &db,
                BodyOwner::Func(named_function(&db, &url, visitor)),
                vec![],
            ),
            vec![value],
        );
        let EvalOutcome::Ready(value) = result else {
            panic!("{subject}: {result:?}");
        };
        assert!(matches!(
            value.value(&db),
            SemConstValue::Scalar {
                value: SemConstScalar::Bool(true),
                ..
            }
        ));
    }
}

fn bound_constant_output(fixture: &Fixture<&str>) -> read_back_support::TransplantedFixture {
    let adapter = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/read_back_runtime/bound_constants.fe");
    read_back_support::transplant_fixture(fixture.path(), fixture.content(), &adapter)
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_bound_constants", glob: "*.fe")]
fn missing_const_generic_destination_binding_rejects(fixture: Fixture<&str>) {
    let mut copied = bound_constant_output(&fixture);
    let input = HirDb::generated_module(&copied.output, &copied.url).expect("frozen generation");
    let mut requests = input.requests(&copied.output).clone();
    let request = requests
        .iter_mut()
        .find(|request| {
            request
                .provider
                .named_path()
                .unwrap()
                .ends_with("read_back_digits")
        })
        .unwrap();
    let FrozenMetaValue::Code(code) = &mut request.value else {
        panic!("item quote");
    };
    let FrozenCodeHole::Code(child, ..) = code.nodes[code.root].holes[0] else {
        panic!("body fragment");
    };
    code.nodes[child].generic_uses[0].index = 2;
    input.set_requests(&mut copied.output).to(requests);
    let file = copied
        .output
        .workspace()
        .get(&copied.output, &copied.url)
        .unwrap();
    let errors = hir::lower::generated_hir_errors(&copied.output, copied.output.top_mod(file));
    assert!(
        errors.iter().any(|error| error
            .failure
            .to_string()
            .contains("generic has no destination binding")),
        "{errors:?}"
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_bound_constants", glob: "*.fe")]
fn qualified_const_cannot_borrow_another_traits_authority(fixture: Fixture<&str>) {
    let mut copied = bound_constant_output(&fixture);
    let input = HirDb::generated_module(&copied.output, &copied.url).expect("frozen generation");
    let mut requests = input.requests(&copied.output).clone();
    let request = requests
        .iter_mut()
        .find(|request| {
            request
                .provider
                .named_path()
                .unwrap()
                .ends_with("read_back_qualified")
        })
        .unwrap();
    let FrozenMetaValue::Code(code) = &mut request.value else {
        panic!("item quote");
    };
    let FrozenCodeHole::Code(child, ..) = code.nodes[code.root].holes[0] else {
        panic!("body fragment");
    };
    code.nodes[child].rooted_traits[0].path = "HasValue".into();
    input.set_requests(&mut copied.output).to(requests);
    let file = copied
        .output
        .workspace()
        .get(&copied.output, &copied.url)
        .unwrap();
    let errors =
        hir::lower::generated_quote_hygiene_errors(&copied.output, copied.output.top_mod(file));
    assert!(
        errors.iter().any(|error| error
            .failure
            .to_string()
            .contains("qualified constant changed trait")),
        "{errors:?}"
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_unsupported",
    glob: "*.fe",
)]
fn read_back_unsupported(fixture: Fixture<&str>) {
    assert_read_back_unsupported(fixture);
}

fn quotation_corpus_output(
    fixture: Fixture<&str>,
) -> Option<(DriverDataBase, DriverDataBase, Url)> {
    let snapshot =
        std::fs::read_to_string(std::path::Path::new(fixture.path()).with_extension("snap"))
            .expect("existing quotation CLI oracle");
    let (authored, url) = fixture_db(fixture);
    let output = match prepare(&authored) {
        Err(error) => {
            assert!(
                snapshot.contains("=== EXIT CODE: 1 ==="),
                "unexpected rejection: {error:?}"
            );
            let start = snapshot
                .find("error[")
                .expect("rejected fixture names its diagnostic");
            let end = snapshot[start..].find(']').expect("diagnostic code ends") + start + 1;
            let actual = error
                .rendered()
                .clone()
                .unwrap_or_else(|| format!("error[18-{:04}]: {}", error.code(), error.message()));
            assert!(
                actual.contains(&snapshot[start..end]),
                "wrong rejection: {actual}"
            );
            println!("expected rejection, no checked output");
            return None;
        }
        Ok(None) => {
            let file = authored
                .workspace()
                .get(&authored, &url)
                .expect("source file");
            assert!(
                fe_driver::elaboration::invocations(&authored, file)
                    .expect("source invocations")
                    .is_empty()
            );
            println!("no generation request or checked output");
            return None;
        }
        Ok(Some(output)) => output,
    };
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let module = output.top_mod(file);
    let errors = hir::lower::generated_hir_errors(&output, module);
    assert!(errors.is_empty(), "generated HIR errors: {errors:?}");
    let errors = hir::lower::generated_quote_hygiene_errors(&output, module);
    assert!(errors.is_empty(), "generated hygiene errors: {errors:?}");
    Some((authored, output, url))
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_*.fe",
)]
fn quoted_output_bodies_read_back(fixture: Fixture<&str>) {
    use hir::span::{DesugaredOrigin, HirOrigin};
    let Some((_, output, url)) = quotation_corpus_output(fixture) else {
        return;
    };
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let module = output.top_mod(file);
    let mut failures = Vec::new();
    let mut bodies = 0;
    for function in module.all_funcs(&output) {
        if !matches!(
            function.origin(&output),
            HirOrigin::Desugared(DesugaredOrigin::Generation(_))
        ) {
            continue;
        }
        let name = function.pretty_print_signature(&output);
        if function.has_effects(&output) {
            assert_eq!(
                read_back(&output, *function),
                Err(ReadBackError::Effectful),
                "{name}"
            );
            continue;
        }
        bodies += 1;
        if let Err(error) = read_back(&output, *function) {
            failures.push(format!("{name}: {error:?}"));
        }
    }
    println!("{bodies} generated bodies inspected");
    assert!(
        failures.is_empty(),
        "quoted-body admission gaps: {failures:#?}"
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_*.fe",
)]
fn quoted_outputs_round_trip(fixture: Fixture<&str>) {
    let Some((authored, output, url)) = quotation_corpus_output(fixture) else {
        return;
    };
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_scalar_splice.fe",
)]
fn quote_comparison_detects_changed_scalar(fixture: Fixture<&str>) {
    assert_quote_comparison_detects_changed_body(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_local_binder.fe",
)]
fn quote_comparison_detects_changed_binding(fixture: Fixture<&str>) {
    assert_quote_comparison_detects_changed_body(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_provider_root.fe",
)]
fn quote_comparison_detects_changed_function(fixture: Fixture<&str>) {
    assert_quote_comparison_detects_changed_body(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/fe_test_runner",
    glob: "quote_derived_clone_struct.fe",
)]
fn quote_comparison_detects_changed_field_authority(fixture: Fixture<&str>) {
    use hir::{
        analysis::ty::adt_def::AdtRef,
        hir_def::{FieldParent, ItemKind, scope_graph::ScopeId},
    };
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let (authored, output, url) = quotation_corpus_output(fixture).expect("checked quote");
    quote_round_trip::assert_round_trip(&authored, &output, &url);
    let file = output.workspace().get(&output, &url).unwrap();
    let grants = output
        .top_mod(file)
        .all_funcs(&output)
        .iter()
        .filter_map(|function| function.body(&output))
        .flat_map(|body| {
            body.quote_scopes(&output)
                .iter()
                .flat_map(|(_, scope)| &scope.fields)
        })
        .collect::<Vec<_>>();
    let ScopeId::Field(FieldParent::Struct(owner), index) =
        grants.first().expect("issued grants").field
    else {
        panic!("struct grant");
    };
    // Same-named fields on a different owner must not satisfy the oracle.
    let other = output
        .top_mod(file)
        .scope()
        .children(&output)
        .find_map(|scope| {
            if let ScopeId::Item(ItemKind::Struct(other)) = scope
                && other != owner
            {
                Some(other)
            } else {
                None
            }
        })
        .expect("lookalike owner");
    for (actual, mismatch) in [
        (
            ScopeId::Field(FieldParent::Struct(owner), index + 1),
            "issued field index",
        ),
        (
            ScopeId::Field(FieldParent::Struct(other), index),
            "issued field owner identity",
        ),
    ] {
        let failure = catch_unwind(AssertUnwindSafe(|| {
            quote_round_trip::assert_field_grant(
                &output,
                &output,
                AdtRef::Struct(owner),
                usize::from(index),
                actual,
            );
        }))
        .expect_err("comparison rejects a changed grant coordinate");
        let message = failure
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| failure.downcast_ref::<&str>().copied())
            .unwrap();
        assert!(message.contains(mismatch), "{message}");
    }
}

fn assert_quote_comparison_detects_changed_body(fixture: Fixture<&str>) {
    use hir::analysis::semantic::SemConstScalar;
    use hir::span::{DesugaredOrigin, HirOrigin};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    let case = std::path::Path::new(fixture.path())
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let (authored, mut output, url) = if case == "distinct_captures" {
        let url = Url::from_file_path(fixture.path()).unwrap();
        let authored = field_authority_fixture_db(fixture);
        let output = prepare(&authored)
            .expect("checked open captures")
            .expect("generation");
        (authored, output, url)
    } else {
        quotation_corpus_output(fixture).expect("checked quote")
    };
    let input = HirDb::generated_module(&output, &url).expect("frozen generation");
    let expected = input.requests(&output).clone();
    let mut changed = expected.clone();
    assert_eq!(changed.len(), 1);
    let FrozenMetaValue::Code(code) = &mut changed[0].value else {
        panic!("fixture emits code");
    };
    let node = &mut code.nodes[code.root];
    let mismatch = match case.as_str() {
        "distinct_captures" => {
            let holes = node
                .holes
                .iter_mut()
                .filter_map(|hole| {
                    if let FrozenCodeHole::Code(child, _) = hole {
                        Some(child)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            let mut holes = holes.into_iter();
            let first = holes.next().expect("first fragment");
            let second = holes.next().expect("second fragment");
            std::mem::swap(first, second);
            "open name must capture the nearest visible local identity"
        }
        "quote_scalar_splice" => {
            let integer = node
                .holes
                .iter_mut()
                .find_map(|hole| match hole {
                    FrozenCodeHole::Value(value, _) => match value.as_mut() {
                        FrozenMetaValue::Scalar {
                            value: SemConstScalar::Int { value },
                            ..
                        } => Some(value),
                        _ => None,
                    },
                    _ => None,
                })
                .expect("integer splice");
            *integer -= 1u8;
            "left: \"42\""
        }
        "quote_local_binder" => {
            let (outer, rest) = node.locals.split_first_mut().expect("outer local");
            let inner = rest.last_mut().expect("second outer-scope local");
            std::mem::swap(&mut outer.uses, &mut inner.uses);
            "local use retains its binder"
        }
        "quote_provider_root" => {
            let root = node.rooted_funcs.first_mut().expect("rooted helper");
            let file = authored.workspace().get(&authored, &url).unwrap();
            let other = authored
                .top_mod(file)
                .all_funcs(&authored)
                .iter()
                .find_map(|function| {
                    let locator = function.frozen_locator(&authored)?;
                    (function.name(&authored).unwrap_ref().data(&authored) == "bump"
                        && locator != root.owner)
                        .then_some(locator)
                })
                .expect("different helper with the same signature");
            root.path = other
                .named_path()
                .expect("named helper")
                .strip_prefix(&format!(
                    "{}::",
                    authored
                        .top_mod(file)
                        .scope()
                        .pretty_path(&authored)
                        .unwrap()
                ))
                .expect("fixture helper belongs to its source module")
                .to_owned();
            root.owner = other;
            "selected function identity"
        }
        _ => panic!("unknown comparison counterexample"),
    };
    input.set_requests(&mut output).to(changed);
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    let errors = hir::lower::generated_quote_hygiene_errors(&output, module);
    assert!(
        errors.is_empty(),
        "changed quote still passes hygiene: {errors:?}"
    );
    for function in module.all_funcs(&output) {
        if matches!(
            function.origin(&output),
            HirOrigin::Desugared(DesugaredOrigin::Generation(_))
        ) {
            let (errors, _) = hir::analysis::ty::ty_check::check_func_body(&output, *function);
            assert!(
                errors.is_empty(),
                "changed body remains well typed: {errors:?}"
            );
        }
    }
    let failure = catch_unwind(AssertUnwindSafe(|| {
        quote_round_trip::assert_round_trip_against(&authored, &output, &url, &expected);
    }))
    .expect_err("comparison must reject a valid body with different meaning");
    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied())
        .expect("comparison assertion message");
    assert!(message.contains(mismatch), "wrong mismatch: {message}");
}

#[test]
fn quoted_arm_lists_retain_holes_and_check_their_kind() {
    use hir::hir_def::{MatchArm, QuoteFragment, QuoteHolePosition, QuoteKind};
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///quote_arm_transport.fe").unwrap();
    db.workspace().update(
        &mut db,
        url.clone(),
        r#"
use core::meta::{Arm, Code, empty}
pub const fn arms() -> Code<Arm> {
    let prior = empty<Arm>()
    quote { ${prior}
        true => 1,
        false => 0,
    }
}
"#
        .to_owned(),
    );
    let func = named_function(&db, &url, "arms");
    let (diagnostics, _) = hir::analysis::ty::ty_check::check_func_body(&db, func);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let body = func.body(&db).unwrap();
    let quote = body
        .exprs(&db)
        .iter()
        .find_map(|(_, expr)| match expr {
            Partial::Present(Expr::Quote(quote)) => Some(quote),
            _ => None,
        })
        .expect("arm quote retained");
    assert_eq!(quote.syntactic_kind(&db), Some(QuoteKind::Arm));
    let QuoteFragment::Arms(arms) = &quote.fragment else {
        panic!("arm fragment")
    };
    assert_eq!(arms.len(), 3);
    assert!(matches!(arms[0], MatchArm::Hole(0)));
    assert!(arms[1..].iter().all(|arm| arm.parts().is_some()));
    assert_eq!(quote.holes.len(), 1);
    assert_eq!(
        quote.holes[0].position,
        QuoteHolePosition::Code(hir::hir_def::QuoteKind::Arm)
    );
}

#[test]
fn expression_fragments_cannot_supply_match_arm_lists() {
    let mut db = DriverDataBase::default();
    db.workspace().update(
        &mut db,
        Url::parse("file:///quote_arm_wrong_kind.fe").unwrap(),
        r#"
mod author {
    use core::meta::{Code, Expr, Item, Record}
    pub const fn emit(record: Record) -> Code<Item> {
        let not_arms: Code<Expr> = quote { true }
        quote {
            fn generated(_ value: ${record}) -> bool {
                match value { ${not_arms} }
            }
        }
    }
}
#[generate(ingot::author::emit)]
enum Coin { Heads, Tails }
"#
        .to_owned(),
    );
    let error = match prepare(&db) {
        Err(error) => error,
        Ok(_) => panic!("wrong arm kind accepted"),
    };
    let rendered = error.rendered().expect("rendered provider diagnostic");
    assert!(
        rendered.contains("this splice cannot stand for match arms"),
        "{rendered}"
    );
    assert!(
        rendered.contains("expected `Code<Arm>`, found `Code<Expr>`"),
        "{rendered}"
    );
}

#[test]
fn quoted_multifile_trait_keeps_its_module() {
    for (generated, twin) in [
        (
            include_str!("fixtures/quote_multifile/impl_where.fe"),
            include_str!("fixtures/quote_multifile/impl_where_twin.fe"),
        ),
        (
            include_str!("fixtures/quote_multifile/generic_bound.fe"),
            include_str!("fixtures/quote_multifile/generic_bound_twin.fe"),
        ),
        (
            include_str!("fixtures/quote_multifile/method_where.fe"),
            include_str!("fixtures/quote_multifile/method_where_twin.fe"),
        ),
    ] {
        for (source, generated) in [(generated, true), (twin, false)] {
            let mut authored = DriverDataBase::default();
            for (path, source) in [
                (
                    "file:///project/fe.toml",
                    "name = \"quote_multifile\"\nversion = \"0.1.0\"\n",
                ),
                ("file:///project/src/lib.fe", source),
                (
                    "file:///project/src/sub.fe",
                    include_str!("fixtures/quote_multifile/sub.fe"),
                ),
            ] {
                authored.workspace().update(
                    &mut authored,
                    Url::parse(path).unwrap(),
                    source.to_string(),
                );
            }
            let message = if generated {
                match prepare(&authored) {
                    Err(error) => error.message(),
                    Ok(_) => panic!("sub::Mark must not bind root Mark"),
                }
            } else {
                let url = Url::parse("file:///project/src/lib.fe").unwrap();
                let file = authored.workspace().get(&authored, &url).unwrap();
                let diagnostics = authored.run_on_ingot(authored.top_mod(file).ingot(&authored));
                assert!(
                    diagnostics.has_errors(&authored),
                    "handwritten bound must fail"
                );
                diagnostics.format_diags(&authored)
            };
            assert!(message.contains("error[6-0003]"), "{message}");
            assert!(message.contains("Mark"), "{message}");
        }
    }
}

#[test]
fn quoted_multifile_helper_keeps_nested_modules() {
    use hir::analysis::{
        semantic::{EvalOutcome, eval_body_owner_const_with_args},
        ty::ty_check::BodyOwner,
    };
    for external in [false, true] {
        let mut authored = DriverDataBase::default();
        for (path, source) in [
            (
                "file:///project/fe.toml",
                "name = \"quote_multifile\"\nversion = \"0.1.0\"\n",
            ),
            (
                "file:///project/src/lib.fe",
                r#"
#[generate(ingot::outer::author::emit)]
pub struct Target {}
const fn helper() -> u256 { 99 }
"#,
            ),
            ("file:///project/src/outer.fe", ""),
            (
                "file:///project/src/outer/author.fe",
                r#"
use core::meta::{Code, Item, Record}
pub struct Token {}
use ingot::outer::author::{Token as Renamed, helper as selected}
const fn helper(_ token: Token) -> u256 { 7 }
pub const fn emit(_ record: Record) -> Code<Item> {
    quote { pub const fn generated() -> u256 {
        let token: Renamed = Token {}
        selected(token)
    } }
}
"#,
            ),
        ] {
            let path = if external && path.contains("/src/outer") {
                path.replace("/project/", "/policy/")
            } else {
                path.to_owned()
            };
            let source = if external && path.ends_with("/project/src/lib.fe") {
                source.replace("ingot::outer::author::emit", "policy::outer::author::emit")
            } else {
                source.to_owned()
            };
            authored
                .workspace()
                .update(&mut authored, Url::parse(&path).unwrap(), source);
        }
        if external {
            for (path, source) in [
                (
                    "file:///policy/fe.toml",
                    "name = \"policy\"\nversion = \"0.1.0\"\n",
                ),
                ("file:///policy/src/lib.fe", ""),
            ] {
                authored.workspace().update(
                    &mut authored,
                    Url::parse(path).unwrap(),
                    source.to_string(),
                );
            }
            authored.dependency_graph().add_dependency(
                &mut authored,
                &Url::parse("file:///project/").unwrap(),
                &Url::parse("file:///policy/").unwrap(),
                "policy".into(),
                common::dependencies::DependencyArguments::default(),
            );
        }
        let output = prepare(&authored)
            .expect("nested helper quote elaborates")
            .expect("generation");
        let url = Url::parse("file:///project/src/lib.fe").unwrap();
        let function = named_function(&output, &url, "generated");
        let value = eval_body_owner_const_with_args(
            &output,
            BodyOwner::Func(function),
            GenericSubst::for_body_owner(&output, BodyOwner::Func(function), vec![]),
            vec![],
        );
        let EvalOutcome::Ready(value) = value else {
            panic!("generated helper evaluates: {value:?}")
        };
        assert_eq!(value.pretty_print(&output), "7");
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_field_authority",
    glob: "*.fe",
)]
fn quoted_fields_require_provider_authority(fixture: Fixture<&str>) {
    let snapshot_path = fixture.path().to_owned();
    let authored = field_authority_fixture_db(fixture);
    let error = match prepare(&authored) {
        Err(error) => error,
        Ok(_) => {
            panic!("generated fields must retain provider visibility and issued owner authority")
        }
    };
    assert!(
        error.message().contains("visible")
            || error.message().contains("field") && error.message().contains("owner"),
        "{}",
        error.message()
    );
    let rendered = error
        .rendered()
        .expect("field authority error is rendered at its authored site");
    assert!(rendered.contains("generated here"), "{rendered}");
    test_utils::snap_test!(rendered, &snapshot_path);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_field_authority/allowed",
    glob: "*.fe",
)]
fn quoted_provider_private_fields_remain_accessible(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).expect("fixture path");
    let authored = field_authority_fixture_db(fixture);
    let output = prepare(&authored)
        .expect("provider's own private field authority survives generation")
        .expect("generation");
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

fn field_authority_fixture_db(fixture: Fixture<&str>) -> DriverDataBase {
    use hir::analysis::analysis_pass::{ModuleAnalysisPass, ParsingPass};
    let (db, url) = fixture_db(fixture);
    let file = db.workspace().get(&db, &url).unwrap();
    let diagnostics = ParsingPass {}.run_on_module(&db, db.top_mod(file));
    assert!(
        diagnostics.is_empty(),
        "{}",
        db.format_complete_diagnostics(
            &diagnostics
                .iter()
                .map(|diag| diag.to_complete(&db))
                .collect::<Vec<_>>()
        )
    );
    drop(diagnostics);
    db
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_field_authority/unsupported",
    glob: "*.fe",
)]
fn quoted_pattern_field_labels_are_not_yet_syntax(fixture: Fixture<&str>) {
    use hir::analysis::analysis_pass::{ModuleAnalysisPass, ParsingPass};
    let (db, url) = fixture_db(fixture);
    let file = db.workspace().get(&db, &url).unwrap();
    let diagnostics = ParsingPass {}.run_on_module(&db, db.top_mod(file));
    assert!(
        !diagnostics.is_empty(),
        "field-label holes in record patterns require frontend support"
    );
    let rendered = db.format_complete_diagnostics(
        &diagnostics
            .iter()
            .map(|diag| diag.to_complete(&db))
            .collect::<Vec<_>>(),
    );
    assert!(
        rendered.contains("while parsing record pattern field list"),
        "{rendered}"
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_fold_limits",
    glob: "*.fe",
)]
fn deep_quote_folds_report_limits_without_stack_overflow(fixture: Fixture<&str>) {
    let snapshot_path = fixture.path().to_owned();
    let authored = field_authority_fixture_db(fixture);
    let error = match prepare(&authored) {
        Err(error) => error,
        Ok(_) => panic!("over-limit quote fold was admitted"),
    };
    let rendered = error.render(&authored);
    assert!(rendered.contains("limit"), "{rendered}");
    assert!(!rendered.contains("panicked"), "{rendered}");
    test_utils::snap_test!(rendered, &snapshot_path);
}

#[test]
fn quoted_fold_boundaries_remain_normal_compile_results() {
    // Item folds just below the frozen-description cap must not reach a
    // second unbounded recursion in item emission.
    let items = include_str!("fixtures/quote_fold_limits/items.fe");
    for count in [1000, 2000, 2700] {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///item_fold_boundary.fe").unwrap();
        db.workspace()
            .update(&mut db, url, items.replace("5000", &count.to_string()));
        let error = match prepare(&db) {
            Err(error) => error,
            Ok(_) => panic!("repeated identical helper declarations require a diagnostic"),
        };
        assert!(error.render(&db).contains("error["));
    }
    let expression = include_str!("fixtures/quote_fold_limits/expression.fe");
    for count in [16, 63] {
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///expression_fold_boundary.fe").unwrap();
        db.workspace()
            .update(&mut db, url, expression.replace("1000", &count.to_string()));
        prepare(&db)
            .expect("expression fold below the nesting cap stays valid")
            .expect("generation");
    }
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_request_budget",
    glob: "*.fe",
)]
fn generation_request_has_one_node_budget(fixture: Fixture<&str>) {
    let snapshot_path = fixture.path().to_owned();
    let authored = field_authority_fixture_db(fixture);
    let error = match prepare(&authored) {
        Err(error) => error,
        Ok(_) => panic!("over-budget generation request was admitted before publication"),
    };
    assert_eq!(
        error.code(),
        4,
        "request budget is a typed output-construction failure"
    );
    let rendered = error.render(&authored);
    assert!(
        rendered.contains("quotation request node limit"),
        "{}",
        error.message()
    );
    test_utils::snap_test!(rendered, &snapshot_path);
}

#[test]
fn request_budget_counts_shared_edges_and_discards_partial_output() {
    use hir::span::{DesugaredOrigin, HirOrigin};
    let source = include_str!("fixtures/quote_request_budget/repeated_items_20.fe")
        .replace("while i < 20", "while i < 1");
    let mut authored = DriverDataBase::default();
    let url = Url::parse("file:///request_budget_graph.fe").unwrap();
    authored
        .workspace()
        .update(&mut authored, url.clone(), source);
    let mut output = prepare(&authored)
        .expect("one item fits")
        .expect("generation");
    let input = HirDb::generated_module(&output, &url).expect("frozen request");
    let mut requests = input.requests(&output).clone();
    let FrozenMetaValue::Code(code) = &mut requests[0].value else {
        panic!("item code")
    };
    let template = code.nodes[code.root].clone();
    assert_eq!(template.holes.len(), 2);
    for _ in 0..40 {
        let mut parent = template.clone();
        for hole in &mut parent.holes {
            let FrozenCodeHole::Code(child, _) = hole else {
                panic!("item edge")
            };
            *child = code.root;
        }
        code.root = code.nodes.len();
        code.nodes.push(parent);
    }
    input.set_requests(&mut output).to(requests);
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    let errors = hir::lower::generated_hir_errors(&output, module);
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].failure,
        hir::lower::GeneratedQuoteFailure::RequestNodeLimit {
            actual: 65_537,
            maximum: 65_536,
        }
    );
    assert!(
        module.all_funcs(&output).iter().all(|function| !matches!(
            function.origin(&output),
            HirOrigin::Desugared(DesugaredOrigin::Generation(_))
        )),
        "failed expansion must discard every partially emitted declaration"
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_request_budget/limitations",
    glob: "*.fe",
)]
fn splice_only_empty_item_composition_fails_closed(fixture: Fixture<&str>) {
    let authored = field_authority_fixture_db(fixture);
    let error = match prepare(&authored) {
        Err(error) => error,
        Ok(_) => panic!("splice-only empty item syntax requires checked kind reification"),
    };
    assert!(
        error.message().contains("generation requires item code"),
        "{}",
        error.message()
    );
}

#[test]
fn quotation_node_budget_is_separate_for_each_request() {
    let mut source = String::from(
        "mod author { use core::meta::{Code, Item, Record}\npub const fn emit(_ record: Record) -> Code<Item> { quote {\n",
    );
    let expression = std::iter::repeat_n("1", 200)
        .collect::<Vec<_>>()
        .join(" + ");
    // Each request copies over 40,000 expression/statement nodes, so their
    // combined work exceeds the request limit without any single body doing so.
    for i in 0..100 {
        source.push_str(&format!("pub fn f{i}() -> u256 {{ {expression} }}\n"));
    }
    source.push_str("} } }\nmod left {\n#[generate(ingot::author::emit)]\nstruct L {}\n}\nmod right {\n#[generate(ingot::author::emit)]\nstruct R {}\n}\n");
    let mut authored = DriverDataBase::default();
    let url = Url::parse("file:///separate_request_budgets.fe").unwrap();
    authored
        .workspace()
        .update(&mut authored, url.clone(), source);
    let output = prepare(&authored)
        .expect("two individually bounded requests fit")
        .expect("generation");
    let requests = HirDb::generated_module(&output, &url)
        .unwrap()
        .requests(&output);
    assert_eq!(requests.len(), 2);
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_open_scope/allowed",
    glob: "*.fe",
)]
fn quoted_open_names_bind_the_nearest_local(fixture: Fixture<&str>) {
    use hir::analysis::{
        semantic::{
            EvalOutcome, SemConstId, SemConstScalar, SemConstValue, eval_body_owner_const_with_args,
        },
        ty::{ty_check::BodyOwner, ty_def::TyId},
    };
    let url = Url::from_file_path(fixture.path()).unwrap();
    let authored = field_authority_fixture_db(fixture);
    let output = prepare(&authored)
        .expect("nearest lexical binder, including match arms, elaborates")
        .expect("generation");
    quote_round_trip::assert_round_trip(&authored, &output, &url);
    let generated = named_function(&output, &url, "gen");
    let twin = named_function(&output, &url, "twin");
    assert_eq!(generated.is_const(&output), twin.is_const(&output));
    if !generated.is_const(&output) {
        // Sequence iteration calls ordinary len/get methods. Its value oracle
        // runs in quoted_open_captures_execute_at_o0_and_o2 instead of CTFE.
        return;
    }
    let argument = SemConstId::new(
        &output,
        SemConstValue::Scalar {
            ty: TyId::u256(&output),
            value: SemConstScalar::Int { value: 2.into() },
        },
    );
    let evaluate = |name| {
        let outcome = eval_body_owner_const_with_args(
            &output,
            BodyOwner::Func(named_function(&output, &url, name)),
            GenericSubst::for_body_owner(
                &output,
                BodyOwner::Func(named_function(&output, &url, name)),
                vec![],
            ),
            vec![argument],
        );
        let EvalOutcome::Ready(value) = outcome else {
            panic!("{name}: {outcome:?}")
        };
        value.pretty_print(&output)
    };
    assert_eq!(
        evaluate("gen"),
        evaluate("twin"),
        "constant open capture must match its ordinary lexical twin"
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_open_scope/rejected",
    glob: "*.fe",
)]
fn quoted_open_arm_names_do_not_skip_local_types(fixture: Fixture<&str>) {
    let authored = field_authority_fixture_db(fixture);
    let Err(error) = prepare(&authored) else {
        panic!("nearest local type must reject this enum pattern");
    };
    assert_eq!(
        error.code(),
        5,
        "ordinary checked-source diagnostic: {error:?}"
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_open_scope/allowed",
    glob: "distinct_captures.fe",
)]
fn quoted_open_comparison_detects_changed_capture(fixture: Fixture<&str>) {
    assert_quote_comparison_detects_changed_body(fixture);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_fold_limits/allowed",
    glob: "*.fe",
)]
fn realistic_quote_derives_fit_the_limits(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).unwrap();
    let authored = field_authority_fixture_db(fixture);
    let output = prepare(&authored)
        .expect("realistic field-wise derives fit")
        .expect("generation");
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_fold_limits/function",
    glob: "*.fe.in",
)]
fn quoted_function_copy_has_its_own_node_limit(fixture: Fixture<&str>) {
    let snapshot_path = fixture.path().strip_suffix(".in").unwrap().to_owned();
    let source = fixture
        .content()
        .replace("__VALUES__\n", &"                0,\n".repeat(8192));
    let mut authored = DriverDataBase::default();
    let url = Url::from_file_path(&snapshot_path).unwrap();
    authored.workspace().update(&mut authored, url, source);
    let Err(error) = prepare(&authored) else {
        panic!("large body needs a function diagnostic");
    };
    assert_eq!(error.code(), 4);
    let rendered = error.render(&authored);
    assert!(
        rendered.contains("quoted function node limit exceeded: 8193 > 8192"),
        "{rendered}"
    );
    test_utils::snap_test!(rendered, &snapshot_path);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_alias_arguments/allowed", glob: "*.fe")]
fn quoted_alias_constructor_preserves_arguments(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).unwrap();
    let authored = field_authority_fixture_db(fixture);
    let output = prepare(&authored)
        .expect("alias arguments elaborate")
        .expect("generation");
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_alias_arguments/rejected", glob: "*.fe")]
fn quoted_alias_constructor_rejects_destination_width(fixture: Fixture<&str>) {
    let authored = field_authority_fixture_db(fixture);
    let Err(error) = prepare(&authored) else {
        panic!("Opt<u8> must not become Opt<u256>");
    };
    let rendered = error.render(&authored);
    assert!(
        rendered.contains("Opt<u256>") && rendered.contains("Opt<u8>"),
        "{rendered}"
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_alias_arguments/allowed", glob: "fixed_width.fe")]
fn quoted_alias_comparison_rejects_changed_width(fixture: Fixture<&str>) {
    assert_alias_comparison_rejects_changed_width(fixture, 9);
}

fn assert_alias_comparison_rejects_changed_width(fixture: Fixture<&str>, expected_count: usize) {
    use hir::analysis::ty::ty_def::PrimTy;
    use hir::core::meta::FrozenQuoteSignatureType;
    assert_quote_comparison_rejects_widened_type(fixture, |code| {
        let mut count = 0;
        for node in &mut code.nodes {
            for constructor in &mut node.rooted_constructors {
                if let Some(arguments) = &mut constructor.type_arguments {
                    assert_eq!(
                        arguments,
                        &[FrozenQuoteSignatureType::Primitive(PrimTy::U8)]
                    );
                    arguments[0] = FrozenQuoteSignatureType::Primitive(PrimTy::U256);
                    count += 1;
                }
            }
        }
        assert_eq!(count, expected_count);
    });
}

fn assert_quote_comparison_rejects_widened_type(
    fixture: Fixture<&str>,
    change: impl FnOnce(&mut hir::core::meta::FrozenCode),
) {
    assert_quote_comparison_rejects_mutation(fixture, |_, code| change(code), &["U8", "U256"]);
}

fn assert_quote_comparison_rejects_mutation(
    fixture: Fixture<&str>,
    change: impl FnOnce(&DriverDataBase, &mut hir::core::meta::FrozenCode),
    mismatches: &[&str],
) {
    use salsa::Setter;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let (authored, url) = fixture_db(fixture);
    let mut output = prepare(&authored).unwrap().unwrap();
    let input = HirDb::generated_module(&output, &url).unwrap();
    let expected = input.requests(&output).clone();
    let mut changed = expected.clone();
    let FrozenMetaValue::Code(code) = &mut changed[0].value else {
        panic!("code");
    };
    change(&authored, code);
    input.set_requests(&mut output).to(changed);
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    for function in module.all_funcs(&output) {
        let (errors, _) = hir::analysis::ty::ty_check::check_func_body(&output, *function);
        assert!(
            errors.is_empty(),
            "changed function {:?} is not well typed: {errors:?}",
            function
                .name(&output)
                .to_opt()
                .map(|name| name.data(&output))
        );
    }
    let failure = catch_unwind(AssertUnwindSafe(|| {
        quote_round_trip::assert_round_trip_against(&authored, &output, &url, &expected);
    }))
    .expect_err("independent comparison must reject a valid carrier mutation");
    let message = failure
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| failure.downcast_ref::<&str>().copied())
        .expect("comparison message");
    assert!(
        mismatches.iter().all(|mismatch| message.contains(mismatch)),
        "wrong comparison failure: {message}"
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_private_nominals", glob: "*.fe")]
fn quoted_provider_private_nominals_keep_author_scope(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).unwrap();
    let authored = field_authority_fixture_db(fixture);
    let output = prepare(&authored)
        .expect("provider-private nominals elaborate")
        .expect("generation");
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_private_nominals/rejected", glob: "*.fe")]
fn ordinary_constructor_paths_keep_destination_visibility(fixture: Fixture<&str>) {
    use hir::analysis::diagnostics::DiagnosticVoucher;
    let (db, url) = fixture_db(fixture);
    let func = named_function(&db, &url, "unauthorized");
    let (diagnostics, _) = hir::analysis::ty::ty_check::check_func_body(&db, func);
    assert!(
        !diagnostics.is_empty(),
        "ordinary code must not acquire quote authority"
    );
    let rendered = db.format_complete_diagnostics(
        &diagnostics
            .iter()
            .map(|d| d.to_complete(&db))
            .collect::<Vec<_>>(),
    );
    assert!(rendered.contains("not visible"), "{rendered}");
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_private_nominals/readback", glob: "source.fe")]
fn copied_constructor_arguments_keep_destination_generics(fixture: Fixture<&str>) {
    let adapter = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/quote_private_nominals/readback/adapter.fe");
    let copied = read_back_support::transplant_fixture(fixture.path(), fixture.content(), &adapter);
    quote_round_trip::assert_round_trip(&copied.authored, &copied.output, &copied.url);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_self_constructors", glob: "*.fe")]
fn quoted_self_constructors_keep_impl_owner(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).unwrap();
    let authored = field_authority_fixture_db(fixture);
    let output = prepare(&authored)
        .expect("Self constructor transport")
        .expect("generation");
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_self_constructors/rejected", glob: "*.fe")]
fn quoted_self_preserves_field_visibility(fixture: Fixture<&str>) {
    let authored = field_authority_fixture_db(fixture);
    let error = prepare(&authored)
        .err()
        .expect("Self does not authorize private fields");
    let rendered = error.rendered().expect("rendered field diagnostic");
    assert!(rendered.contains("8-0114"), "{rendered}");
    assert!(rendered.contains("not visible"), "{rendered}");
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_struct_alias/allowed", glob: "*.fe")]
fn quoted_struct_aliases_keep_owner_and_arguments(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).unwrap();
    let authored = field_authority_fixture_db(fixture);
    let output = prepare(&authored)
        .expect("struct alias transport")
        .expect("generation");
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_struct_alias/rejected", glob: "*.fe")]
fn quoted_struct_aliases_reject_destination_width(fixture: Fixture<&str>) {
    let authored = field_authority_fixture_db(fixture);
    let error = prepare(&authored)
        .err()
        .expect("Box<u8> must not become Box<u256>");
    let rendered = error.render(&authored);
    assert!(
        rendered.contains("Box<u8>") && rendered.contains("Box<u256>"),
        "{rendered}"
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_struct_alias/allowed", glob: "width.fe")]
fn quoted_struct_alias_comparison_rejects_changed_width(fixture: Fixture<&str>) {
    assert_alias_comparison_rejects_changed_width(fixture, 4);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_signature_alias/allowed", glob: "*.fe")]
fn quoted_signature_aliases_expand_at_provider(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).unwrap();
    let authored = field_authority_fixture_db(fixture);
    let output = prepare(&authored)
        .expect("signature aliases")
        .expect("generation");
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_signature_alias/allowed", glob: "fixed_width.fe")]
fn quoted_signature_comparison_rejects_changed_width(fixture: Fixture<&str>) {
    use hir::analysis::ty::ty_def::PrimTy;
    use hir::core::meta::FrozenQuoteSignatureType;
    assert_quote_comparison_rejects_widened_type(fixture, |code| {
        let types = &mut code.nodes[code.root].function_types[0];
        assert_eq!(
            types.params,
            vec![Some(FrozenQuoteSignatureType::Primitive(PrimTy::U8))]
        );
        assert_eq!(
            types.result,
            Some(FrozenQuoteSignatureType::Primitive(PrimTy::U8))
        );
        types.params[0] = Some(FrozenQuoteSignatureType::Primitive(PrimTy::U256));
        types.result = Some(FrozenQuoteSignatureType::Primitive(PrimTy::U256));
    });
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_signature_alias/rejected", glob: "*.fe")]
fn quoted_signature_aliases_reject_destination_width(fixture: Fixture<&str>) {
    use hir::analysis::diagnostics::DiagnosticVoucher;
    let url = Url::from_file_path(fixture.path()).unwrap();
    let authored = field_authority_fixture_db(fixture);
    let rendered = match prepare(&authored) {
        Err(error) => error.render(&authored),
        Ok(Some(output)) => {
            let wrong = named_function(&output, &url, "wrong");
            let (errors, _) = hir::analysis::ty::ty_check::check_func_body(&output, wrong);
            assert!(!errors.is_empty(), "Box<u8> must not become Box<u256>");
            output.format_complete_diagnostics(
                &errors
                    .iter()
                    .map(|error| error.to_complete(&output))
                    .collect::<Vec<_>>(),
            )
        }
        Ok(None) => panic!("expected generation"),
    };
    assert!(
        rendered.contains("8-0000")
            && rendered.contains("Box<u8>")
            && rendered.contains("Box<u256>"),
        "{rendered}"
    );
}

#[test]
fn expanded_signature_types_share_the_freezing_budget() {
    let template = include_str!("fixtures/quote_signature_alias/limits/expanded_types.fe.in");
    for functions in [1, 32] {
        let source = template
            .replace("__ELEMENTS__", &vec!["u8"; 1024].join(", "))
            .replace(
                "__FUNCTIONS__",
                &(0..functions)
                    .map(|index| {
                        format!(
                            "        fn generated_{index}(_ value: own Huge) -> Huge {{ value }}\n"
                        )
                    })
                    .collect::<String>(),
            );
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///expanded_signature_types.fe").unwrap();
        db.workspace().update(&mut db, url, source);
        if functions == 1 {
            prepare(&db)
                .expect("a single expanded alias fits")
                .expect("generation");
        } else {
            let Err(error) = prepare(&db) else {
                panic!("expanded signature types bypassed the shared freezing budget");
            };
            assert_eq!(error.code(), 3);
            let rendered = error.render(&db);
            assert!(rendered.contains("frozen meta nodes limit"), "{rendered}");
        }
    }
}

#[test]
fn checked_type_freezing_is_bounded_on_the_normal_stack() {
    use fe_driver::elaboration::read_back::read_back_type;
    use hir::analysis::ty::ty_def::{PrimTy, TyBase, TyData, TyId};
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///checked_type_freezing.fe").unwrap();
    db.workspace()
        .update(&mut db, url.clone(), "fn context() {}".to_owned());
    let function = named_function(&db, &url, "context");
    let tuple = TyId::new(&db, TyData::TyBase(TyBase::Prim(PrimTy::Tuple(1))));
    let mut ty = TyId::bool(&db);
    for depth in 1..=300 {
        ty = TyId::app(&db, tuple, ty);
        if depth == 128 {
            read_back_type(&db, function, ty).expect("ordinary nested type fits");
        }
    }
    assert!(
        read_back_type(&db, function, ty).is_err(),
        "deep checked types need a normal construction error"
    );
    let pair = TyId::new(&db, TyData::TyBase(TyBase::Prim(PrimTy::Tuple(2))));
    let mut ty = TyId::bool(&db);
    for _ in 0..16 {
        ty = TyId::app(&db, TyId::app(&db, pair, ty), ty);
    }
    assert!(
        read_back_type(&db, function, ty).is_err(),
        "shared checked types still charge every frozen edge"
    );
}

#[test]
fn expanded_alias_types_fail_during_bounded_freezing() {
    let template = include_str!("fixtures/quote_signature_alias/limits/expanded_types.fe.in");
    let mut aliases = "type T0 = u8\n".to_owned();
    for level in 1..=16 {
        let previous = level - 1;
        aliases.push_str(&format!("type T{level} = (T{previous}, T{previous})\n"));
    }
    aliases.push_str("type Huge = T16");
    let source = template
        .replace("type Huge = (__ELEMENTS__)", &aliases)
        .replace(
            "__FUNCTIONS__",
            "fn generated(_ value: own Huge) -> Huge { value }",
        );
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///expanded_alias_type.fe").unwrap();
    db.workspace().update(&mut db, url, source);
    let Err(error) = prepare(&db) else {
        panic!("exponentially shared alias must fail before freezing a large tree");
    };
    let rendered = error.render(&db);
    assert!(
        rendered.contains("quoted type nodes") && rendered.contains("65537 > 65536"),
        "{rendered}"
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_private_nominals/readback", glob: "source.fe")]
fn copied_constructor_types_share_the_request_budget(fixture: Fixture<&str>) {
    use hir::analysis::ty::ty_def::PrimTy;
    use hir::core::meta::FrozenQuoteSignatureType as T;
    let adapter = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/quote_private_nominals/readback/adapter.fe");
    let mut copied =
        read_back_support::transplant_fixture(fixture.path(), fixture.content(), &adapter);
    let input = HirDb::generated_module(&copied.output, &copied.url).expect("generation");
    let mut requests = input.requests(&copied.output).clone();
    let FrozenMetaValue::Code(code) = &mut requests[0].value else {
        panic!("code")
    };
    let [FrozenCodeHole::Code(child, _)] = code.nodes[code.root].holes.as_slice() else {
        panic!("body fragment")
    };
    let child = *child;
    let root = &mut code.nodes[child].rooted_constructors[0];
    assert_eq!(
        root.func,
        Some(0),
        "read-back keeps the source function index"
    );
    root.type_arguments = Some(vec![T::Tuple(vec![T::Primitive(PrimTy::Bool); 65_536])]);
    input.set_requests(&mut copied.output).to(requests);
    let file = copied
        .output
        .workspace()
        .get(&copied.output, &copied.url)
        .unwrap();
    let errors = hir::lower::generated_hir_errors(&copied.output, copied.output.top_mod(file));
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].failure,
        hir::lower::GeneratedQuoteFailure::RequestNodeLimit {
            actual: 65_537,
            maximum: 65_536,
        }
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_signature_alias/limits", glob: "associated_call.fe")]
fn associated_constant_call_types_share_the_request_budget(fixture: Fixture<&str>) {
    use hir::analysis::ty::ty_def::PrimTy;
    let (authored, url) = fixture_db(fixture);
    let mut output = prepare(&authored)
        .expect("associated constant")
        .expect("generation");
    let input = HirDb::generated_module(&output, &url).unwrap();
    let mut requests = input.requests(&output).clone();
    let FrozenMetaValue::Code(code) = &mut requests[0].value else {
        panic!("code")
    };
    let root = code.nodes[code.root]
        .rooted_funcs
        .iter_mut()
        .find(|root| root.assoc_const == Some(0))
        .expect("constant call");
    root.type_arguments = Some(vec![FrozenQuoteSignatureType::Tuple(vec![
        FrozenQuoteSignatureType::Primitive(PrimTy::Bool); 65_536
    ])]);
    input.set_requests(&mut output).to(requests);
    let file = output.workspace().get(&output, &url).unwrap();
    let errors = hir::lower::generated_hir_errors(&output, output.top_mod(file));
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].failure,
        hir::lower::GeneratedQuoteFailure::RequestNodeLimit {
            actual: 65_537,
            maximum: 65_536,
        }
    );
}

#[test]
fn reflected_field_types_share_the_freezing_budget() {
    let template = include_str!("fixtures/quote_signature_alias/limits/reflected_type.fe.in");
    for level in [4, 16] {
        let mut aliases = "type D0<T> = T\n".to_owned();
        for depth in 1..=level {
            let previous = depth - 1;
            aliases.push_str(&format!(
                "type D{depth}<T> = (D{previous}<T>, D{previous}<T>)\n"
            ));
        }
        let source = template
            .replace("__ALIASES__", &aliases)
            .replace("__LEVEL__", &level.to_string());
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///reflected_type_budget.fe").unwrap();
        db.workspace().update(&mut db, url, source);
        if level == 4 {
            prepare(&db)
                .expect("small reflected type")
                .expect("generation");
        } else {
            let Err(error) = prepare(&db) else {
                panic!("large reflected metadata was admitted")
            };
            assert_eq!(error.code(), 3);
            let rendered = error.render(&db);
            assert!(
                rendered.contains("frozen meta nodes limit exceeded: 65537 > 65536"),
                "{rendered}"
            );
        }
    }
}

#[test]
fn reflected_type_holes_charge_each_emitted_expansion() {
    use hir::core::meta::FrozenQuoteFieldType as F;
    let template = include_str!("fixtures/quote_signature_alias/limits/reflected_type.fe.in");
    let source = template
        .replace("__ALIASES__", "type D0<T> = T")
        .replace("__LEVEL__", "0");
    let mut authored = DriverDataBase::default();
    let url = Url::parse("file:///reflected_type_replay.fe").unwrap();
    authored
        .workspace()
        .update(&mut authored, url.clone(), source);
    let mut output = prepare(&authored).expect("small type").expect("generation");
    let input = HirDb::generated_module(&output, &url).unwrap();
    let original = input.requests(&output).clone();
    let FrozenMetaValue::Code(code) = &original[0].value else {
        panic!("code")
    };
    let slots = code.nodes[code.root]
        .holes
        .iter()
        .enumerate()
        .filter_map(|(slot, hole)| {
            matches!(hole, FrozenCodeHole::Value(_, Some(F::TargetParam(0)))).then_some(slot)
        })
        .collect::<Vec<_>>();
    assert_eq!(slots.len(), 3, "impl argument, parameter and result");
    for slot in slots {
        let mut changed = original.clone();
        let FrozenMetaValue::Code(code) = &mut changed[0].value else {
            panic!("code")
        };
        let FrozenCodeHole::Value(_, hint) = &mut code.nodes[code.root].holes[slot] else {
            panic!("hint")
        };
        *hint = Some(F::Tuple(vec![F::TargetParam(0); 65_536]));
        input.set_requests(&mut output).to(changed);
        let file = output.workspace().get(&output, &url).unwrap();
        let errors = hir::lower::generated_hir_errors(&output, output.top_mod(file));
        assert_eq!(errors.len(), 1, "type-hole slot {slot}");
        assert!(
            matches!(errors[0].failure,
            hir::lower::GeneratedQuoteFailure::RequestNodeLimit { actual, maximum: 65_536 }
                if (65_537..=65_538).contains(&actual)),
            "type-hole slot {slot}: {:?}",
            errors[0].failure
        );
    }
}

#[test]
fn shared_reflected_types_are_charged_on_item_replay() {
    let template = include_str!("fixtures/quote_signature_alias/limits/reflected_type.fe.in");
    let mut aliases = "type D0<T> = T\n".to_owned();
    for level in 1..=12 {
        let previous = level - 1;
        aliases.push_str(&format!(
            "type D{level}<T> = (D{previous}<T>, D{previous}<T>)\n"
        ));
    }
    let single = template
        .replace("__ALIASES__", &aliases)
        .replace("__LEVEL__", "12");
    let mut control = DriverDataBase::default();
    let control_url = Url::parse("file:///single_reflected_type_chunk.fe").unwrap();
    control
        .workspace()
        .update(&mut control, control_url, single.clone());
    prepare(&control)
        .expect("one reflected item fits")
        .expect("generation");
    let source = single
        .replace("quote { impl", "let chunk = quote { impl")
        .replace("    } }", "    } }\n    quote {\n        ${chunk}\n        ${chunk}\n        fn marker() {}\n    }");
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///shared_reflected_type_replay.fe").unwrap();
    db.workspace().update(&mut db, url, source);
    let Err(error) = prepare(&db) else {
        panic!("replayed reflected types bypassed the request budget")
    };
    assert_eq!(
        error.code(),
        4,
        "freezing fits; output replay needs the limit"
    );
    let rendered = error.render(&db);
    assert!(
        rendered.contains("quotation request node limit exceeded: 65537 > 65536"),
        "{rendered}"
    );
}

#[test]
fn checked_body_metadata_has_one_aggregate_freezing_budget() {
    let elements = std::iter::repeat_n("u8", 1024)
        .collect::<Vec<_>>()
        .join(", ");
    let values = std::iter::repeat_n("0", 1024)
        .collect::<Vec<_>>()
        .join(", ");
    let statements = (0..40)
        .map(|index| format!("let _value{index}: Huge = identity<Huge>(value())\n"))
        .collect::<String>();
    let source = format!(
        "type Huge = ({elements})\nfn value() -> Huge {{ ({values}) }}\nfn identity<T>(_ value: own T) -> T {{ value }}\nfn subject() -> u256 {{ {statements} 0 }}"
    );
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///checked_body_metadata_budget.fe").unwrap();
    db.workspace().update(&mut db, url.clone(), source);
    let subject = named_function(&db, &url, "subject");
    let result = read_back(&db, subject);
    assert_eq!(
        result,
        Err(ReadBackError::Construction(Box::new(
            fe_driver::elaboration::construction::ConstructionError::Limit {
                name: fe_driver::elaboration::FreezingLimitKind::MetaNodes,
                actual: 65537,
                maximum: 65536,
            },
        ))),
        "individually bounded types need one aggregate read-back limit"
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/read_back_view_transport", glob: "*.fe")]
fn repeated_code_edges_share_the_freezing_budget(fixture: Fixture<&str>) {
    use fe_driver::elaboration::{construction::ConstructionError, invocations};
    use hir::analysis::semantic::{
        CodeSource, QuotedCode, SemConstId, SemConstValue, StructuralHandle,
    };
    let (db, url) = fixture_db(fixture);
    let subject = named_function(&db, &url, "subject");
    let child = fe_driver::elaboration::read_back::read_back_value(&db, subject).unwrap();
    let &SemConstValue::Structural { ty, .. } = child.value(&db) else {
        panic!("code value")
    };
    let file = db.workspace().get(&db, &url).unwrap();
    let invocation = invocations(&db, file).unwrap().pop().unwrap();
    let input = invocation.record_input(&db).unwrap();
    // Resource mutation: all edges point at one already issued child. This
    // carrier is inspected only at freezing, not admitted as an output template.
    for edges in [2, 65_536] {
        let graph = QuotedCode::new(&db, CodeSource::Empty, vec![child; edges]);
        let value = SemConstId::new(
            &db,
            SemConstValue::Structural {
                ty,
                handle: StructuralHandle::Code(graph),
            },
        );
        let frozen = input.freeze_value(value);
        if edges == 2 {
            assert!(frozen.is_ok(), "small shared graph: {frozen:?}");
        } else {
            assert!(
                matches!(
                    frozen,
                    Err(ConstructionError::Limit {
                        name: fe_driver::elaboration::FreezingLimitKind::MetaNodes,
                        actual: 65537,
                        maximum: 65536,
                    })
                ),
                "{frozen:?}"
            );
        }
    }
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_open_scope/allowed", glob: "parameter_shadow.fe")]
fn replay_preflights_nested_binder_metadata(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).unwrap();
    let authored = field_authority_fixture_db(fixture);
    let mut output = prepare(&authored).unwrap().unwrap();
    let input = HirDb::generated_module(&output, &url).unwrap();
    let mut requests = input.requests(&output).clone();
    let FrozenMetaValue::Code(code) = &mut requests[0].value else {
        panic!("code")
    };
    let node = &mut code.nodes[code.root];
    let site = node.template.as_ref().unwrap().expr;
    let local = node.locals.first_mut().expect("shadowing local");
    // Resource mutation, not a valid binder-identity mutation. The guard must
    // run before attempting to consume or clone the oversized use table.
    local.uses = vec![site; 65_536];
    input.set_requests(&mut output).to(requests);
    let file = output.workspace().get(&output, &url).unwrap();
    let errors = hir::lower::generated_hir_errors(&output, output.top_mod(file));
    assert_eq!(errors.len(), 1);
    assert!(
        matches!(errors[0].failure,
        hir::lower::GeneratedQuoteFailure::RequestNodeLimit { actual, maximum: 65_536 }
            if actual > 65_536),
        "{:?}",
        errors[0].failure
    );
}

#[test]
fn copied_kind_bounds_have_a_pre_recursion_limit() {
    for depth in [128, 300] {
        let kind = "* -> ".repeat(depth) + "*";
        let source = format!(
            "use core::meta::{{Code, Item, Record}}\nconst fn emit(_ record: Record) -> Code<Item> {{ quote {{ fn generated() -> u256 {{ 0 }} }} }}\n#[generate(emit)]\nstruct Target<F: {kind}> {{}}"
        );
        let mut db = DriverDataBase::default();
        let url = Url::parse("file:///quoted_kind_bound_limit.fe").unwrap();
        db.workspace().update(&mut db, url, source);
        if depth == 128 {
            prepare(&db)
                .expect("ordinary kind bound fits")
                .expect("generation");
        } else {
            let Err(error) = prepare(&db) else {
                panic!("deep copied kind needs a quotation diagnostic")
            };
            assert_eq!(error.code(), 4);
            let rendered = error.render(&db);
            assert!(
                rendered.contains("quoted type nesting limit exceeded: 257 > 256"),
                "{rendered}"
            );
        }
    }
}

#[test]
fn reflected_hints_share_the_enclosing_type_depth_limit() {
    use hir::core::meta::{FrozenQuoteFieldType as F, FrozenQuoteSignatureType as T};
    let source = include_str!("fixtures/quote_signature_alias/limits/reflected_type.fe.in")
        .replace("__ALIASES__", "type D0<T> = T")
        .replace("__LEVEL__", "0");
    let mut authored = DriverDataBase::default();
    let url = Url::parse("file:///combined_type_hint_depth.fe").unwrap();
    authored
        .workspace()
        .update(&mut authored, url.clone(), source);
    let mut output = prepare(&authored).unwrap().unwrap();
    let input = HirDb::generated_module(&output, &url).unwrap();
    let mut requests = input.requests(&output).clone();
    let FrozenMetaValue::Code(code) = &mut requests[0].value else {
        panic!("code")
    };
    let node = &mut code.nodes[code.root];
    let argument = &mut node.impl_trait_type_arguments.as_mut().unwrap()[0];
    let T::Hole(slot) = argument else {
        panic!("field type hole")
    };
    let slot = *slot;
    let mut nested = T::Hole(slot);
    for _ in 0..200 {
        nested = T::Ptr(Box::new(nested));
    }
    *argument = nested;
    let FrozenCodeHole::Value(_, Some(hint)) = &mut node.holes[slot as usize] else {
        panic!("hint")
    };
    let mut nested = F::TargetParam(0);
    for _ in 0..100 {
        nested = F::Ptr(Box::new(nested));
    }
    *hint = nested;
    input.set_requests(&mut output).to(requests);
    let file = output.workspace().get(&output, &url).unwrap();
    let errors = hir::lower::generated_hir_errors(&output, output.top_mod(file));
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].failure,
        hir::lower::GeneratedQuoteFailure::TypeDepthLimit {
            actual: 257,
            maximum: 256
        }
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_fragment_locals",
    glob: "*.fe",
)]
fn ordinary_fragment_locals_retain_checked_binders(fixture: Fixture<&str>) {
    let (authored, url) = fixture_db(fixture);
    let output = prepare(&authored)
        .expect("ordinary fragment binders generate")
        .expect("generation request");
    let file = output.workspace().get(&output, &url).unwrap();
    let module = output.top_mod(file);
    assert!(hir::lower::generated_hir_errors(&output, module).is_empty());
    assert!(hir::lower::generated_quote_hygiene_errors(&output, module).is_empty());
    quote_round_trip::assert_round_trip(&authored, &output, &url);
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_fragment_locals",
    glob: "arms.fe",
)]
fn fragment_local_comparison_rejects_changed_binder_identity(fixture: Fixture<&str>) {
    assert_quote_comparison_rejects_mutation(
        fixture,
        |authored, code| {
            let node = code
                .nodes
                .iter_mut()
                .find(|node| node.locals.iter().any(|local| local.func.is_none()))
                .expect("arm fragment locals");
            let key = node.template.as_ref().unwrap();
            let file = authored
                .workspace()
                .get(authored, key.owner.file())
                .unwrap();
            let owner = authored
                .top_mod(file)
                .all_funcs(authored)
                .iter()
                .copied()
                .find(|func| func.frozen_locator(authored).as_ref() == Some(&key.owner))
                .unwrap();
            let body = owner.body(authored).unwrap();
            let hir::hir_def::Expr::Quote(quote) = body.exprs(authored)[key.expr].unwrap_ref()
            else {
                panic!("authored arm quote")
            };
            let index = |name: &str| {
                node.locals
                    .iter()
                    .position(|local| {
                        let hir::hir_def::Pat::Path(hir::hir_def::Partial::Present(path), _) =
                            quote.template.pats(authored)[local.binder].unwrap_ref()
                        else {
                            return false;
                        };
                        path.as_ident(authored)
                            .is_some_and(|ident| ident.data(authored) == name)
                    })
                    .expect("named tuple binder")
            };
            let left = index("left");
            let right = index("right");
            assert_ne!(left, right);
            assert_eq!(node.locals[left].uses.len(), 1);
            assert_eq!(node.locals[right].uses.len(), 1);
            let uses = node.locals[left].uses.clone();
            node.locals[left].uses = node.locals[right].uses.clone();
            node.locals[right].uses = uses;
        },
        &["local use retains its binder"],
    );
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/cli_output/single_files",
    glob: "quote_match_nonexhaustive.fe",
)]
fn generated_match_diagnostic_points_at_authored_match(fixture: Fixture<&str>) {
    let source = fixture.content().to_owned();
    let (authored, _) = fixture_db(fixture);
    let error = match prepare(&authored) {
        Err(error) => error,
        Ok(_) => panic!("missing enum arm must be rejected"),
    };
    assert_eq!(error.code(), 5);
    let line = source
        .lines()
        .position(|line| line.contains("match self"))
        .unwrap()
        + 1;
    let rendered = error.render(&authored);
    assert!(
        rendered.contains(&format!("quote_match_nonexhaustive.fe:{line}:")),
        "{rendered}"
    );
    assert!(
        rendered.contains("8-0034") && rendered.contains("Mode::Off"),
        "{rendered}"
    );
    assert!(rendered.contains("generated here"), "{rendered}");
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_open_scope/rejected",
    glob: "arm_shadow_type.fe",
)]
fn generated_arm_type_diagnostic_keeps_fragment_source(fixture: Fixture<&str>) {
    let source = fixture.content().to_owned();
    let snapshot_path = fixture.path().to_owned();
    let (authored, _) = fixture_db(fixture);
    let Err(error) = prepare(&authored) else {
        panic!("arm-local u256 cannot match an enum pattern")
    };
    let line = source
        .lines()
        .position(|line| line.contains("match other"))
        .unwrap()
        + 1;
    let rendered = error.render(&authored);
    assert!(
        rendered.contains(&format!("arm_shadow_type.fe:{line}:")),
        "{rendered}"
    );
    assert!(rendered.contains("generated here"), "{rendered}");
    test_utils::snap_test!(rendered, &snapshot_path);
}

#[test]
fn generated_match_diagnostic_keeps_provider_file() {
    let lib = include_str!("fixtures/quote_diagnostic_sources/lib.fe");
    let author = include_str!("fixtures/quote_diagnostic_sources/author.fe");
    let mut authored = DriverDataBase::default();
    for (path, source) in [
        (
            "file:///project/fe.toml",
            "name = \"quote_diagnostic_sources\"\nversion = \"0.1.0\"\n",
        ),
        ("file:///project/src/lib.fe", lib),
        ("file:///project/src/author.fe", author),
    ] {
        authored
            .workspace()
            .update(&mut authored, Url::parse(path).unwrap(), source.to_owned());
    }
    let Err(error) = prepare(&authored) else {
        panic!("missing arm must be rejected")
    };
    assert_eq!(error.code(), 5, "{error:?}");
    let line = author
        .lines()
        .position(|line| line.contains("match self"))
        .unwrap()
        + 1;
    let rendered = error.render(&authored);
    assert!(
        rendered.contains(&format!("author.fe:{line}:")),
        "{rendered}"
    );
    assert!(
        rendered.contains("lib.fe") && rendered.contains("generated here"),
        "{rendered}"
    );
    assert!(
        rendered.contains("8-0034") && rendered.contains("Mode::Off"),
        "{rendered}"
    );
    let snapshot_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/quote_diagnostic_sources/author.fe");
    test_utils::snap_test!(rendered, snapshot_path.to_str().unwrap());
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_diagnostic_sources",
    glob: "or_pattern_binders.fe",
)]
fn generated_or_pattern_diagnostic_names_authored_binders(fixture: Fixture<&str>) {
    let source = fixture.content().to_owned();
    let (mut authored, url) = fixture_db(fixture);
    let Err(error) = prepare(&authored) else {
        panic!("different variant binder groups cannot share an or-pattern")
    };
    assert_eq!(error.code(), 5);
    let rendered = error.render(&authored);
    let line = source
        .lines()
        .position(|line| line.contains("match s"))
        .unwrap()
        + 1;
    assert!(
        rendered.contains("the binders of `${a}` are not available here"),
        "{rendered}"
    );
    let column = source
        .lines()
        .nth(line - 1)
        .unwrap()
        .find("${a.at(fa)}")
        .unwrap()
        + 1;
    assert!(
        rendered.contains(&format!("or_pattern_binders.fe:{line}:{column}")),
        "{rendered}"
    );
    assert!(
        !rendered.contains("undefined variable `<quote-group-local-"),
        "{rendered}"
    );
    assert!(rendered.contains("generated here"), "{rendered}");
    assert!(rendered.contains("8-0076"), "{rendered}");

    // Ordinary authored binders reject this same or-pattern shape.
    authored.workspace().update(
        &mut authored,
        url.clone(),
        "enum E { A(u256), B(u256) }\nfn f(_ s: E) -> u256 { match s { E::A(a) | E::B(b) => a } }"
            .into(),
    );
    let file = authored.workspace().get(&authored, &url).unwrap();
    let ordinary = authored.run_on_ingot(authored.top_mod(file).ingot(&authored));
    assert!(ordinary.has_errors(&authored));
    let rendered = ordinary.format_diags(&authored);
    assert!(rendered.contains("8-0076"), "{rendered}");
}

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/quote_diagnostic_sources",
    glob: "missing_protocol.fe",
)]
fn missing_quotation_protocol_reports_the_quote(fixture: Fixture<&str>) {
    let snapshot_path = fixture.path().to_owned();
    let (mut db, url) = fixture_db(fixture);
    let (core_url, text) = db
        .workspace()
        .all_files(&db)
        .iter()
        .find(|(_, file)| file.text(&db).contains("pub struct Where {}"))
        .map(|(url, file)| (url.clone(), file.text(&db).clone()))
        .expect("bundled quotation protocol");
    db.workspace().update(
        &mut db,
        core_url,
        text.replace("pub struct Where {}", "pub struct MissingWhere {}"),
    );
    let file = db.workspace().get(&db, &url).unwrap();
    let diagnostics = db.run_on_ingot(db.top_mod(file).ingot(&db));
    let rendered = diagnostics.format_diags(&db);
    assert!(
        rendered.contains("quotation requires `core::meta::Where`"),
        "{rendered}"
    );
    assert!(rendered.contains("missing_protocol.fe:4:"), "{rendered}");
    test_utils::snap_test!(rendered, &snapshot_path);
}

#[test]
fn every_missing_quotation_protocol_entry_is_diagnosed() {
    use hir::core::meta::MetaItem;
    for item in MetaItem::ALL {
        let mut db = DriverDataBase::default();
        let url: Url = "file:///missing_entry.fe".parse().unwrap();
        db.workspace().update(
            &mut db,
            url.clone(),
            "pub const fn author() -> u256 { let code = quote { 1 }\n0 }".into(),
        );
        let (core_url, text) = db
            .workspace()
            .all_files(&db)
            .iter()
            .find(|(_, file)| file.text(&db).contains("pub struct Where {}"))
            .map(|(url, file)| (url.clone(), file.text(&db).clone()))
            .unwrap();
        let name = item.path().rsplit("::").next().unwrap();
        let declaration = match item {
            MetaItem::Code => "pub struct Code<".to_owned(),
            MetaItem::DeclarationName => "pub enum DeclarationName ".to_owned(),
            _ => format!("pub struct {name} "),
        };
        assert!(text.contains(&declaration));
        let missing = declaration.replacen(name, &format!("Missing{name}"), 1);
        let changed = text.replacen(&declaration, &missing, 1);
        db.workspace()
            .update(&mut db, core_url.clone(), changed.clone());
        let file = db.workspace().get(&db, &url).unwrap();
        let diagnostics = db.run_on_ingot(db.top_mod(file).ingot(&db));
        let rendered = diagnostics.format_diags(&db);
        drop(diagnostics);
        assert!(
            rendered.contains(&format!("quotation requires `{}`", item.path())),
            "{item:?}: {rendered}"
        );
        assert!(rendered.contains("error[8-0115]"), "{rendered}");

        // A lookalike in the author's scope cannot repair the core protocol.
        db.workspace().update(&mut db, url.clone(), format!(
            "pub struct {name} {{}}\npub const fn author() -> u256 {{ let code = quote {{ 1 }}\n0 }}"));
        let rendered = db
            .run_on_ingot(db.top_mod(file).ingot(&db))
            .format_diags(&db);
        assert!(
            rendered.contains(&format!("quotation requires `{}`", item.path())),
            "{rendered}"
        );

        // Check a concrete result type, even with the author's lookalike in scope.
        db.workspace().update(&mut db, url.clone(), format!(
            "pub struct {name} {{}}\npub const fn author() -> core::meta::Code<core::meta::Expr> {{ quote {{ 1 }} }}"));

        // Ordinary exports and aliases of the actual core type retain its identity.
        let alias = if item == MetaItem::Code {
            "pub type Code<K> = MissingCode<K>".to_owned()
        } else {
            format!("pub type {name} = Missing{name}")
        };
        for repair in [
            format!("pub use ingot::meta::Missing{name} as {name}"),
            alias,
        ] {
            db.workspace()
                .update(&mut db, core_url.clone(), format!("{changed}\n{repair}\n"));
            let diagnostics = db.run_on_ingot(db.top_mod(file).ingot(&db));
            assert!(
                !diagnostics.has_errors(&db),
                "{item:?}: {repair}: {}",
                diagnostics.format_diags(&db)
            );
        }
    }
}

#[test]
fn checked_type_read_back_preserves_the_underlying_depth_failure() {
    let mut source = String::from("type D0 = u8\n");
    for index in 1..=300 {
        source.push_str(&format!("type D{index} = (D{}, u8)\n", index - 1));
    }
    source.push_str("fn subject(value: own D300) -> D300 { value }\n");
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///checked_type_read_back_depth.fe").unwrap();
    db.workspace().update(&mut db, url.clone(), source);
    let subject = named_function(&db, &url, "subject");
    let error =
        fe_driver::elaboration::read_back::read_back_type(&db, subject, subject.return_ty(&db))
            .expect_err("expanded type needs a normal depth diagnostic");
    assert_eq!(
        error,
        ReadBackError::Construction(Box::new(
            fe_driver::elaboration::construction::ConstructionError::Limit {
                name: fe_driver::elaboration::FreezingLimitKind::TypeDepth,
                actual: 257,
                maximum: 256,
            },
        ))
    );
}

#[test]
fn checked_body_issuance_defers_admission_and_preserves_unsupported_type_rejection() {
    use fe_driver::elaboration::{construction::ConstructionError, invocations};
    let source = format!(
        "{}\nuse core::meta::{{Record, Code, Item}}\nconst fn author(_ record: Record) -> Code<Item> {{ quote {{ fn generated() -> u256 {{ 0 }} }} }}\n#[generate(author)]\nstruct Target {{}}\n",
        include_str!("fixtures/read_back_cast/read_back_cast_shadow.fe"),
    );
    let mut db = DriverDataBase::default();
    let url = Url::parse("file:///checked_body_issuance.fe").unwrap();
    db.workspace().update(&mut db, url.clone(), source);
    let subject = named_function(&db, &url, "unsupported_const_array");
    assert_eq!(read_back(&db, subject), Err(ReadBackError::UnsupportedType));
    let value = fe_driver::elaboration::read_back::read_back_value(&db, subject)
        .expect("issuance retains checked source without freezing transport metadata");
    let file = db.workspace().get(&db, &url).unwrap();
    let requests = invocations(&db, file).unwrap();
    let input = requests[0].record_input(&db).unwrap();
    assert_eq!(
        input.freeze_value(value),
        Err(ConstructionError::ReadBack(ReadBackError::UnsupportedType)),
        "transport must retain the native read-back rejection",
    );
}
