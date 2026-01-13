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
        assert_cmd_snapshot!(context.command_no_parallel(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_something ... ok
        test test::test_something_else ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

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
        assert_cmd_snapshot!(context.command_no_parallel(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_something ... ok
        test test::test_something_else ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

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
        assert_cmd_snapshot!(context.command_no_parallel(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_string_only(first_entry=a, order=['a']) ... ok
        test test::test_string_and_int(first_entry=a, order=['a']) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

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
                assert arr == [1]

            def test_function2():
                assert arr == [1, 2, 1]
            ",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test foo.inner.test_file2::test_function1 ... ok
    test foo.inner.test_file2::test_function2 ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_something ... ok
    test test::test_something_else ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_something ... ok

    diagnostics:

    warning[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `failing_teardown_fixture`
     --> test.py:5:5
      |
    4 | @karva.fixture(auto_use=True)
    5 | def failing_teardown_fixture():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^
    6 |     yield
    7 |     raise RuntimeError("Teardown failed!")
      |
    info: Failed to reset fixture: Teardown failed!

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
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

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_something ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_first ... ok
    test test::test_second ... ok

    diagnostics:

    error[fixture-failure]: Fixture `failing_scoped_fixture` failed
     --> test.py:5:5
      |
    4 | @karva.fixture(scope="module", auto_use=True)
    5 | def failing_scoped_fixture():
      |     ^^^^^^^^^^^^^^^^^^^^^^
    6 |     raise RuntimeError("Scoped fixture failed!")
      |
    info: Fixture failed here
     --> test.py:6:5
      |
    4 | @karva.fixture(scope="module", auto_use=True)
    5 | def failing_scoped_fixture():
    6 |     raise RuntimeError("Scoped fixture failed!")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    7 |
    8 | def test_first():
      |
    info: Scoped fixture failed!

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}
