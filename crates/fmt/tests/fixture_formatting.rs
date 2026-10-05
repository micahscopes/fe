//! Formats every Fe fixture in the repository and checks that formatting is
//! stable and keeps the program: the formatted text parses, formatting it
//! again changes nothing, and its tokens, apart from layout, comments, braces
//! and separators the formatter may add or drop, are the source's tokens.
//! Files with known formatter gaps are listed in `fmt_known_gaps.txt`; a
//! listed file that now formats cleanly must be removed from the list.

use std::path::{Path, PathBuf};

use fe_fmt::{Config, format_str};
use parser::{RecoveryMode, SyntaxKind, SyntaxNode, parse_source_file};

fn fe_files(dir: &Path, files: &mut Vec<PathBuf>) {
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

/// The tokens that carry meaning. Commas and newlines separate items, and the
/// formatter may add or drop a trailing one; it may also wrap an arm body in
/// braces.
fn meaningful_tokens(source: &str) -> Vec<String> {
    let (green, _) = parse_source_file(source, RecoveryMode::Recover);
    SyntaxNode::new_root(green)
        .descendants_with_tokens()
        .filter_map(|element| element.into_token())
        .filter(|token| {
            !token.kind().is_trivia()
                && !matches!(
                    token.kind(),
                    SyntaxKind::Newline
                        | SyntaxKind::Comma
                        | SyntaxKind::LBrace
                        | SyntaxKind::RBrace
                )
        })
        .map(|token| token.text().to_string())
        .collect()
}

#[test]
fn formatting_keeps_every_fixture_and_is_stable() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.join("../..");
    let known_gaps: std::collections::BTreeSet<String> =
        std::fs::read_to_string(manifest.join("tests/fmt_known_gaps.txt"))
            .expect("known gaps list")
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_owned)
            .collect();
    let mut fixed = Vec::new();
    let mut files = Vec::new();
    for dir in ["crates", "ingots"] {
        fe_files(&root.join(dir), &mut files);
    }
    files.sort();
    let mut failures = Vec::new();
    let mut checked = 0;
    for path in &files {
        let source = std::fs::read_to_string(path).expect("fixture source");
        if !parse_source_file(&source, RecoveryMode::Recover)
            .1
            .is_empty()
        {
            continue;
        }
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .display()
            .to_string();
        let failures_before = failures.len();
        check_file(&relative, &source, &mut failures);
        checked += 1;
        if known_gaps.contains(&relative) {
            if failures.len() == failures_before {
                fixed.push(relative);
            }
            failures.truncate(failures_before);
        }
    }
    assert!(checked > 0, "no fixtures were formatted");
    assert!(
        failures.is_empty() && fixed.is_empty(),
        "{}\n\nlisted in fmt_known_gaps.txt but now format cleanly; remove them:\n{}",
        failures.join("\n"),
        fixed.join("\n")
    );
}

fn check_file(relative: &str, source: &str, failures: &mut Vec<String>) {
    let Ok(formatted) = format_str(source, &Config::default()) else {
        failures.push(format!("{relative}: formatting failed"));
        return;
    };
    if !parse_source_file(&formatted, RecoveryMode::Recover)
        .1
        .is_empty()
    {
        failures.push(format!("{relative}: formatted output does not parse"));
        return;
    }
    if format_str(&formatted, &Config::default()).ok().as_ref() != Some(&formatted) {
        failures.push(format!("{relative}: formatting again changes the output"));
    }
    let (before, after) = (meaningful_tokens(source), meaningful_tokens(&formatted));
    if before != after {
        let at = before
            .iter()
            .zip(&after)
            .position(|(a, b)| a != b)
            .unwrap_or(before.len().min(after.len()));
        failures.push(format!(
            "{relative}: formatting changed the tokens at token {at}: {:?} became {:?}",
            &before[at..(at + 6).min(before.len())],
            &after[at..(at + 6).min(after.len())],
        ));
    }
}
