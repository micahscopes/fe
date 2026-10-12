use std::path::Path;

use camino::Utf8PathBuf;
use dir_test::{Fixture, dir_test};
use fe_hir::analysis::{
    analysis_pass::ModuleAnalysisPass,
    name_resolution::{
        ImportAnalysisPass, NameDerivation, ResolvedImports, diagnostics::ImportDiag,
        resolve_imports,
    },
};
use fe_hir::test_db::{HirAnalysisTestDb, HirPropertyFormatter};
use fe_hir::{hir_def::Use, span::LazySpan};
use rustc_hash::FxHashMap;
use test_utils::snap_test;

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/test_files/imports",
    glob: "*.fe"
)]
fn import_standalone(fixture: Fixture<&str>) {
    let mut db = HirAnalysisTestDb::default();
    let path = Path::new(fixture.path());
    let file_name = path.file_name().and_then(|file| file.to_str()).unwrap();
    let file = db.new_stand_alone(file_name.into(), fixture.content());
    let (top_mod, mut prop_formatter) = db.top_mod(file);

    db.assert_no_diags(top_mod);

    let mut pass = ImportAnalysisPass {};
    let resolved_imports = resolve_imports(&db, top_mod.ingot(&db));
    let diags = pass.run_on_module(&db, top_mod);
    if !diags.is_empty() {
        panic!("Failed to resolve imports");
    }

    let res = format_imports(&db, &mut prop_formatter, &resolved_imports.1);
    snap_test!(res, fixture.path());
}

#[test]
fn std_evm_import_is_available() {
    let mut db = HirAnalysisTestDb::default();
    let file = db.new_stand_alone(
        Utf8PathBuf::from("std_import.fe"),
        r#"
use std::evm::revert

fn increment(val: u256) -> u256 {
    return val + 1
}

fn f() {
    assert!(increment(val: 0) == 1)
}
"#,
    );
    let (top_mod, _) = db.top_mod(file);
    db.assert_no_diags(top_mod);
}

fn format_imports<'db>(
    db: &'db HirAnalysisTestDb,
    prop_formatter: &mut HirPropertyFormatter<'db>,
    imports: &ResolvedImports<'db>,
) -> String {
    let mut use_res_map: FxHashMap<Use, Vec<String>> = FxHashMap::default();

    for name_resolved in imports.named_resolved.values().flat_map(|r| r.values()) {
        for res in name_resolved.iter_ok() {
            match res.derivation {
                NameDerivation::NamedImported(use_) => use_res_map
                    .entry(use_)
                    .or_default()
                    .push(res.pretty_path(db).unwrap()),
                _ => unreachable!(),
            }
        }
    }

    for (_, glob_set) in imports.glob_resolved.iter() {
        for (&use_, res_set_with_ident) in glob_set.iter() {
            for (ident, res_set) in res_set_with_ident.iter() {
                let ident = ident.data(db);
                for res in res_set {
                    let def_path = res.pretty_path(db).unwrap();
                    let resolved = format!("{def_path} as {ident}");
                    use_res_map.entry(use_).or_default().push(resolved)
                }
            }
        }
    }
    for (use_, mut values) in use_res_map.into_iter() {
        // Skip uses whose spans cannot be resolved (for example,
        // implicit imports that do not originate from this file).
        let span = use_.span();
        if span.resolve(db).is_none() {
            continue;
        }

        let use_span = span.into();
        values.sort_unstable();
        let imported_names = values.join(" | ");
        prop_formatter.push_prop(use_.top_mod(db), use_span, imported_names)
    }

    prop_formatter.finish(db)
}

fn padded_imports(padding: usize, imports: &[&str], body: &str) -> String {
    format!(
        "{}\n{}\n{body}",
        (0..padding)
            .map(|i| format!("mod pad{i} {{}}\n"))
            .collect::<String>(),
        imports.join("\n"),
    )
}

#[test]
fn builtin_glob_paths_wait_for_decidable_import_scopes() {
    let cases = [
        ("foo", "mod foo { pub mod i32 { pub struct Leaf {} } }"),
        (
            "ingress",
            "mod ingress { pub use ingot::bridge::* }\nmod bridge { pub mod i32 { pub struct Leaf {} } }",
        ),
        (
            "a",
            "mod a { pub use ingot::b::* }\nmod b { pub use ingot::a::*\npub mod i32 { pub struct Leaf {} } }",
        ),
    ];
    for padding in 0..24 {
        for (prefix, body) in cases {
            let glob = format!("use {prefix}::*");
            for imports in [[glob.as_str(), "use i32::*"], ["use i32::*", glob.as_str()]] {
                let mut db = HirAnalysisTestDb::default();
                let source = padded_imports(
                    padding,
                    &imports,
                    &format!("use Leaf as Item\n{body}\nfn accept(_ value: Item) {{}}"),
                );
                let file = db.new_stand_alone("builtin_glob.fe".into(), &source);
                let (top_mod, _) = db.top_mod(file);
                let (diags, _) = resolve_imports(&db, top_mod.ingot(&db));
                // The disputed glob must publish Leaf before its ambiguity is
                // diagnosed, so dependent imports have no cascading NotFound.
                assert!(
                    matches!(diags.as_slice(), [ImportDiag::Ambiguous(_, name, _)] if name.data(&db) == "i32"),
                    "padding {padding}, imports {imports:?}: {diags:?}"
                );
            }
        }
    }
}

#[test]
fn builtin_paths_without_competing_globs_remain_invalid() {
    for padding in 0..24 {
        let mut db = HirAnalysisTestDb::default();
        let source = padded_imports(padding, &["use i32::*"], "");
        let file = db.new_stand_alone("invalid_builtin.fe".into(), &source);
        let (top_mod, _) = db.top_mod(file);
        let (diags, _) = resolve_imports(&db, top_mod.ingot(&db));
        assert!(
            matches!(diags.as_slice(), [ImportDiag::InvalidPathSegment(_, name, _)] if name.data(&db) == "i32"),
            "padding {padding}: {diags:?}"
        );
    }
}

#[test]
fn named_overrides_and_glob_dependencies_keep_their_resolution() {
    let cases = [
        (
            ["use foo::i32", "use i32::Leaf as Item"],
            "mod foo { pub mod i32 { pub struct Leaf {} } }",
        ),
        (
            ["use foo::*", "use branch::*"],
            "use Leaf as Item\nmod foo { pub mod branch { pub struct Leaf {} } }",
        ),
    ];
    for padding in 0..24 {
        for (imports, body) in cases {
            for imports in [imports, [imports[1], imports[0]]] {
                let mut db = HirAnalysisTestDb::default();
                let source = padded_imports(
                    padding,
                    &imports,
                    &format!("{body}\nfn accept(_ value: Item) {{}}"),
                );
                let file = db.new_stand_alone("import_dependency.fe".into(), &source);
                let (top_mod, _) = db.top_mod(file);
                db.assert_no_diags(top_mod);
            }
        }
    }
}
