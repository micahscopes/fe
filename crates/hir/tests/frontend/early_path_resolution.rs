use std::path::Path;

use dir_test::{Fixture, dir_test};
use fe_hir::analysis::{
    name_resolution::{NameDomain, resolve_path},
    ty::trait_resolution::PredicateListId,
};
use fe_hir::test_db::{HirAnalysisTestDb, HirPropertyFormatter};
use fe_hir::{
    hir_def::{Expr, ExprId, ItemKind, Pat, PatId, PathId, TopLevelMod, TypeId},
    visitor::prelude::*,
};
use test_utils::snap_test;

// xxx remove this

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/test_files/early_path_resolution",
    glob: "*.fe"
)]
fn early_path_resolution_standalone(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let path = Path::new(fixture.path());
    let file_name = path.file_name().and_then(|file| file.to_str()).unwrap();
    let file = db.new_stand_alone(file_name.into(), fixture.content());
    let (top_mod, mut prop_formatter) = db.top_mod(file);
    db.assert_no_diags(top_mod);

    let mut ctxt = VisitorCtxt::with_top_mod(&db, top_mod);
    PathVisitor {
        db: &db,
        top_mod,
        domain_stack: Vec::new(),
        prop_formatter: &mut prop_formatter,
    }
    .visit_top_mod(&mut ctxt, top_mod);

    let res = prop_formatter.finish(&db);
    snap_test!(res, fixture.path());
}

struct PathVisitor<'db, 'a> {
    db: &'db HirAnalysisTestDb,
    top_mod: TopLevelMod<'db>,
    domain_stack: Vec<NameDomain>,
    prop_formatter: &'a mut HirPropertyFormatter<'db>,
}

impl<'db> Visitor<'db> for PathVisitor<'db, '_> {
    fn visit_item(&mut self, ctxt: &mut VisitorCtxt<'db, LazyItemSpan<'db>>, item: ItemKind<'db>) {
        if matches!(item, ItemKind::Use(_)) {
            return;
        }

        self.domain_stack.push(NameDomain::TYPE);
        walk_item(self, ctxt, item);
        self.domain_stack.pop();
    }

    fn visit_ty(&mut self, ctxt: &mut VisitorCtxt<'db, LazyTySpan<'db>>, ty: TypeId<'db>) {
        self.domain_stack.push(NameDomain::TYPE);
        walk_type(self, ctxt, ty);
        self.domain_stack.pop();
    }

    fn visit_pat(&mut self, _: &mut VisitorCtxt<'db, LazyPatSpan<'db>>, _: PatId, _: &Pat<'db>) {}

    fn visit_expr(
        &mut self,
        ctxt: &mut VisitorCtxt<'db, LazyExprSpan<'db>>,
        expr: ExprId,
        expr_data: &Expr<'db>,
    ) {
        if matches!(expr_data, Expr::Block { .. }) {
            walk_expr(self, ctxt, expr);
        }
    }

    fn visit_path(&mut self, ctxt: &mut VisitorCtxt<'db, LazyPathSpan<'db>>, path: PathId<'db>) {
        let scope = ctxt.scope();
        let prop = match resolve_path(
            self.db,
            path,
            scope,
            PredicateListId::empty_list(self.db),
            false,
        ) {
            Ok(res) => res.pretty_path(self.db).unwrap(),
            Err(err) => err.print(),
        };
        let span = ctxt
            .span()
            .unwrap()
            .segment(path.segment_index(self.db))
            .ident()
            .into();
        self.prop_formatter.push_prop(self.top_mod, span, prop);

        walk_path(self, ctxt, path);
    }
}

#[test]
fn resolved_declaration_keeps_alias_arguments_under_destination_shadowing() {
    use fe_hir::analysis::name_resolution::{PathRes, resolve_ident_to_bucket, resolve_name_res};

    let mut db = HirAnalysisTestDb::default();
    let source = db.new_stand_alone(
        "resolved_reference_source.fe".into(),
        "struct Opt<T> { value: T }\ntype O8 = Opt<u8>\n",
    );
    let destination = db.new_stand_alone(
        "resolved_reference_destination.fe".into(),
        "struct Opt<T> { value: T }\ntype O8 = Opt<u16>\n",
    );
    let (source_mod, _) = db.top_mod(source);
    let (destination_mod, _) = db.top_mod(destination);
    db.assert_no_diags(source_mod);
    db.assert_no_diags(destination_mod);
    let path = PathId::from_str(&db, "O8");
    let assumptions = PredicateListId::empty_list(&db);
    let declaration = resolve_ident_to_bucket(&db, path, source_mod.scope())
        .pick(NameDomain::TYPE)
        .as_ref()
        .expect("source alias should resolve");
    let direct = resolve_name_res(
        &db,
        declaration,
        None,
        path,
        destination_mod.scope(),
        assumptions,
    )
    .expect("resolved declaration should instantiate in the destination");
    let captured = PathId::new(
        &db,
        fe_hir::hir_def::PathKind::BoundItem {
            item: declaration.scope().expect("alias has a declaration scope"),
            display: path,
        },
        None,
    );
    let carried = resolve_path(&db, captured, destination_mod.scope(), assumptions, false)
        .expect("carried declaration should resolve without destination name lookup");
    let authored = resolve_path(&db, path, source_mod.scope(), assumptions, false).unwrap();
    let shadow = resolve_path(&db, path, destination_mod.scope(), assumptions, false).unwrap();
    fn alias_type<'db>(resolution: &PathRes<'db>) -> fe_hir::analysis::ty::ty_def::TyId<'db> {
        match resolution {
            PathRes::TyAlias(_, ty, _) => *ty,
            other => panic!("expected alias, got {other:?}"),
        }
    }
    assert_eq!(alias_type(&direct), alias_type(&authored));
    assert_eq!(alias_type(&carried), alias_type(&authored));
    assert_ne!(alias_type(&authored), alias_type(&shadow));
    assert!(alias_type(&direct).pretty_print(&db).contains("u8"));
    assert!(alias_type(&shadow).pretty_print(&db).contains("u16"));
}
