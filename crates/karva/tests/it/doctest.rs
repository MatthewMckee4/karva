use insta_cmd::assert_cmd_snapshot;
use serde_json::Value;

use crate::common::TestContext;

const DOCTEST_FIXTURE: &str = r#"
"""Module examples.

>>> 1 + 1
2
"""

def documented():
    """Function examples.

    >>> 2 + 2
    5
    """

def raises_unexpectedly():
    """An unexpected exception includes its type and message.

    >>> 1 / 0
    0
    """

def test_regular():
    pass
"#;

#[test]
fn doctest_modules_is_opt_in() {
    let context = TestContext::with_file("test_examples.py", DOCTEST_FIXTURE);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_examples::test_regular
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.command_no_parallel().arg("--doctest-modules"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 4 tests across 1 worker
            PASS [TIME] test_examples::doctest:@module
            FAIL [TIME] test_examples::doctest:documented
            FAIL [TIME] test_examples::doctest:raises_unexpectedly
            PASS [TIME] test_examples::test_regular

    failures:

    test_examples::doctest:documented:

    error[test-failure]: Test `doctest:documented` failed
      --> test_examples.py:11:5
       |
    11 |     >>> 2 + 2
       |     ^^^
       |
    info: Doctest failed at test_examples.py:11
          Example:
            2 + 2
          Expected:
            5
          Got:
            4

    test_examples::doctest:raises_unexpectedly:

    error[test-failure]: Test `doctest:raises_unexpectedly` failed
      --> test_examples.py:18:5
       |
    18 |     >>> 1 / 0
       |     ^^^
       |
    info: Doctest raised at test_examples.py:18
          Example:
            1 / 0
          Exception:
            ZeroDivisionError: division by zero

    ────────────
         Summary [TIME] 4 tests run: 2 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn doctest_modules_can_be_enabled_in_configuration() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.test]
doctest-modules = true
"#,
        ),
        ("test_examples.py", DOCTEST_FIXTURE),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 4 tests across 1 worker
            PASS [TIME] test_examples::doctest:@module
            FAIL [TIME] test_examples::doctest:documented
            FAIL [TIME] test_examples::doctest:raises_unexpectedly
            PASS [TIME] test_examples::test_regular

    failures:

    test_examples::doctest:documented:

    error[test-failure]: Test `doctest:documented` failed
      --> test_examples.py:11:5
       |
    11 |     >>> 2 + 2
       |     ^^^
       |
    info: Doctest failed at test_examples.py:11
          Example:
            2 + 2
          Expected:
            5
          Got:
            4

    test_examples::doctest:raises_unexpectedly:

    error[test-failure]: Test `doctest:raises_unexpectedly` failed
      --> test_examples.py:18:5
       |
    18 |     >>> 1 / 0
       |     ^^^
       |
    info: Doctest raised at test_examples.py:18
          Example:
            1 / 0
          Exception:
            ZeroDivisionError: division by zero

    ────────────
         Summary [TIME] 4 tests run: 2 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.command_no_parallel().arg("--doctest-modules=false"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_examples::test_regular
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn doctest_selector_runs_one_document() {
    let context = TestContext::with_file("test_examples.py", DOCTEST_FIXTURE);

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--doctest-modules", "test_examples.py::doctest:@module"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_examples::doctest:@module
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn doctest_objects_share_state_and_honor_directives() {
    let context = TestContext::with_file(
        "test_objects.py",
        r#"
def stateful():
    """Examples share their namespace.

    >>> values = []
    >>> values.append(1)
    >>> values
    [1]
    >>> "alphabet"  # doctest: +ELLIPSIS
    'alpha...'
    >>> print("left    right")  # doctest: +NORMALIZE_WHITESPACE
    left right
    >>> missing_name  # doctest: +SKIP
    """

class Calculator:
    """A documented class.

    >>> Calculator().multiply(2, 3)
    6
    """

    def multiply(self, left, right):
        """Multiply two values.

        >>> Calculator().multiply(3, 4)
        12
        """
        return left * right
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--doctest-modules"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test_objects::doctest:stateful
            PASS [TIME] test_objects::doctest:Calculator
            PASS [TIME] test_objects::doctest:Calculator.multiply
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn doctests_validate_strict_module_tags() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.test]
doctest-modules = true
strict-tags = true
"#,
        ),
        (
            "test_tagged.py",
            r#"
"""
>>> 1 + 1
2
"""

import pytest

pytestmark = pytest.mark.typo
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[unknown-tag]: Tag `typo` is not registered
     --> test_tagged.py:9:26
      |
    9 | pytestmark = pytest.mark.typo
      |                          ^^^^ unregistered tag
      |
    info: Register `typo` in the project-wide `[tags]` table.

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn doctests_require_a_runtime_visible_docstring() {
    let context = TestContext::with_file(
        "test_decorated.py",
        r#"
def hide_docstring(function):
    return object()

@hide_docstring
def documented():
    """
    >>> 1 + 1
    2
    """

def test_regular():
    pass
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--doctest-modules"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_decorated::test_regular
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn doctest_ids_are_stable_in_reports() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.junit]
path = "reports/results.xml"
"#,
        ),
        (
            "test_reports.py",
            r#"
"""Module examples.

>>> 1 + 1
2
"""

def test_regular():
    pass

def module():
    """An object whose name cannot collide with the module case.

    >>> 2 + 2
    4
    """

def broken():
    """A failing example is preserved in machine-readable reports.

    >>> 3 * 3
    10
    """
"#,
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--doctest-modules",
            "--status-level=none",
            "--result-output=reports/results.json",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_reports::doctest:broken:

    error[test-failure]: Test `doctest:broken` failed
      --> test_reports.py:21:5
       |
    21 |     >>> 3 * 3
       |     ^^^
       |
    info: Doctest failed at test_reports.py:21
          Example:
            3 * 3
          Expected:
            10
          Got:
            9

    ────────────
         Summary [TIME] 4 tests run: 3 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    let report: Value = serde_json::from_str(&context.read_file("reports/results.json"))
        .expect("JSON report should parse");
    let doctest = report["tests"]
        .as_array()
        .expect("JSON tests should be an array")
        .iter()
        .find(|test| test["name"] == "doctest:@module")
        .expect("JSON report should include module doctest");
    assert_eq!(doctest["module"], "test_reports");
    assert_eq!(doctest["full_name"], "test_reports::doctest:@module");
    assert!(
        report["tests"]
            .as_array()
            .expect("JSON tests should be an array")
            .iter()
            .any(|test| test["full_name"] == "test_reports::doctest:module"),
        "JSON report should distinguish an object named module"
    );
    let failed = report["tests"]
        .as_array()
        .expect("JSON tests should be an array")
        .iter()
        .find(|test| test["name"] == "doctest:broken")
        .expect("JSON report should include failing doctest");
    assert_eq!(failed["status"], "failed");
    let diagnostic = failed["diagnostic"]["rendered"]
        .as_str()
        .expect("failing JSON doctest should include a diagnostic");
    assert!(diagnostic.contains("Expected:\n        10"));
    assert!(diagnostic.contains("Got:\n        9"));

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--doctest-modules",
            "--status-level=none",
            "--result-output=reports/results.jsonl",
            "--result-format=jsonl",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_reports::doctest:broken:

    error[test-failure]: Test `doctest:broken` failed
      --> test_reports.py:21:5
       |
    21 |     >>> 3 * 3
       |     ^^^
       |
    info: Doctest failed at test_reports.py:21
          Example:
            3 * 3
          Expected:
            10
          Got:
            9

    ────────────
         Summary [TIME] 4 tests run: 3 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    let doctest = context
        .read_file("reports/results.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record should parse"))
        .find(|record| record["type"] == "test" && record["name"] == "doctest:@module")
        .expect("JSONL report should include module doctest");
    assert_eq!(doctest["module"], "test_reports");
    assert_eq!(doctest["full_name"], "test_reports::doctest:@module");

    assert!(
        context
            .read_file("reports/results.jsonl")
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record should parse"))
            .any(|record| record["full_name"] == "test_reports::doctest:module"),
        "JSONL report should distinguish an object named module"
    );

    let failed = context
        .read_file("reports/results.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record should parse"))
        .find(|record| record["name"] == "doctest:broken")
        .expect("JSONL report should include failing doctest");
    assert_eq!(failed["status"], "failed");
    let diagnostic = failed["diagnostic"]["rendered"]
        .as_str()
        .expect("failing JSONL doctest should include a diagnostic");
    assert!(diagnostic.contains("Expected:\n        10"));
    assert!(diagnostic.contains("Got:\n        9"));

    let junit = context.read_file("reports/results.xml");
    assert!(
        junit.contains(r#"<testcase classname="test_reports" name="doctest:@module""#),
        "JUnit report should include module doctest ID"
    );
    assert!(
        junit.contains(r#"<testcase classname="test_reports" name="doctest:module""#),
        "JUnit report should distinguish an object named module"
    );
    assert!(
        junit.contains(r#"<testcase classname="test_reports" name="doctest:broken""#)
            && junit.contains("Expected:")
            && junit.contains("Got:"),
        "JUnit report should include the failing doctest diagnostic"
    );
}
