use std::collections::{BTreeMap, BTreeSet};

use camino::{Utf8Path, Utf8PathBuf};
use insta_cmd::assert_cmd_snapshot;
use karva_coverage::native::{CoverageMode, NativeCoverage, NativeFileCoverage, SourceFingerprint};

use crate::common::TestContext;

const SOURCE: &str = "first = 1\nsecond = 2\n";

fn write_coverage(context: &TestContext, path: &Utf8Path) {
    write_coverage_lines(context, path, BTreeSet::from([1]));
}

fn write_coverage_lines(context: &TestContext, path: &Utf8Path, executed: BTreeSet<u32>) {
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
                executed,
                line_contexts: BTreeMap::from([(1, BTreeSet::from(["test_example".to_owned()]))]),
                branches: None,
            },
        )]),
    );
    artifact
        .write(&root.join(path))
        .expect("write coverage data");
}

#[test]
fn combine_unions_pending_artifacts_and_removes_inputs() {
    let context = TestContext::new();
    let first = Utf8Path::new(".karva/coverage/pending/first.json");
    let second = Utf8Path::new(".karva/coverage/pending/second.json");
    write_coverage_lines(&context, first, BTreeSet::from([1]));
    write_coverage_lines(&context, second, BTreeSet::from([2]));

    assert_cmd_snapshot!(context.coverage("combine"), @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    ");

    let combined = NativeCoverage::read(&context.root().join(".karva/coverage/data.json"))
        .expect("read combined coverage");
    let file = combined
        .files
        .get(Utf8Path::new("src/app.py"))
        .expect("combined source");
    assert_eq!(file.executed, BTreeSet::from([1, 2]));
    assert!(!context.root().join(first).exists());
    assert!(!context.root().join(second).exists());
}

#[test]
fn combine_failure_preserves_output_and_inputs() {
    let context = TestContext::new();
    let output = Utf8Path::new(".karva/coverage/data.json");
    let valid = Utf8Path::new("inputs/valid.json");
    let rejected = Utf8Path::new("inputs/rejected.json");
    write_coverage_lines(&context, output, BTreeSet::from([1]));
    write_coverage_lines(&context, valid, BTreeSet::from([2]));
    context.write_file(rejected.as_str(), "not native coverage");
    let original = context.read_file(output.as_str());

    assert_cmd_snapshot!(
        context
            .coverage("combine")
            .args([valid.as_str(), rejected.as_str()]),
        @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: failed to combine native coverage artifacts:
    `<temp_dir>/inputs/rejected.json`: failed to parse native coverage artifact `<temp_dir>/inputs/rejected.json`: expected ident at line 1 column 2
    "
    );

    assert_eq!(context.read_file(output.as_str()), original);
    assert!(context.root().join(valid).exists());
    assert!(context.root().join(rejected).exists());
}

#[test]
fn combine_appends_directory_inputs_and_keeps_them() {
    let context = TestContext::new();
    let output = Utf8Path::new(".karva/coverage/data.json");
    let shard = Utf8Path::new("artifacts/nested/shard.json");
    write_coverage_lines(&context, output, BTreeSet::from([1]));
    write_coverage_lines(&context, shard, BTreeSet::from([2]));

    assert_cmd_snapshot!(
        context
            .coverage("combine")
            .args(["artifacts", "--append", "--keep"]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "
    );

    let combined = NativeCoverage::read(&context.root().join(output)).expect("read combined data");
    let file = combined
        .files
        .get(Utf8Path::new("src/app.py"))
        .expect("combined source");
    assert_eq!(file.executed, BTreeSet::from([1, 2]));
    assert!(context.root().join(shard).exists());
}

#[test]
fn combine_maps_cross_platform_paths_deterministically() {
    let context = TestContext::new();
    context.write_file(
        "karva.toml",
        r"
[profile.default.coverage]
path-aliases = ['C:\repo=.']
",
    );
    let unix = Utf8Path::new("artifacts/unix.json");
    let windows = Utf8Path::new("artifacts/windows.json");
    write_coverage(&context, unix);
    let mut windows_artifact =
        NativeCoverage::read(&context.root().join(unix)).expect("read portable coverage artifact");
    windows_artifact.source_roots = BTreeSet::from([Utf8PathBuf::from(r"C:\repo\src")]);
    let file = windows_artifact
        .files
        .remove(Utf8Path::new("src/app.py"))
        .expect("portable source");
    windows_artifact
        .files
        .insert(Utf8PathBuf::from(r"C:\repo\src\app.py"), file);
    windows_artifact
        .write(&context.root().join(windows))
        .expect("write Windows artifact");

    assert_cmd_snapshot!(
        context.coverage("combine").args([
            windows.as_str(),
            unix.as_str(),
            "--keep",
        ]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "
    );
    let output = ".karva/coverage/data.json";
    let first = context.read_file(output);

    assert_cmd_snapshot!(
        context.coverage("combine").args([
            unix.as_str(),
            windows.as_str(),
            "--keep",
        ]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "
    );
    assert_eq!(context.read_file(output), first);
    let combined = NativeCoverage::read(&context.root().join(output)).expect("read combined data");
    assert_eq!(
        combined.files.keys().collect::<Vec<_>>(),
        vec![Utf8Path::new("src/app.py")]
    );
}

#[test]
fn erase_is_idempotent_and_preserves_reports_and_neighbors() {
    let context = TestContext::new();
    context.write_file("karva.toml", "");
    let combined = Utf8Path::new(".karva/coverage/data.json");
    let shard = Utf8Path::new(".karva/coverage/pending/nested/shard.json");
    write_coverage(&context, combined);
    write_coverage(&context, shard);
    context.write_file(".karva/coverage/notes.json", "unrelated");
    context.write_file(".karva/coverage/pending/notes.txt", "unrelated");
    context.write_file("coverage.xml", "xml report");
    context.write_file("coverage.json", "json report");
    context.write_file("coverage.lcov", "lcov report");
    context.write_file("htmlcov/index.html", "html report");
    context.write_file("nested/.gitkeep", "");
    let mut command = context.karva_command_in(context.root().join("nested"));
    command.args(["coverage", "erase"]);

    assert_cmd_snapshot!(command, @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    ");
    assert!(!context.root().join(combined).exists());
    assert!(!context.root().join(shard).exists());
    for path in [
        ".karva/coverage/notes.json",
        ".karva/coverage/pending/notes.txt",
        "coverage.xml",
        "coverage.json",
        "coverage.lcov",
        "htmlcov/index.html",
    ] {
        assert!(
            context.root().join(path).exists(),
            "expected `{path}` to survive"
        );
    }

    assert_cmd_snapshot!(context.coverage("erase"), @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    ");
    assert_cmd_snapshot!(context.coverage("report"), @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: no coverage data found at `<temp_dir>/.karva/coverage/data.json`; run `uv run karva test --cov` first
    ");
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
fn html_writes_navigable_annotated_report() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(
        context.coverage("html").args([
            "--directory",
            "coverage-site",
            "--title",
            "Karva <coverage>",
        ]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "
    );
    insta::assert_snapshot!(
        "coverage_html_index",
        context.read_file("coverage-site/index.html")
    );
    insta::assert_snapshot!(
        "coverage_html_source",
        context.read_file("coverage-site/source-7372632f6170702e7079.html")
    );
}

#[test]
fn xml_writes_cobertura_report() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(context.coverage("xml"), @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    ");
    insta::assert_snapshot!(context.read_file("coverage.xml"), @r#"
    <?xml version="1.0" ?>
    <coverage version="1.0" timestamp="[TIMESTAMP]" lines-valid="2" lines-covered="1" line-rate="0.5000" branches-covered="0" branches-valid="0" branch-rate="0.0000" complexity="0.0">
      <sources>
        <source>.</source>
      </sources>
      <packages>
        <package name="." line-rate="0.5000" branch-rate="0.0000" complexity="0.0">
          <classes>
            <class name="src/app.py" filename="src/app.py" line-rate="0.5000" branch-rate="0.0000" complexity="0.0">
              <methods/>
              <lines>
                <line number="1" hits="1" branch="false"/>
                <line number="2" hits="0" branch="false"/>
              </lines>
            </class>
          </classes>
        </package>
      </packages>
    </coverage>
    "#);
}

#[test]
fn json_exports_pretty_report_with_contexts() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(context.coverage("json"), @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    ");
    assert!(!context.read_file("coverage.json").contains('\n'));

    assert_cmd_snapshot!(
        context
            .coverage("json")
            .args(["--pretty-print", "--show-contexts"]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "
    );
    insta::assert_snapshot!(context.read_file("coverage.json"), @r#"
    {
      "meta": {
        "format": 2,
        "version": "karva",
        "show_contexts": true
      },
      "files": {
        "src/app.py": {
          "executed_lines": [
            1
          ],
          "summary": {
            "covered_lines": 1,
            "num_statements": 2,
            "percent_covered": 50.0,
            "missing_lines": [
              2
            ],
            "excluded_lines": []
          },
          "missing_lines": [
            2
          ],
          "excluded_lines": [],
          "contexts": {
            "1": [
              "test_example"
            ]
          }
        }
      },
      "totals": {
        "covered_lines": 1,
        "num_statements": 2,
        "percent_covered": 50.0
      }
    }
    "#);
}

#[test]
fn lcov_writes_portable_tracefile() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(context.coverage("lcov"), @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    ");
    insta::assert_snapshot!(context.read_file("coverage.lcov"), @r"
    SF:src/app.py
    DA:1,1
    DA:2,0
    LF:2
    LH:1
    end_of_record
    ");
}

#[test]
fn report_auto_combines_pending_shards() {
    let context = TestContext::new();
    write_coverage(
        &context,
        Utf8Path::new(".karva/coverage/pending/shard-a.json"),
    );

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
    karva failed
      Cause: no coverage data found at `<temp_dir>/.karva/coverage/data.json`; run `uv run karva test --cov` first
    ");
}

#[test]
fn report_fail_under_returns_failure() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(context.coverage("report").arg("--fail-under=75"), @"
    success: false
    exit_code: 2
    ----- stdout -----

    Name         Stmts   Miss   Cover
    [LONG-LINE]
    src/app.py       2      1     50%
    [LONG-LINE]
    TOTAL            2      1     50%

    ----- stderr -----
    coverage failure: required total coverage of 75% not reached, total coverage was 50%
    ");
}

#[test]
fn report_total_outputs_only_percentage() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(context.coverage("report").args(["--format", "total"]), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    50

    ----- stderr -----
    ");
}

#[test]
fn report_show_missing_lists_ranges() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(context.coverage("report").arg("--show-missing"), @r"
    success: true
    exit_code: 0
    ----- stdout -----

    Name         Stmts   Miss   Cover   Missing
    [LONG-LINE]
    src/app.py       2      1     50%   2
    [LONG-LINE]
    TOTAL            2      1     50%

    ----- stderr -----
    ");
}

#[test]
fn report_rejects_unmatched_selector() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(context.coverage("report").arg("missing.module"), @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: coverage selector `missing.module` matched no source files
    ");
}

#[test]
fn report_writes_and_appends_markdown() {
    let context = TestContext::new();
    write_coverage(&context, Utf8Path::new(".karva/coverage/data.json"));

    assert_cmd_snapshot!(
        context
            .coverage("report")
            .args(["--format", "markdown", "--output", "summary.md"]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "
    );
    assert_cmd_snapshot!(
        context.coverage("report").args([
            "--format",
            "markdown",
            "--output",
            "summary.md",
            "--append",
        ]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "
    );
    insta::assert_snapshot!(context.read_file("summary.md"), @r"
    | Name | Stmts | Miss | Cover |
    | --- | ---: | ---: | ---: |
    | src/app.py | 2 | 1 | 50% |
    | **TOTAL** | 2 | 1 | **50%** |
    | Name | Stmts | Miss | Cover |
    | --- | ---: | ---: | ---: |
    | src/app.py | 2 | 1 | 50% |
    | **TOTAL** | 2 | 1 | **50%** |
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
    2 | def test_failure():
      |     ^^^^^^^^^^^^
    info: Test failed here
     --> test_failure.py:3:5
    3 |     assert False
      |     ^^^^^^^^^^^^

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
