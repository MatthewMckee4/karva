use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::{TestRunnerExt, get_auto_use_kw};

#[rstest]
fn test_function_scope_auto_use_fixture(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test.py",
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

    let result = test_context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_scope_auto_use_fixture(
    #[values("pytest", "karva")] framework: &str,
    #[values("module", "package", "session")] scope: &str,
) {
    let test_context = TestContext::with_file(
        "<test>/test.py",
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

    let result = test_context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_auto_use_fixture(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test.py",
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

    let result = test_context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}
#[test]
fn test_auto_use_fixture_in_parent_module() {
    let test_context = TestContext::with_files([
        (
            "<test>/conftest.py",
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
            "<test>/foo/test_file2.py",
            "
            from ..conftest import arr

            def test_function1():
                assert arr == [1]

            def test_function2():
                assert arr == [1, 2, 1]
            ",
        ),
    ]);

    let result = test_context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}
