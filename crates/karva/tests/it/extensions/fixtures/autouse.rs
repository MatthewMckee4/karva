use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

fn get_auto_use_kw(framework: &str) -> &str {
    match framework {
        "pytest" => "autouse",
        "karva" => "auto_use",
        _ => panic!("Invalid framework"),
    }
}

#[rstest]
fn test_function_scope_auto_use_fixture(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        format!(
            r#"
import {framework}

arr = []

@{framework}.fixture(scope="function", {auto_use_kw}=True)
def auto_function_fixture():
    arr.append(1)
    yield
    arr.append(2)

def test_something():
    assert arr == [1]

def test_something_else():
    assert arr == [1, 2, 1]
"#,
            auto_use_kw = get_auto_use_kw(framework),
        )
        .as_str(),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_something
                PASS [TIME] test::test_something_else
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_scope_auto_use_fixture(
    #[values("pytest", "karva")] framework: &str,
    #[values("module", "package", "session")] scope: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import {framework}

arr = []

@{framework}.fixture(scope="{scope}", {auto_use_kw}=True)
def auto_function_fixture():
    arr.append(1)
    yield
    arr.append(2)

def test_something():
    assert arr == [1]

def test_something_else():
    assert arr == [1]
"#,
            auto_use_kw = get_auto_use_kw(framework),
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_something
                PASS [TIME] test::test_something_else
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_auto_use_fixture(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
                from {framework} import fixture

                @fixture
                def first_entry():
                    return "a"

                @fixture
                def order(first_entry):
                    return []

                @fixture({auto_use_kw}=True)
                def append_first(order, first_entry):
                    return order.append(first_entry)

                def test_string_only(order, first_entry):
                    assert order == [first_entry]

                def test_string_and_int(order, first_entry):
                    order.append(2)
                    assert order == [first_entry, 2]
                "#,
            auto_use_kw = get_auto_use_kw(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_string_only(order=['a'], first_entry=a)
                PASS [TIME] test::test_string_and_int(order=['a'], first_entry=a)
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}
#[test]
fn test_auto_use_fixture_in_parent_module() {
    let context = TestContext::with_files([
        (
            "foo/conftest.py",
            "
            import karva

            arr = []

            @karva.fixture(auto_use=True)
            def global_fixture():
                arr.append(1)
                yield
                arr.append(2)
            ",
        ),
        (
            "foo/inner/test_file2.py",
            "
            from ..conftest import arr

            def test_function1():
                assert arr == [1], arr

            def test_function2():
                assert arr == [1, 2, 1], arr
            ",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] foo.inner.test_file2::test_function1
            PASS [TIME] foo.inner.test_file2::test_function2
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_auto_use_fixture_setup_failure() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture(auto_use=True)
def failing_fixture():
    raise RuntimeError("Setup failed!")

def test_something():
    assert True

def test_something_else():
    assert True
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_something
            PASS [TIME] test::test_something_else
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_auto_use_fixture_teardown_failure() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture(auto_use=True)
def failing_teardown_fixture():
    yield
    raise RuntimeError("Teardown failed!")

def test_something():
    assert True


"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_something

    diagnostics:

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `failing_teardown_fixture`
     --> test.py:5:5
      |
    5 | def failing_teardown_fixture():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Failed to reset fixture: Teardown failed!

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_auto_use_fixture_with_failing_dependency() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture
def failing_dep():
    raise ValueError("Dependency failed!")

@karva.fixture(auto_use=True)
def auto_fixture(failing_dep):
    return "should not reach here"

def test_something():
    assert True
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_something
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_scoped_auto_use_fixture_setup_failure() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture(scope="module", auto_use=True)
def failing_scoped_fixture():
    raise RuntimeError("Scoped fixture failed!")

def test_first():
    assert True

def test_second():
    assert True
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_first
            PASS [TIME] test::test_second

    diagnostics:

    error[fixture-failure]: Fixture `failing_scoped_fixture` failed
     --> test.py:5:5
      |
    5 | def failing_scoped_fixture():
      |     ^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Fixture failed here
     --> test.py:6:5
      |
    6 |     raise RuntimeError("Scoped fixture failed!")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Scoped fixture failed!

    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "#);
}

/// Mirrors the cibuildwheel scenario exactly: multiple autouse fixtures in a subdirectory
/// conftest, where one depends on a non-autouse fixture from the parent conftest.
///
/// Before the fix for #633/#635, only the first autouse fixture from each conftest
/// was applied (due to a spurious `break`). The second autouse (`set_second`) would
/// never run, and `fake_pkg` (which depends on a parent fixture) would also be missed.
#[rstest]
fn test_multiple_autouse_fixtures_in_subdirectory_conftest(
    #[values("pytest", "karva")] framework: &str,
) {
    let parent_conftest = format!(
        r#"
import {framework}

@{framework}.fixture
def parent_value():
    return "from_parent"
"#
    );
    let sub_conftest = format!(
        r#"
import {framework}

@{framework}.fixture({auto_use_kw}=True)
def first_autouse(monkeypatch):
    monkeypatch.setenv("FIRST_SET", "yes")

@{framework}.fixture({auto_use_kw}=True)
def second_autouse(monkeypatch, parent_value):
    monkeypatch.setenv("SECOND_SET", parent_value)
"#,
        auto_use_kw = get_auto_use_kw(framework)
    );
    let context = TestContext::with_files([
        ("unit_test/conftest.py", parent_conftest.as_str()),
        ("unit_test/main_tests/conftest.py", sub_conftest.as_str()),
        (
            "unit_test/main_tests/test_something.py",
            "
import os

def test_both_autouse_ran():
    assert os.environ.get('FIRST_SET') == 'yes', os.environ.get('FIRST_SET')
    assert os.environ.get('SECOND_SET') == 'from_parent', os.environ.get('SECOND_SET')
",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] unit_test.main_tests.test_something::test_both_autouse_ran
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

/// All autouse fixtures in a module must be applied, not just the first one.
#[rstest]
fn test_multiple_auto_use_fixtures(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import {framework}

arr = []

@{framework}.fixture({auto_use_kw}=True)
def first_fixture():
    arr.append("first")

@{framework}.fixture({auto_use_kw}=True)
def second_fixture():
    arr.append("second")

def test_both_fixtures_run():
    assert arr == ["first", "second"], arr
"#,
            auto_use_kw = get_auto_use_kw(framework),
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_both_fixtures_run
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}
