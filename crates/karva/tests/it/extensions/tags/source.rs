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
    ]));
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
    ]));
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
    assert_cmd_snapshot!(
        format!("external_source_invalid_{}", coordinates.replace(", ", "_")),
        context.command()
    );
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
    assert_cmd_snapshot!(context.command());
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
    ]));
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
    ]));
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
    assert_cmd_snapshot!(context.command());
}
