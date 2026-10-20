//! `fe-leadsheet FILE.fe [--tests] [--out DIR]`: lower a standalone Fe file to
//! leadsheet-vsdg text.
//!
//! - Without `--tests`: one module per contract, its runtime entry Fe's own
//!   dispatcher.
//! - With `--tests`: one module per `#[test]`, its entry the test root; the
//!   module's comments say what Fe expects of it.
//! - `--dump FILTER` prints the rMIR of the functions whose symbol contains
//!   FILTER instead.
//!
//! With `--out DIR` each module is written to `DIR/NAME.vsdg`; otherwise the
//! modules are printed one after another. A summary goes to stderr; the exit
//! code is nonzero if anything could not be lowered.

use std::process::ExitCode;

use common::{InputDb, config::Config, diagnostics::Severity};
use driver::DriverDataBase;
use hir::hir_def::HirIngot;
use url::Url;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut path = None;
    let mut tests = false;
    let mut out = None;
    let mut dump = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--tests" => tests = true,
            "--out" => out = it.next().cloned(),
            "--dump" => dump = it.next().cloned(),
            _ if path.is_none() && !a.starts_with('-') => path = Some(a.clone()),
            _ => {
                eprintln!("usage: fe-leadsheet FILE.fe [--tests] [--out DIR | --dump FILTER]");
                return ExitCode::from(2);
            }
        }
    }
    let Some(path) = path else {
        eprintln!("usage: fe-leadsheet FILE.fe [--tests] [--out DIR | --dump FILTER]");
        return ExitCode::from(2);
    };
    let canonical = match std::fs::canonicalize(&path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let content = match std::fs::read_to_string(&canonical) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let url = Url::from_file_path(&canonical).expect("an absolute path");
    let mut db = DriverDataBase::default();
    let ingot_dir = canonical.parent().and_then(|p| {
        p.ancestors().find(|d| {
            d.join("fe.toml").is_file()
                && std::fs::read_to_string(d.join("fe.toml"))
                    .is_ok_and(|c| matches!(Config::parse(&c), Ok(Config::Ingot(_))))
        })
    });
    if let Some(dir) = ingot_dir {
        let ingot_url = Url::from_directory_path(dir).expect("a directory");
        if driver::init_ingot(&mut db, &ingot_url) {
            return ExitCode::FAILURE;
        }
        if db.workspace().get(&db, &url).is_none() {
            db.workspace().touch(&mut db, url.clone(), Some(content));
        }
    } else {
        db.workspace().touch(&mut db, url.clone(), Some(content));
    }
    let Some(file) = db.workspace().get(&db, &url) else {
        eprintln!("{path}: not loaded");
        return ExitCode::FAILURE;
    };
    let top_mod = db.top_mod(file);
    let ingot = ingot_dir.and_then(|_| db.workspace().containing_ingot(&db, url.clone()));
    if let Some(ingot) = ingot {
        let hir_diags = db.run_on_ingot(ingot);
        if hir_diags.has_errors(&db) {
            hir_diags.emit(&db);
            return ExitCode::FAILURE;
        }
        let mir_diags = db.mir_diagnostics_for_ingot(ingot);
        if mir_diags.iter().any(|d| d.severity == Severity::Error) {
            db.emit_complete_diagnostics(&mir_diags);
            return ExitCode::FAILURE;
        }
    } else {
        let hir_diags = db.run_on_top_mod(top_mod);
        if hir_diags.has_errors(&db) {
            hir_diags.emit(&db);
            return ExitCode::FAILURE;
        }
        let mir_diags = db.mir_diagnostics_for_top_mod(top_mod);
        if mir_diags.iter().any(|d| d.severity == Severity::Error) {
            db.emit_complete_diagnostics(&mir_diags);
            return ExitCode::FAILURE;
        }
    }
    if let Some(filter) = dump {
        return match fe_leadsheet::dump(&db, top_mod, tests, &filter) {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{path}: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let lower_one = |m: hir::hir_def::TopLevelMod<'_>| {
        if tests {
            fe_leadsheet::lower_tests(&db, m)
        } else {
            fe_leadsheet::lower_contracts(&db, m)
        }
    };
    let mut results: Vec<fe_leadsheet::TestModule> = match lower_one(top_mod) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    if results.is_empty()
        && let Some(ingot) = ingot
    {
        for &m in ingot.all_modules(&db) {
            if m == top_mod {
                continue;
            }
            match lower_one(m) {
                Ok(r) => results.extend(r),
                Err(e) => {
                    eprintln!("{path}: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    let mut failed = false;
    for r in results {
        match r {
            Ok(l) => {
                let ctor = l.constructor.as_ref().map_or(String::new(), |c| format!(", constructor @{c}"));
                eprintln!("{}: {} functions, entry @{}{ctor}{}", l.name, l.functions, l.entry, l.expect.as_ref().map_or(String::new(), |e| format!(", expect {e}")));
                match &out {
                    Some(dir) => {
                        let file = std::path::Path::new(dir).join(format!("{}.vsdg", l.name));
                        if let Err(e) = std::fs::write(&file, &l.text) {
                            eprintln!("{}: {e}", file.display());
                            failed = true;
                        }
                    }
                    None => print!("{}", l.text),
                }
            }
            Err((name, e)) => {
                eprintln!("{name}: not lowered: {e}");
                failed = true;
            }
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}
