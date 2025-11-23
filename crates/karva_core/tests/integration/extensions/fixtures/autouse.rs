use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::{TestRunnerExt, get_auto_use_kw};

#[rstest]
fn test_function_scope_auto_use_fixture(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test_function_scope_auto_use_fixture.py",
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
        "<test>/test_function_scope_auto_use_fixture.py",
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
        "<test>/test_nested_generator_fixture.py",
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
