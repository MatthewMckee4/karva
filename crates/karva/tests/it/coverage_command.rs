use std::collections::{BTreeMap, BTreeSet};

use camino::{Utf8Path, Utf8PathBuf};
use insta_cmd::assert_cmd_snapshot;
use karva_coverage::native::{CoverageMode, NativeCoverage, NativeFileCoverage, SourceFingerprint};

use crate::common::TestContext;

const SOURCE: &str = "first = 1\nsecond = 2\n";

fn write_coverage(context: &TestContext, path: &Utf8Path) {
    context.write_file("src/app.py", SOURCE);
    let root = context.root();
    let artifact = NativeCoverage::new(
        CoverageMode::Line,
        BTreeSet::from([Utf8PathBuf::from("src")]),
        None,
        BTreeMap::from([(
            Utf8PathBuf::from("src/app.py"),
            NativeFileCoverage {
                source_fingerprint: SourceFingerprint::from_bytes(SOURCE.as_bytes()),
                executable: BTreeSet::from([1, 2]),
                excluded: BTreeSet::new(),
                executed: BTreeSet::from([1]),
                line_contexts: BTreeMap::new(),
                branches: None,
            },
        )]),
    );
    artifact
        .write(&root.join(path))
        .expect("write coverage data");
}

#[test]
fn report_reads_default_native_data() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(context.coverage("report"), @r"
    success: true
    exit_code: 0
    ----- stdout -----

    Name         Stmts   Miss   Cover
    [LONG-LINE]
    src/app.py       2      1     50%
    [LONG-LINE]
    TOTAL            2      1     50%

    ----- stderr -----
    ");
}

#[test]
fn report_discovers_project_from_subdirectory_and_honors_config() {
    let context = TestContext::new();
    context.write_file(
        "karva.toml",
        r#"
[profile.default.coverage]
data-file = "build/native.json"
precision = 2
"#,
    );
    context.write_file("nested/.gitkeep", "");
    write_coverage(&context, Utf8Path::new("build/native.json"));
    let mut command = context.karva_command_in(context.root().join("nested"));
    command.args(["coverage", "report"]);

    assert_cmd_snapshot!(command, @"
    success: true
    exit_code: 0
    ----- stdout -----

    Name         Stmts   Miss   Cover
    [LONG-LINE]
    src/app.py       2      1   50.00%
    [LONG-LINE]
    TOTAL            2      1   50.00%

    ----- stderr -----
    ");
}

#[test]
fn report_cli_data_file_overrides_config() {
    let context = TestContext::new();
    context.write_file(
        "karva.toml",
        r#"
[profile.default.coverage]
data-file = "missing.json"
"#,
    );
    write_coverage(&context, Utf8Path::new("actual.json"));

    assert_cmd_snapshot!(
        context.coverage("report").args(["--data-file", "actual.json"]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----

    Name         Stmts   Miss   Cover
    [LONG-LINE]
    src/app.py       2      1     50%
    [LONG-LINE]
    TOTAL            2      1     50%

    ----- stderr -----
    "
    );
}

#[test]
fn report_missing_data_names_default_and_remedy() {
    let context = TestContext::new();

    assert_cmd_snapshot!(context.coverage("report"), @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: no coverage data found at `<temp_dir>/.karva/coverage/data.json`; run `uv run karva test --cov` first
    ");
}

#[test]
fn report_fail_under_returns_failure() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(context.coverage("report").arg("--fail-under=75"), @r"
    success: false
    exit_code: 1
    ----- stdout -----

    Name         Stmts   Miss   Cover
    [LONG-LINE]
    src/app.py       2      1     50%
    [LONG-LINE]
    TOTAL            2      1     50%

    coverage failure: required total coverage of 75% not reached, total coverage was 50%

    ----- stderr -----
    ");
}

#[test]
fn test_run_persists_native_data_without_rendering() {
    let context = TestContext::with_file(
        "test_native.py",
        r"
def test_native():
    assert True
",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--cov", "--cov-report=", "--status-level=none", "test_native.py"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );

    assert_cmd_snapshot!(context.coverage("report"), @r"
    success: true
    exit_code: 0
    ----- stdout -----

    Name             Stmts   Miss   Cover
    [LONG-LINE]
    test_native.py       2      0    100%
    [LONG-LINE]
    TOTAL                2      0    100%

    ----- stderr -----
    ");
}

#[test]
fn failing_test_still_persists_native_data() {
    let context = TestContext::with_file(
        "test_failure.py",
        r"
def test_failure():
    assert False
",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--cov", "--cov-report=", "--status-level=none", "test_failure.py"]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_failure::test_failure:

    error[test-failure]: Test `test_failure` failed
     --> test_failure.py:2:5
      |
    2 | def test_failure():
      |     ^^^^^^^^^^^^
      |
    info: Test failed here
     --> test_failure.py:3:5
      |
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    assert!(context.root().join(".karva/coverage/data.json").is_file());
}

#[test]
fn cov_append_unions_runs_and_default_replaces_them() {
    let context = TestContext::with_file(
        "test_append.py",
        r"
def first():
    return 1

def second():
    return 2

def test_alpha():
    assert first() == 1

def test_beta():
    assert second() == 2
",
    );
    let run = |filter: &str, append: bool| {
        let mut command = context.command_no_parallel();
        command.args([
            "--cov",
            "--cov-report=",
            "--status-level=none",
            "-E",
            filter,
            "test_append.py",
        ]);
        if append {
            command.arg("--cov-append");
        }
        command
    };

    assert_cmd_snapshot!(run("test(~alpha)", false), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
    assert_cmd_snapshot!(run("test(~beta)", true), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");

    let path = context.root().join(".karva/coverage/data.json");
    let appended = NativeCoverage::read(&path).expect("read appended data");
    let appended_file = appended
        .files
        .get(Utf8Path::new("test_append.py"))
        .expect("appended source");
    assert!(appended_file.executed.contains(&3));
    assert!(appended_file.executed.contains(&6));

    assert_cmd_snapshot!(run("test(~alpha)", false), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
    let replaced = NativeCoverage::read(&path).expect("read replaced data");
    let replaced_file = replaced
        .files
        .get(Utf8Path::new("test_append.py"))
        .expect("replaced source");
    assert!(replaced_file.executed.contains(&3));
    assert!(!replaced_file.executed.contains(&6));
}
