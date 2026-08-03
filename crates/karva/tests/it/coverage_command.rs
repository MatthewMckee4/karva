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
        root.clone(),
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
