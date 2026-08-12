use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
use regex::Regex;
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
    info: Expected output:
            5
          Actual output:
            4

    test_examples::doctest:raises_unexpectedly:

    error[test-failure]: Test `doctest:raises_unexpectedly` failed
      --> test_examples.py:18:5
       |
    18 |     >>> 1 / 0
       |     ^^^
       |
    info: Unexpected exception:
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
            r"
[profile.default.test]
doctest-modules = true
",
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
    info: Expected output:
            5
          Actual output:
            4

    test_examples::doctest:raises_unexpectedly:

    error[test-failure]: Test `doctest:raises_unexpectedly` failed
      --> test_examples.py:18:5
       |
    18 |     >>> 1 / 0
       |     ^^^
       |
    info: Unexpected exception:
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
fn doctest_failure_points_to_the_failing_example() {
    let context = TestContext::with_file(
        "test_location.py",
        r#"
def documented():
    """Two examples.

    >>> 1 + 1
    2
    >>> 2 + 2
    5
    """
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--doctest-modules"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_location::doctest:documented

    failures:

    test_location::doctest:documented:

    error[test-failure]: Test `doctest:documented` failed
     --> test_location.py:7:5
      |
    7 |     >>> 2 + 2
      |     ^^^
      |
    info: Expected output:
            5
          Actual output:
            4

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn doctests_validate_strict_module_tags() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[profile.default.test]
doctest-modules = true
strict-tags = true
",
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
fn doctests_ignore_module_parametrization() {
    let context = TestContext::with_file(
        "test_parametrized.py",
        r#"
"""
>>> 1 + 1
2
"""

import pytest

pytestmark = pytest.mark.parametrize("unused", [1, 2])
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--doctest-modules"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_parametrized::doctest:@module
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn doctests_require_a_runtime_visible_docstring() {
    let context = TestContext::with_file(
        "test_decorated.py",
        r#"
from pathlib import Path

import pytest

@pytest.fixture(autouse=True)
def automatic():
    Path("fixture-ran").touch()

def hide_docstring(function):
    return object()

@hide_docstring
def documented():
    """
    >>> 1 + 1
    2
    """
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--doctest-modules"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");

    assert!(!context.root().join("fixture-ran").exists());
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
    info: Expected output:
            10
          Actual output:
            9

    ────────────
         Summary [TIME] 4 tests run: 3 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    let report: Value = serde_json::from_str(&context.read_file("reports/results.json"))
        .expect("JSON report should parse");
    assert_snapshot!(
        serde_json::to_string_pretty(&report).expect("JSON report should serialize"),
        @r#"
    {
      "elapsed_seconds": "[TIME]",
      "schema_version": 2,
      "stats": {
        "errors": 0,
        "failed": 1,
        "flaky": 0,
        "passed": 3,
        "skipped": 0,
        "slow": 0,
        "total": 4
      },
      "status": "failed",
      "tests": [
        {
          "duration_seconds": "[TIME]",
          "full_name": "test_reports::doctest:@module",
          "module": "test_reports",
          "name": "doctest:@module",
          "status": "passed"
        },
        {
          "diagnostic": {
            "code": "test-failure",
            "message": "Test `doctest:broken` failed",
            "rendered": "error[test-failure]: Test `doctest:broken` failed\n  --> test_reports.py:21:5\n   |/n21 |     >>> 3 * 3\n   |     ^^^\n   |/ninfo: Expected output:\n        10\n      Actual output:\n        9\n\n",
            "severity": "error"
          },
          "duration_seconds": "[TIME]",
          "full_name": "test_reports::doctest:broken",
          "module": "test_reports",
          "name": "doctest:broken",
          "status": "failed"
        },
        {
          "duration_seconds": "[TIME]",
          "full_name": "test_reports::doctest:module",
          "module": "test_reports",
          "name": "doctest:module",
          "status": "passed"
        },
        {
          "duration_seconds": "[TIME]",
          "full_name": "test_reports::test_regular",
          "module": "test_reports",
          "name": "test_regular",
          "status": "passed"
        }
      ]
    }
    "#
    );

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
    info: Expected output:
            10
          Actual output:
            9

    ────────────
         Summary [TIME] 4 tests run: 3 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    let records = context
        .read_file("reports/results.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record should parse"))
        .collect::<Vec<_>>();
    assert_snapshot!(
        serde_json::to_string_pretty(&records).expect("JSONL records should serialize"),
        @r#"
    [
      {
        "duration_seconds": "[TIME]",
        "full_name": "test_reports::doctest:@module",
        "module": "test_reports",
        "name": "doctest:@module",
        "schema_version": 2,
        "status": "passed",
        "type": "test"
      },
      {
        "diagnostic": {
          "code": "test-failure",
          "message": "Test `doctest:broken` failed",
          "rendered": "error[test-failure]: Test `doctest:broken` failed\n  --> test_reports.py:21:5\n   |/n21 |     >>> 3 * 3\n   |     ^^^\n   |/ninfo: Expected output:\n        10\n      Actual output:\n        9\n\n",
          "severity": "error"
        },
        "duration_seconds": "[TIME]",
        "full_name": "test_reports::doctest:broken",
        "module": "test_reports",
        "name": "doctest:broken",
        "schema_version": 2,
        "status": "failed",
        "type": "test"
      },
      {
        "duration_seconds": "[TIME]",
        "full_name": "test_reports::doctest:module",
        "module": "test_reports",
        "name": "doctest:module",
        "schema_version": 2,
        "status": "passed",
        "type": "test"
      },
      {
        "duration_seconds": "[TIME]",
        "full_name": "test_reports::test_regular",
        "module": "test_reports",
        "name": "test_regular",
        "schema_version": 2,
        "status": "passed",
        "type": "test"
      },
      {
        "elapsed_seconds": "[TIME]",
        "schema_version": 2,
        "stats": {
          "errors": 0,
          "failed": 1,
          "flaky": 0,
          "passed": 3,
          "skipped": 0,
          "slow": 0,
          "total": 4
        },
        "status": "failed",
        "type": "run_finished"
      }
    ]
    "#
    );

    let junit = Regex::new(r#"time="[0-9.]+""#)
        .expect("valid time regex")
        .replace_all(
            &context.read_file("reports/results.xml"),
            r#"time="[TIME]""#,
        )
        .to_string();
    assert_snapshot!(junit, @r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <testsuites name="karva-tests" tests="4" failures="1" skipped="0" errors="0" time="[TIME]">
      <testsuite name="test_reports" tests="4" failures="1" skipped="0" errors="0" time="[TIME]">
        <testcase classname="test_reports" name="doctest:@module" time="[TIME]"/>
        <testcase classname="test_reports" name="doctest:broken" time="[TIME]">
          <failure message="Test `doctest:broken` failed" type="test-failure">error[test-failure]: Test `doctest:broken` failed
      --&gt; test_reports.py:21:5
       |
    21 |     &gt;&gt;&gt; 3 * 3
       |     ^^^
       |
    info: Expected output:
            10
          Actual output:
            9

    </failure>
        </testcase>
        <testcase classname="test_reports" name="doctest:module" time="[TIME]"/>
        <testcase classname="test_reports" name="test_regular" time="[TIME]"/>
      </testsuite>
    </testsuites>
    "#);
}
