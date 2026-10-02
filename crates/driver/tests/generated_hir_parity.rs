use std::{collections::BTreeMap, ops::Range};

use common::InputDb;
use dir_test::{Fixture, dir_test};
use fe_driver::DriverDataBase;
use hir::{
    HirDb,
    hir_def::{Body, Expr, ExprId, FieldIndex, Func, ItemKind, Partial, Pat, PatId, Stmt, StmtId},
    span::LazySpan,
};
use parser::{
    SyntaxNode,
    ast::{self, prelude::*},
};
use url::Url;

fn expression(db: &DriverDataBase, body: Body<'_>, id: ExprId) -> String {
    match id.data(db, body).unwrap_ref() {
        Expr::Lit(value) => format!("Lit({:?})", value.pretty_print(db)),
        Expr::Path(Partial::Present(path)) => format!("Path({:?})", path.pretty_print(db)),
        Expr::Cast(value, Partial::Present(ty)) => {
            format!(
                "Cast({}, {:?})",
                expression(db, body, *value),
                ty.pretty_print(db)
            )
        }
        Expr::Bin(lhs, rhs, op) => format!(
            "Bin({op:?}, {}, {})",
            expression(db, body, *lhs),
            expression(db, body, *rhs)
        ),
        Expr::Call(callee, args) => format!(
            "Call({}, {:?})",
            expression(db, body, *callee),
            args.iter()
                .map(|arg| (
                    arg.label.map(|label| label.data(db).clone()),
                    expression(db, body, arg.expr)
                ))
                .collect::<Vec<_>>()
        ),
        Expr::Field(base, Partial::Present(field)) => {
            let field = match field {
                FieldIndex::Ident(name) => name.data(db).clone(),
                FieldIndex::Index(index) => index.data(db).to_string(),
            };
            format!("Field({}, {:?})", expression(db, body, *base), field)
        }
        Expr::Tuple(values) => format!(
            "Tuple({:?})",
            values
                .iter()
                .map(|value| expression(db, body, *value))
                .collect::<Vec<_>>()
        ),
        Expr::Block(stmts) => format!(
            "Block({:?})",
            stmts
                .iter()
                .map(|stmt| statement(db, body, *stmt))
                .collect::<Vec<_>>()
        ),
        other => panic!("generated expression needs parity normalization: {other:?}"),
    }
}

fn pattern(db: &DriverDataBase, body: Body<'_>, id: PatId) -> String {
    match id.data(db, body).unwrap_ref() {
        Pat::Path(Partial::Present(path), mutable) => {
            format!("Path({:?}, {mutable})", path.pretty_print(db))
        }
        other => panic!("generated pattern needs parity normalization: {other:?}"),
    }
}

fn statement(db: &DriverDataBase, body: Body<'_>, id: StmtId) -> String {
    match id.data(db, body).unwrap_ref() {
        Stmt::Let(pat, ty, value) => format!(
            "Let({}, {:?}, {:?})",
            pattern(db, body, *pat),
            ty.map(|ty| ty.pretty_print(db)),
            value.map(|value| expression(db, body, value))
        ),
        Stmt::Expr(value) => format!("Expr({})", expression(db, body, *value)),
        other => panic!("generated statement needs parity normalization: {other:?}"),
    }
}

fn normalized_function(db: &DriverDataBase, function: Func<'_>) -> (String, String) {
    let body = function.body(db).expect("generated function body");
    (
        function.pretty_print_signature(db),
        expression(db, body, body.expr(db)),
    )
}

fn normalized_item(db: &DriverDataBase, item: ItemKind<'_>) -> String {
    match item {
        ItemKind::Func(function) => format!("Function({:?})", normalized_function(db, function)),
        ItemKind::ImplTrait(implementation) => {
            let printed = implementation.pretty_print(db);
            let header = printed.split_once(" {\n").expect("printed impl header").0;
            let constants = implementation
                .hir_consts(db)
                .iter()
                .map(|constant| {
                    let name = constant.name.to_opt().expect("const name").data(db);
                    let ty = constant.ty.to_opt().expect("const type").pretty_print(db);
                    let body = constant.value.to_opt().expect("const value");
                    (name.clone(), ty, expression(db, body, body.expr(db)))
                })
                .collect::<Vec<_>>();
            let methods = implementation
                .methods(db)
                .map(|method| normalized_function(db, method))
                .collect::<Vec<_>>();
            format!("Impl({header:?}, {constants:?}, {methods:?})")
        }
        other => panic!("unexpected generated item: {other:?}"),
    }
}

fn is_top_level_generated_item(db: &DriverDataBase, item: ItemKind<'_>) -> bool {
    if !matches!(item, ItemKind::Func(_) | ItemKind::ImplTrait(_)) {
        return false;
    }
    !item
        .scope()
        .parent(db)
        .is_some_and(|parent| matches!(parent.item(), ItemKind::ImplTrait(_)))
}

fn assert_parity(fixture: Fixture<&str>) {
    let url = Url::from_file_path(fixture.path()).expect("fixture path is absolute");
    let mut authored = DriverDataBase::default();
    authored
        .workspace()
        .update(&mut authored, url.clone(), fixture.content().to_string());
    let output = fe_driver::elaboration::prepare(&authored)
        .unwrap()
        .expect("generation request");
    let file = output.workspace().get(&output, &url).expect("fixture file");
    let module = output.top_mod(file);
    let root = ast::Root::cast(SyntaxNode::new_root(hir::lower::parse_file_impl(
        &output, module,
    )))
    .unwrap();
    let requests = output
        .generated_module(&url)
        .expect("frozen generation input");
    let targets = requests
        .requests(&output)
        .iter()
        .map(|request| {
            let target = root
                .syntax()
                .descendants()
                .filter_map(ast::Struct::cast)
                .find(|target| {
                    let span: Range<usize> = target.syntax().text_range().into();
                    span.start == request.target_span_start
                })
                .expect("authored generation target");
            let span: Range<usize> = target.syntax().text_range().into();
            span
        })
        .collect::<Vec<_>>();
    let mut generated = Vec::new();
    for item in module.all_items(&output).iter().copied() {
        if !is_top_level_generated_item(&output, item) {
            continue;
        }
        let Some(span) = item.span().resolve(&output) else {
            continue;
        };
        if span.file.url(&output).as_ref() != Some(&url) {
            continue;
        }
        let span: Range<usize> = span.range.into();
        let Some(target) = targets
            .iter()
            .find(|target| target.start <= span.start && span.start < target.end)
        else {
            continue;
        };
        generated.push((item, target.end, item.pretty_print(&output)));
    }
    assert!(
        !generated.is_empty(),
        "fixture produced no generated HIR items"
    );

    let mut insertions = BTreeMap::<usize, Vec<(usize, String)>>::new();
    for (index, (_, offset, printed)) in generated.iter().enumerate() {
        insertions
            .entry(*offset)
            .or_default()
            .push((index, printed.clone()));
    }
    let mut replay_source = fixture.content().to_string();
    for (offset, entries) in insertions.into_iter().rev() {
        let inserted = entries
            .into_iter()
            .map(|(index, printed)| format!("\n// generated_hir_parity_{index}\n{printed}\n"))
            .collect::<String>();
        replay_source.insert_str(offset, &inserted);
    }
    let mut replay = DriverDataBase::default();
    replay
        .workspace()
        .update(&mut replay, url.clone(), replay_source.clone());
    let replay_file = replay.workspace().get(&replay, &url).unwrap();
    let replay_module = replay.top_mod(replay_file);
    let items = replay_module.all_items(&replay);
    for (index, (original, _, _)) in generated.iter().enumerate() {
        let marker = format!("// generated_hir_parity_{index}\n");
        let start = replay_source.find(&marker).expect("printed item marker") + marker.len();
        let parsed = items
            .iter()
            .copied()
            .find(|item| {
                item.span().resolve(&replay).is_some_and(|span| {
                    let range: Range<usize> = span.range.into();
                    span.file.url(&replay).as_ref() == Some(&url) && range.start == start
                })
            })
            .unwrap_or_else(|| panic!("re-lowered item {index} is absent"));
        assert_eq!(
            normalized_item(&output, *original),
            normalized_item(&replay, parsed),
            "item {index}"
        );
    }
    let diagnostics = replay.run_on_ingot(replay_module.ingot(&replay));
    assert!(
        !diagnostics.has_errors(&replay),
        "{}",
        diagnostics.format_diags(&replay)
    );
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/generated_const_generic_function", glob: "*.fe")]
fn generated_function_roundtrips(fixture: Fixture<&str>) {
    assert_parity(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/tests/fixtures/generic_derive", glob: "*.fe")]
fn generated_implementation_roundtrips(fixture: Fixture<&str>) {
    assert_parity(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../fe/tests/fixtures/structural_generation", glob: "abi.fe")]
fn generated_abi_roundtrips(fixture: Fixture<&str>) {
    assert_parity(fixture);
}
