use common::InputDb;
use dir_test::{Fixture, dir_test};
use fe_driver::DriverDataBase;
use hir::analysis::analysis_pass::{
    AttrMisusePass, ErrorLowerPass, EventLowerPass, ModuleAnalysisPass, MsgLowerPass, ParsingPass,
};
use url::Url;

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../../ingots/core/src", glob: "**/*.fe")]
fn bundled_core_has_no_parse_or_lowering_diagnostics(fixture: Fixture<&str>) {
    check_bundled_syntax(fixture);
}

#[dir_test(dir: "$CARGO_MANIFEST_DIR/../../ingots/std/src", glob: "**/*.fe")]
fn bundled_std_has_no_parse_or_lowering_diagnostics(fixture: Fixture<&str>) {
    check_bundled_syntax(fixture);
}

fn check_bundled_syntax(fixture: Fixture<&str>) {
    let mut db = DriverDataBase::default();
    let url = Url::from_file_path(fixture.path()).expect("bundled file URL");
    let file = db
        .workspace()
        .update(&mut db, url, fixture.content().to_string());
    let module = db.top_mod(file);
    let mut passes: Vec<Box<dyn ModuleAnalysisPass>> = vec![
        Box::new(ParsingPass {}),
        Box::new(MsgLowerPass {}),
        Box::new(EventLowerPass {}),
        Box::new(ErrorLowerPass {}),
        Box::new(AttrMisusePass {}),
    ];
    for pass in &mut passes {
        let diagnostics = pass.run_on_module(&db, module);
        assert!(
            diagnostics.is_empty(),
            "{}: {}",
            fixture.path(),
            db.format_complete_diagnostics(
                &diagnostics
                    .iter()
                    .map(|diag| diag.to_complete(&db))
                    .collect::<Vec<_>>()
            )
        );
    }
}

#[test]
fn recovered_library_parse_errors_remain_diagnostics() {
    let mut db = DriverDataBase::default();
    let file = db.workspace().update(
        &mut db,
        Url::parse("file:///bundled_syntax_recovery.fe").unwrap(),
        "pub enum CodeNodeKind {\n Integer\n Boolean\n String\n}\n".to_owned(),
    );
    // Lowering still returns a recovered module; that must not certify its syntax.
    let module = db.top_mod(file);
    let diagnostics = ParsingPass {}.run_on_module(&db, module);
    assert!(
        !diagnostics.is_empty(),
        "missing enum separators must fail the bundled syntax guard"
    );
}
