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
#[ignore = "Will fail unless `maturin build` is ran"]
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
        assert_cmd_snapshot!(context.command(), @r"
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
        assert_cmd_snapshot!(context.command(), @r"
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
        assert_cmd_snapshot!(context.command(), @r"
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
#[ignore = "Will fail unless `maturin build` is ran"]
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

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test foo.inner.test_file2::test_function1 ... ok
    test foo.inner.test_file2::test_function2 ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
