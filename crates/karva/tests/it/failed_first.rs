use insta_cmd::assert_cmd_snapshot;
use karva_static::EnvVars;

use crate::common::TestContext;

fn priority_context(config: Option<&str>) -> TestContext {
    let context = TestContext::with_file(
        "test_order.py",
        r#"
from pathlib import Path

def record(name):
    with Path("order").open("a", encoding="utf-8") as output:
        output.write(f"{name}\n")

def test_a():
    record("a")

def test_b():
    record("b")
    assert Path("fixed").exists()

def test_c():
    record("c")
"#,
    );
    if let Some(config) = config {
        context.write_file("karva.toml", config);
    }
    context
}

fn seed_failed_test(context: &TestContext) {
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "test_order.py::test_b",
        "--status-level=none",
    ]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_order::test_b:

    error[test-failure]: Test `test_b` failed
      --> test_order.py:11:5
       |
    11 | def test_b():
       |     ^^^^^^
    info: Test failed here
      --> test_order.py:13:5
       |
    13 |     assert Path("fixed").exists()
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
    context.write_file("fixed", "");
    context.write_file("order", "");
}

#[test]
fn failed_first_runs_cached_failure_before_other_tests() {
    let context = priority_context(None);
    seed_failed_test(&context);

    assert_cmd_snapshot!(context.command_no_parallel().args(["--failed-first", "--status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");

    let order = context.read_file("order");
    assert_eq!(order.lines().next(), Some("b"));
    assert_eq!(order.lines().count(), 3);
}

#[test]
fn failed_first_uses_profile_configuration() {
    let context = priority_context(Some("[profile.ci.test]\nfailed-first = true\n"));
    seed_failed_test(&context);

    assert_cmd_snapshot!(context.command_no_parallel().args(["--profile=ci", "--status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
    assert_eq!(context.read_file("order").lines().next(), Some("b"));
}

#[test]
fn failed_first_environment_variable_enables_priority() {
    let context = priority_context(None);
    seed_failed_test(&context);

    assert_cmd_snapshot!(context.command_no_parallel().env(EnvVars::KARVA_FAILED_FIRST, "true").args(["--status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
    assert_eq!(context.read_file("order").lines().next(), Some("b"));
}

#[test]
fn failed_first_with_cache_disabled_runs_selected_suite() {
    let context = priority_context(None);
    seed_failed_test(&context);

    assert_cmd_snapshot!(context.command_no_parallel().args(["--failed-first", "--no-cache", "--status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
    assert_eq!(context.read_file("order").lines().count(), 3);
}

#[test]
fn failed_first_applies_partition_after_selection() {
    let context = priority_context(None);
    seed_failed_test(&context);

    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--failed-first",
        "--partition=slice:1/2",
        "--status-level=none",
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
    let order = context.read_file("order");
    let mut selected = order.lines().collect::<Vec<_>>();
    selected.sort_unstable();
    assert_eq!(selected, ["a", "c"]);
}

#[test]
fn failed_first_prioritizes_cached_parametrize_case() {
    let context = TestContext::with_file(
        "test_param.py",
        r#"
from pathlib import Path
import karva

@karva.tags.parametrize("value", [0, 1, 2])
def test_value(value):
    with Path("order").open("a", encoding="utf-8") as output:
        output.write(f"{value}\n")
    assert value != 1 or Path("fixed").exists()
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "test_param.py::test_value[1]",
        "--status-level=none",
    ]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_param::test_value(value=1):

    error[test-failure]: Test `test_value` failed
     --> test_param.py:6:5
      |
    6 | def test_value(value):
      |     ^^^^^^^^^^
    info: Test ran with arguments:
    info:   `value`: `1`
    info: Test failed here
     --> test_param.py:9:5
      |
    9 |     assert value != 1 or Path("fixed").exists()
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);

    context.write_file("fixed", "");
    context.write_file("order", "");
    assert_cmd_snapshot!(context.command_no_parallel().args(["--failed-first", "--status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
    assert_eq!(context.read_file("order").lines().next(), Some("1"));
}

#[test]
fn failed_first_interleaves_cached_parametrize_case_across_modules() {
    let context = TestContext::with_files([
        (
            "test_a.py",
            r#"
from pathlib import Path
import karva

@karva.tags.parametrize("value", [0, 1])
def test_a(value):
    with Path("order").open("a", encoding="utf-8") as output:
        output.write(f"a{value}\n")
    assert value != 1 or Path("fixed").exists()
"#,
        ),
        (
            "test_b.py",
            r#"
from pathlib import Path

def test_b():
    with Path("order").open("a", encoding="utf-8") as output:
        output.write("b\n")
    assert Path("fixed").exists()
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel().args(["--status-level=none"]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_a::test_a(value=1):

    error[test-failure]: Test `test_a` failed
     --> test_a.py:6:5
      |
    6 | def test_a(value):
      |     ^^^^^^
    info: Test ran with arguments:
    info:   `value`: `1`
    info: Test failed here
     --> test_a.py:9:5
      |
    9 |     assert value != 1 or Path("fixed").exists()
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

    test_b::test_b:

    error[test-failure]: Test `test_b` failed
     --> test_b.py:4:5
      |
    4 | def test_b():
      |     ^^^^^^
    info: Test failed here
     --> test_b.py:7:5
      |
    7 |     assert Path("fixed").exists()
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 failed, 0 skipped

    ----- stderr -----
    "#);

    context.write_file("fixed", "");
    context.write_file("order", "");
    assert_cmd_snapshot!(context.command_no_parallel().args(["--failed-first", "--status-level=none"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    "#);

    let order = context.read_file("order");
    let events = order.lines().collect::<Vec<_>>();
    assert_eq!(events.last(), Some(&"a0"));
    let mut cached_failures = events[..2].to_vec();
    cached_failures.sort_unstable();
    assert_eq!(cached_failures, ["a1", "b"]);
}

#[test]
fn failed_first_orders_failures_across_modules_and_preserves_fixture_scopes() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
from pathlib import Path
import karva

LOG = Path("order")

def record(value):
    with LOG.open("a", encoding="utf-8") as output:
        output.write(f"{value}\n")

@karva.fixture(scope="session", auto_use=True)
def session_lifecycle():
    record("session_setup")
    yield
    record("session_teardown")

@karva.fixture(scope="package", auto_use=True)
def package_lifecycle():
    record("package_setup")
    yield
    record("package_teardown")

@karva.fixture(scope="module", auto_use=True)
def module_lifecycle():
    record("module_setup")
    yield
    record("module_teardown")
"#,
        ),
        (
            "test_a.py",
            r#"
from pathlib import Path

def test_a_fail():
    with Path("order").open("a", encoding="utf-8") as output:
        output.write("a_fail\n")
    assert Path("fixed").exists()

def test_a_pass():
    with Path("order").open("a", encoding="utf-8") as output:
        output.write("a_pass\n")
"#,
        ),
        (
            "nested/conftest.py",
            r#"
import karva

@karva.fixture(scope="package", auto_use=True)
def nested_package_lifecycle():
    with open("order", "a", encoding="utf-8") as output:
        output.write("nested_setup\n")
    yield
    with open("order", "a", encoding="utf-8") as output:
        output.write("nested_teardown\n")
"#,
        ),
        (
            "nested/test_b.py",
            r#"
from pathlib import Path

def test_b_fail():
    with Path("order").open("a", encoding="utf-8") as output:
        output.write("b_fail\n")
    assert Path("fixed").exists()

def test_b_pass():
    with Path("order").open("a", encoding="utf-8") as output:
        output.write("b_pass\n")
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel().args(["--status-level=none"]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    nested.test_b::test_b_fail:

    error[test-failure]: Test `test_b_fail` failed
     --> nested/test_b.py:4:5
      |
    4 | def test_b_fail():
      |     ^^^^^^^^^^^
    info: Test failed here
     --> nested/test_b.py:7:5
      |
    7 |     assert Path("fixed").exists()
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

    test_a::test_a_fail:

    error[test-failure]: Test `test_a_fail` failed
     --> test_a.py:4:5
      |
    4 | def test_a_fail():
      |     ^^^^^^^^^^^
    info: Test failed here
     --> test_a.py:7:5
      |
    7 |     assert Path("fixed").exists()
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

    ────────────
         Summary [TIME] 4 tests run: 2 passed, 2 failed, 0 skipped

    ----- stderr -----
    "#);

    context.write_file("fixed", "");
    context.write_file("order", "");
    assert_cmd_snapshot!(context.command_no_parallel().args(["--failed-first", "--status-level=none"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 4 tests run: 4 passed, 0 skipped

    ----- stderr -----
    "#);

    let order = context.read_file("order");
    let events = order.lines().collect::<Vec<_>>();
    let test_events = events
        .iter()
        .copied()
        .filter(|event| matches!(*event, "a_fail" | "b_fail" | "a_pass" | "b_pass"))
        .collect::<Vec<_>>();
    assert_eq!(test_events.len(), 4);
    assert!(
        test_events[..2]
            .iter()
            .all(|event| event.ends_with("_fail"))
    );
    assert!(
        test_events[2..]
            .iter()
            .all(|event| event.ends_with("_pass"))
    );
    for event in [
        "session_setup",
        "session_teardown",
        "package_setup",
        "package_teardown",
        "nested_setup",
        "nested_teardown",
    ] {
        assert_eq!(
            events
                .iter()
                .filter(|candidate| **candidate == event)
                .count(),
            1
        );
    }
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == "module_setup")
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == "module_teardown")
            .count(),
        2
    );
}
