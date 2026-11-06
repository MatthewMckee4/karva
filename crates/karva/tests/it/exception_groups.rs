use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;
use serde_json::Value;

use crate::common::TestContext;

#[rstest]
fn reports_nested_exception_groups(#[values("json", "jsonl")] format: &str) {
    let context = TestContext::with_files([
        (
            "karva.toml",
            "[profile.default.junit]\npath = 'results.xml'\n",
        ),
        (
            "test_groups.py",
            r#"
import karva

def test_nested():
    try:
        raise ValueError("first leaf")
    except ValueError as first:
        first.add_note("leaf note")
        nested = ExceptionGroup("inner group", [first, TypeError("second leaf")])
    try:
        raise LookupError("explicit cause")
    except LookupError as cause:
        group = ExceptionGroup("outer group", [nested])
        group.add_note("group note")
        raise group from cause

def test_pass():
    assert True
"#,
        ),
    ]);
    insta::allow_duplicates! { assert_cmd_snapshot!(context.command_no_parallel().args([
        "--status-level=none",
        &format!("--result-format={format}"),
        "--result-output=results",
    ]), @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_groups::test_nested:

    error[test-failure]: Test `test_nested` failed
     --> test_groups.py:4:5
      |
    4 | def test_nested():
      |     ^^^^^^^^^^^
    exception: ExceptionGroup: outer group
      at <temp_dir>/test_groups.py:15 in test_nested
      note: group note
    exception.cause: LookupError: explicit cause
      at <temp_dir>/test_groups.py:11 in test_nested
    exception.1: ExceptionGroup: inner group
    exception.1.1: ValueError: first leaf
      at <temp_dir>/test_groups.py:6 in test_nested
      note: leaf note
    exception.1.2: TypeError: second leaf

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    "); }
    {
        let report = context.read_file("results");
        let diagnostic = if format == "json" {
            let report: Value = serde_json::from_str(&report).expect("JSON report");
            report["tests"][0]["diagnostic"].clone()
        } else {
            report
                .lines()
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .find_map(|record| record.pointer("/diagnostic").cloned())
                .expect("test diagnostic")
        };
        assert_eq!(diagnostic["exception"]["message"], "outer group");
        assert_eq!(
            diagnostic["exception"]["children"][0]["children"][0]["message"],
            "first leaf"
        );
        assert_eq!(
            diagnostic["exception"]["children"][0]["children"][0]["notes"][0],
            "leaf note"
        );
        assert_eq!(
            diagnostic["exception"]["cause"]["message"],
            "explicit cause"
        );
        let xml = context.read_file("results.xml");
        assert!(xml.contains("exception.1.1: ValueError: first leaf"));
        assert!(xml.contains("leaf note"));
    }
}

#[test]
fn reports_task_group_and_fixture_groups() {
    let context = TestContext::with_file(
        "test_groups.py",
        r#"
import asyncio
import karva

@karva.fixture
def broken():
    raise ExceptionGroup("fixture group", [RuntimeError("fixture leaf")])

def test_fixture(broken):
    assert False, "body must not run"

async def test_tasks():
    async def fail(message):
        raise ValueError(message)
    async with asyncio.TaskGroup() as group:
        group.create_task(fail("task one"))
        group.create_task(fail("task two"))
"#,
    );
    insta::allow_duplicates! { assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_groups::test_fixture (requires fixture `broken`):

    error[fixture-failure]: Fixture `broken` failed
     --> test_groups.py:6:5
      |
    6 | def broken():
      |     ^^^^^^
    exception: ExceptionGroup: fixture group
      at <temp_dir>/test_groups.py:7 in broken
    exception.1: RuntimeError: fixture leaf

    test_groups::test_tasks:

    error[test-failure]: Test `test_tasks` failed
      --> test_groups.py:12:11
       |
    12 | async def test_tasks():
       |           ^^^^^^^^^^
    exception: ExceptionGroup: unhandled errors in a TaskGroup
      at <temp_dir>/test_groups.py:15 in test_tasks
    exception.1: ValueError: task one
      at <temp_dir>/test_groups.py:14 in fail
    exception.2: ValueError: task two
      at <temp_dir>/test_groups.py:14 in fail

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 1 failed, 1 error, 0 skipped

    ----- stderr -----
    "); }
}

#[rstest]
fn reports_exception_group_limits(#[values("json", "jsonl")] format: &str) {
    let context = TestContext::with_files([
        (
            "karva.toml",
            "[profile.default.junit]\npath = 'results.xml'\n",
        ),
        (
            "test_groups.py",
            r#"
def test_large():
    deep = ValueError("deep leaf")
    for _ in range(40):
        deep = ExceptionGroup("nested", [deep])
    raise ExceptionGroup("large", [deep, ExceptionGroup("wide", [ValueError(str(i)) for i in range(1000)])])
"#,
        ),
    ]);
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"(?s)exception:.*?\n\n", "[BOUNDED EXCEPTION TREE]\n\n");
    let _scope = settings.bind_to_scope();
    insta::allow_duplicates! { assert_cmd_snapshot!(context.command_no_parallel().args([
        "--status-level=none",
        &format!("--result-format={format}"),
        "--result-output=results",
    ]), @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_groups::test_large:

    error[test-failure]: Test `test_large` failed
     --> test_groups.py:2:5
      |
    2 | def test_large():
      |     ^^^^^^^^^^
    [BOUNDED EXCEPTION TREE]

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "); }
    let report = context.read_file("results");
    let diagnostic = if format == "json" {
        let report: Value = serde_json::from_str(&report).expect("JSON report");
        report["tests"][0]["diagnostic"].clone()
    } else {
        let report: Value = serde_json::from_str(report.lines().next().expect("test record"))
            .expect("JSONL report");
        report["diagnostic"].clone()
    };
    assert_eq!(
        diagnostic["exception"]["children"][1]["omitted_children"],
        744
    );
    assert_eq!(
        diagnostic["exception"]["children"][1]["children"]
            .as_array()
            .expect("retained children")
            .len(),
        256
    );
    assert!(
        diagnostic["rendered"]
            .as_str()
            .expect("rendered diagnostic")
            .contains("744 children hidden")
    );
    let xml = context.read_file("results.xml");
    // Receipt: this workload produced 153 KB JSON, 62 KB JSONL and 12 KB JUnit.
    // Allow generous platform/path variation while catching unbounded traversal.
    assert!(report.len() < 1024 * 1024);
    assert!(xml.len() < 1024 * 1024);
    assert!(xml.contains("744 children hidden"));
    assert!(xml.contains("width limit 256, depth limit 32, node limit 512"));
}
