use std::{
    ffi::OsStr,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread::{self, sleep},
    time::{Duration, Instant},
};

// This is a termination guard, not a performance assertion. The full release
// suite runs many compiler processes concurrently, so valid malformed-input
// checks need headroom above their loaded wall-clock time.
const FE_CHECK_TIMEOUT: Duration = Duration::from_secs(30);

struct FeCheckRun {
    code: i32,
    combined: String,
    elapsed: Duration,
    timed_out: bool,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/crash_regressions")
}

fn is_fe_file(path: &Path) -> bool {
    path.extension().and_then(OsStr::to_str) == Some("fe")
}

fn spawn_reader<R: Read + Send + 'static>(mut pipe: R) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut buf = Vec::new();
        pipe.read_to_end(&mut buf).unwrap_or_else(|err| {
            panic!("failed to read `fe check` output stream: {err}");
        });
        buf
    })
}

fn run_fe_check(path: &Path) -> FeCheckRun {
    let mut child = Command::new(env!("CARGO_BIN_EXE_fe"))
        .args(["check", "--standalone", "--color", "never"])
        .arg(path)
        .env("NO_COLOR", "1")
        .env("RUST_BACKTRACE", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|err| panic!("failed to run `fe check` on {path:?}: {err}"));

    // Drain both pipes concurrently to avoid deadlocking when diagnostics exceed pipe capacity.
    let stdout_handle = spawn_reader(
        child
            .stdout
            .take()
            .unwrap_or_else(|| panic!("missing stdout pipe for `fe check` on {path:?}")),
    );
    let stderr_handle = spawn_reader(
        child
            .stderr
            .take()
            .unwrap_or_else(|| panic!("missing stderr pipe for `fe check` on {path:?}")),
    );

    let start = Instant::now();
    let timed_out = loop {
        if child
            .try_wait()
            .unwrap_or_else(|err| panic!("failed to poll `fe check` on {path:?}: {err}"))
            .is_some()
        {
            break false;
        }

        if start.elapsed() >= FE_CHECK_TIMEOUT {
            let _ = child.kill();
            break true;
        }

        sleep(Duration::from_millis(10));
    };

    let elapsed = start.elapsed();

    let status = child
        .wait()
        .unwrap_or_else(|err| panic!("failed to wait for `fe check` on {path:?}: {err}"));

    let stdout = stdout_handle
        .join()
        .unwrap_or_else(|_| panic!("stdout reader thread panicked for {path:?}"));
    let stderr = stderr_handle
        .join()
        .unwrap_or_else(|_| panic!("stderr reader thread panicked for {path:?}"));

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );

    let code = status.code().unwrap_or(-1);
    FeCheckRun {
        code,
        combined,
        elapsed,
        timed_out,
    }
}

#[test]
fn valid_generic_type_default_fixtures_pass_check() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../uitest/fixtures/ty_check");
    // The uitest snapshots cover HIR diagnostics; these valid fixtures must
    // also pass the full `fe check` pipeline.
    for fixture in [
        "const_arg_in_param_projection.fe",
        "generic_type_default_environments.fe",
        "inherent_method_projection_target.fe",
    ] {
        let run = run_fe_check(&fixtures.join(fixture));
        assert!(!run.timed_out, "{fixture} timed out: {}", run.combined);
        assert_eq!(run.code, 0, "{fixture}: {}", run.combined);
    }

    // This HIR fixture still has a known full-pipeline call-body mapping error.
    // Normalizing the retained application must preserve that diagnostic,
    // rather than panic while matching its associated output.
    let wrapper = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../hir/test_files/ty_check/associated_types_wrapper.fe");
    let run = run_fe_check(&wrapper);
    assert!(!run.timed_out, "wrapper timed out: {}", run.combined);
    assert_eq!(run.code, 1, "wrapper: {}", run.combined);
    assert!(run.combined.contains("error[16-0004]"), "{}", run.combined);
    assert!(!run.combined.contains("panicked"), "{}", run.combined);
}

#[test]
fn family_normalization_limits_are_reported_at_written_types() {
    let directory = tempfile::tempdir().unwrap();
    let declarations = "trait Tr { type Out<T> }\nstruct Base {}\nstruct Wrap<U> {}\n\
        impl Tr for Base { type Out<T> = T }\n\
        impl<U: Tr> Tr for Wrap<U> { type Out<T> = <U as Tr>::Out<(T, T)> }\n";
    let mut failures = Vec::new();
    for (name, layers, body, expected_limit) in [
        ("return_literal", 14, "fn f() -> TYPE { 0 }", true),
        ("parameter_used", 14, "fn f(_ x: TYPE) -> u8 { x }", true),
        ("parameter_unused", 24, "fn f(_ x: TYPE) {}", true),
        ("field", 14, "struct S { value: TYPE }", true),
        ("alias", 14, "type Result = TYPE", true),
        ("local", 14, "fn f() { let x: TYPE = 0 }", true),
        ("below_limit", 13, "fn f(_ x: TYPE) {}", false),
    ] {
        let nested = "Wrap<".repeat(layers) + "Base" + &">".repeat(layers);
        let ty = format!("<{nested} as Tr>::Out<u8>");
        let path = directory.path().join(format!("{name}.fe"));
        fs::write(
            &path,
            format!("{declarations}{}", body.replace("TYPE", &ty)),
        )
        .unwrap();
        let run = run_fe_check(&path);
        let limits = run
            .combined
            .matches("type normalization limit exceeded")
            .count();
        if run.timed_out
            || run.code != i32::from(expected_limit)
            || limits != usize::from(expected_limit)
            || run.combined.contains("panicked")
            || run.combined.contains("internal borrow checking error")
        {
            failures.push(format!(
                "{name}: exit={}, limits={limits}, timed_out={}; {}",
                run.code,
                run.timed_out,
                run.combined.lines().take(8).collect::<Vec<_>>().join("\n")
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn crash_regressions_do_not_panic() {
    let dir = fixtures_dir();
    let mut fixtures: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("failed to read crash regressions dir {dir:?}: {err}"))
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| is_fe_file(path))
        .collect();
    fixtures.sort();

    assert!(
        !fixtures.is_empty(),
        "no crash regression fixtures found under {dir:?}"
    );

    let mut failures = Vec::new();

    for fixture in fixtures {
        let run = run_fe_check(&fixture);
        let code = run.code;
        let combined = run.combined;
        let elapsed = run.elapsed;

        if run.timed_out {
            failures.push(format!(
                "`fe check` timed out after {FE_CHECK_TIMEOUT:?} (elapsed {elapsed:?}) on {fixture:?}\n{combined}"
            ));
            continue;
        }

        if code == 101 {
            failures.push(format!(
                "`fe check` panicked on {fixture:?} after {elapsed:?}\n{combined}"
            ));
            continue;
        }

        if combined.contains("panicked at")
            || combined.contains("thread 'main' panicked")
            || combined.contains("stack backtrace:")
        {
            failures.push(format!(
                "`fe check` produced panic output on {fixture:?} after {elapsed:?}\n{combined}"
            ));
            continue;
        }

        if code != 0 && code != 1 {
            failures.push(format!(
                "`fe check` returned unexpected exit code {code} on {fixture:?} after {elapsed:?}\n{combined}"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "crash regression failures:\n\n{}",
        failures.join("\n\n")
    );
}
