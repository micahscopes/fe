use common::InputDb;
use dir_test::{Fixture, dir_test};
use driver::DriverDataBase;
use fmt::{Config, format_str};
use url::Url;

#[dir_test(
    dir: "$CARGO_MANIFEST_DIR/tests/fixtures/fe_test",
    glob: "*.fe",
)]
fn test_fmt_fe_test_fixtures_semantic_roundtrip(fixture: Fixture<&str>) {
    let formatted = format_str(fixture.content(), &Config::default())
        .unwrap_or_else(|err| panic!("format failed for {}: {err:?}", fixture.path()));

    let mut db = DriverDataBase::default();
    let file_url = Url::from_file_path(fixture.path())
        .unwrap_or_else(|_| panic!("fixture path should be absolute: {}", fixture.path()));
    db.workspace()
        .touch(&mut db, file_url.clone(), Some(formatted));
    let db = driver::elaboration::prepare(&db)
        .unwrap_or_else(|err| {
            panic!(
                "formatted output failed generation for {}: {err:?}",
                fixture.path()
            )
        })
        .unwrap_or(db);
    let file = db.workspace().get(&db, &file_url).expect("formatted file");
    let top_mod = db.top_mod(file);

    let diagnostics = db.run_on_top_mod(top_mod);
    assert!(
        diagnostics.is_empty(),
        "formatted output failed parse/HIR analysis for {}:\n{}",
        fixture.path(),
        diagnostics.format_diags(&db),
    );
}

/// What checking a file reports: the generation error code, or the sorted
/// codes of the errors in the checked output.
fn check_outcome(path: &std::path::Path, source: String) -> Vec<String> {
    let mut db = DriverDataBase::default();
    let url = Url::from_file_path(path).expect("absolute fixture path");
    db.workspace().touch(&mut db, url.clone(), Some(source));
    let db = match driver::elaboration::prepare(&db) {
        Err(error) => return vec![format!("generation 19-{:04}", error.code())],
        Ok(output) => output.unwrap_or(db),
    };
    let file = db.workspace().get(&db, &url).expect("fixture file");
    let mut codes = db
        .run_on_top_mod(db.top_mod(file))
        .complete_diags(&db)
        .into_iter()
        .filter(|diag| diag.severity == common::diagnostics::Severity::Error)
        .map(|diag| diag.error_code.to_string())
        .collect::<Vec<_>>();
    codes.sort();
    codes
}

/// Formatting a fixture that uses quotes, recursive type functions or named
/// impls must not change what `fe check` reports for it.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "checks hundreds of fixtures twice; run with --release"
)]
fn formatting_keeps_the_check_result_of_generation_fixtures() {
    fn fe_files(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                fe_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "fe") {
                files.push(path);
            }
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    fe_files(&root.join("crates"), &mut files);
    files.sort();
    let uses_new_syntax = |source: &str| {
        source.contains("quote {")
            || source.contains("quote(")
            || source.contains("recursive type fn")
            || source
                .lines()
                .any(|line| line.trim_start().starts_with("impl") && line.contains(" as "))
    };
    let mut changed = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(&path).expect("fixture source");
        if !uses_new_syntax(&source)
            || !parser::parse_source_file(&source, parser::RecoveryMode::Recover)
                .1
                .is_empty()
        {
            continue;
        }
        let Ok(formatted) = format_str(&source, &Config::default()) else {
            continue;
        };
        if formatted == source {
            continue;
        }
        let before = check_outcome(&path, source);
        let after = check_outcome(&path, formatted);
        if before != after {
            changed.push(format!(
                "{}: {before:?} became {after:?}",
                path.strip_prefix(&root).unwrap_or(&path).display()
            ));
        }
    }
    assert!(
        changed.is_empty(),
        "formatting changed the check result:\n{}",
        changed.join("\n")
    );
}
