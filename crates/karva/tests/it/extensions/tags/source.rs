use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[test]
fn external_source_diagnostics_and_case_results() {
    let context = TestContext::with_files([
        ("karva.toml", "[profile.ci.junit]\npath = 'results.xml'\n"),
        (
            "test_generated.py",
            r#"
import karva

source = karva.SourceDocument("docs/guide.rst", "Title\r\né 42\r\n>>> next\r\n")

@karva.tags.parametrize("value", [
    karva.param(1, id="pass", source=source.location(3, 1)),
    karva.param(2, id="fail", source=source.location(2, 3)),
    karva.param(3, id="skip", source=source.location(3, 1)),
])
def test_generated(value):
    if value == 3:
        karva.skip(reason="unavailable example")
    assert value != 2
"#,
        ),
    ]);
    assert_cmd_snapshot!(context.command().args([
        "--profile=ci",
        "--num-workers=2",
        "--status-level=none"
    ]), @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_generated::test_generated(fail):

    error[test-failure]: Test `test_generated` failed
     --> docs/guide.rst:2:3
      |
    2 | é 42
      |   ^
    info: Test ran with arguments:
    info:   `value`: `2`
    info: Test failed here
      --> test_generated.py:14:5
       |
    14 |     assert value != 2
       |     ^^^^^^^^^^^^^^^^^

    ────────────
         Summary [TIME] 3 tests run: 1 passed, 1 failed, 1 skipped

    ----- stderr -----
    ");
    let report = context.read_file("results.xml");
    let cases: Vec<_> = report
        .lines()
        .filter(|line| line.contains("<testcase"))
        .collect();
    assert_eq!(cases.len(), 3);
    for case in cases {
        assert!(case.contains("file=\"docs/guide.rst\""), "{case}");
        if case.contains("(fail)") {
            assert!(case.contains("line=\"2\" column=\"3\""), "{case}");
        } else {
            assert!(case.contains("line=\"3\" column=\"1\""), "{case}");
        }
    }
    assert_cmd_snapshot!(context.command().args([
        "--profile=ci",
        "--last-failed",
        "--status-level=none"
    ]), @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_generated::test_generated(fail):

    error[test-failure]: Test `test_generated` failed
     --> docs/guide.rst:2:3
      |
    2 | é 42
      |   ^
    info: Test ran with arguments:
    info:   `value`: `2`
    info: Test failed here
      --> test_generated.py:14:5
       |
    14 |     assert value != 2
       |     ^^^^^^^^^^^^^^^^^

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn external_source_invalid_coordinates(
    #[values("0, 1", "3, 1", "1, 0", "1, 4")] coordinates: &str,
) {
    let context = TestContext::with_file(
        "test_generated.py",
        &format!(
            r#"
import karva
source = karva.SourceDocument("guide.rst", "éx\n")
location = source.location({coordinates})

def test_generated():
    pass
"#
        ),
    );
    match coordinates {
        "0, 1" => assert_cmd_snapshot!(context.command(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
        diagnostics:

        error[failed-to-import-module]: Failed to import python module `test_generated`: Source line must be at least 1; received 0

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        "),
        "3, 1" => assert_cmd_snapshot!(context.command(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
        diagnostics:

        error[failed-to-import-module]: Failed to import python module `test_generated`: Source line 3 exceeds document line count 2

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        "),
        "1, 0" => assert_cmd_snapshot!(context.command(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
        diagnostics:

        error[failed-to-import-module]: Failed to import python module `test_generated`: Source column 0 must be in 1..=3 on line 1

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        "),
        _ => {
            assert_eq!(coordinates, "1, 4");
            assert_cmd_snapshot!(context.command(), @"
            success: false
            exit_code: 1
            ----- stdout -----
                Starting 1 test across 1 worker
            diagnostics:

            error[failed-to-import-module]: Failed to import python module `test_generated`: Source column 4 must be in 1..=3 on line 1

            ────────────
                 Summary [TIME] 0 tests run: 0 passed, 0 skipped

            ----- stderr -----
            ");
        }
    }
}

#[test]
fn external_source_rejects_ambiguous_stacked_origins() {
    let context = TestContext::with_file(
        "test_generated.py",
        r#"
import karva
source = karva.SourceDocument("guide.rst", "example\n")

@karva.tags.parametrize("first", [karva.param(1, source=source.location(1, 1))])
@karva.tags.parametrize("second", [karva.param(2, source=source.location(1, 2))])
def test_generated(first, second):
    pass
"#,
    );
    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test_generated::test_generated

    failures:

    test_generated::test_generated:

    error[invalid-parametrize]: Source locations may appear in only one parametrization dimension
     --> test_generated.py:7:5
      |
    7 | def test_generated(first, second):
      |     ^^^^^^^^^^^^^^

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn external_source_survives_retries_and_fixture_errors() {
    let context = TestContext::with_files([
        ("karva.toml", "[profile.ci.junit]\npath = 'results.xml'\n"),
        (
            "test_generated.py",
            r#"
from pathlib import Path
import karva
source = karva.SourceDocument("guide.rst", "retry\nfixture\n")

@karva.fixture
def broken():
    raise ValueError("fixture failed")

@karva.tags.parametrize("value", [karva.param(1, id="retry", source=source.location(1, 1))])
def test_retry(value):
    attempt = Path("attempt.txt")
    retried = attempt.exists()
    attempt.write_text("attempted")
    assert retried

@karva.tags.parametrize("value", [karva.param(1, id="fixture", source=source.location(2, 1))])
def test_fixture(value, broken):
    pass
"#,
        ),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--profile=ci",
        "--retry=1",
        "--status-level=none"
    ]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_generated::test_fixture(fixture) (requires fixture `broken`):

    error[fixture-failure]: Fixture `broken` failed
     --> test_generated.py:7:5
      |
    7 | def broken():
      |     ^^^^^^
    info: Fixture failed here
     --> test_generated.py:8:5
      |
    8 |     raise ValueError("fixture failed")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: fixture failed

    ────────────
         Summary [TIME] 2 tests run: 1 passed (1 flaky), 1 error, 0 skipped
       FLAKY 2/2 [TIME] test_generated::test_retry(retry)

    ----- stderr -----
    "#);
    let report = context.read_file("results.xml");
    assert!(report.contains("file=\"guide.rst\" line=\"1\" column=\"1\""));
    assert!(report.contains("file=\"guide.rst\" line=\"2\" column=\"1\""));
}

#[test]
fn external_source_optional_metadata_and_stacked_params() {
    let context = TestContext::with_files([
        ("karva.toml", "[profile.ci.junit]\npath = 'results.xml'\n"),
        (
            "test_generated.py",
            r#"
import karva
source = karva.SourceDocument("guide.rst", "éx\n")

@karva.tags.parametrize("first", [karva.param(1, source=source.location(1, 3))])
@karva.tags.parametrize("second", [2, 3])
def test_generated(first, second):
    assert first + second == second + 1

@karva.tags.parametrize("value", [karva.param(1, tags=(karva.tags.skip,), source=source.location(2, 1))])
def test_skipped(value):
    raise AssertionError("must not run")

@karva.tags.parametrize("value", [karva.param(1)])
def test_plain(value):
    assert value == 1
"#,
        ),
    ]);
    assert_cmd_snapshot!(context.command().args([
        "--profile=ci",
        "--num-workers=2",
        "--status-level=none"
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 4 tests run: 3 passed, 1 skipped

    ----- stderr -----
    ");
    let report = context.read_file("results.xml");
    assert_eq!(report.matches("<testcase").count(), 4);
    assert_eq!(
        report
            .matches("file=\"guide.rst\" line=\"1\" column=\"3\"")
            .count(),
        2
    );
    assert_eq!(
        report
            .matches("file=\"guide.rst\" line=\"2\" column=\"1\"")
            .count(),
        1
    );
}

#[test]
fn external_source_rejects_empty_path() {
    let context = TestContext::with_file(
        "test_generated.py",
        "import karva\nsource = karva.SourceDocument('', 'example')\ndef test_generated():\n    pass\n",
    );
    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test_generated`: Source document path cannot be empty

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}
