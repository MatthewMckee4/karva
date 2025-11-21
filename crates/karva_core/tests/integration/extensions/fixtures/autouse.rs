use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::{TestRunnerExt, get_auto_use_kw};

#[rstest]
fn test_function_scope_auto_use_fixture(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @r"
        test failures:

        test `<test>.test_function_scope_auto_use_fixture::test_something` at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:12 failed at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:13

        test `<test>.test_function_scope_auto_use_fixture::test_something_else` at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:15 failed at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:16

        test failures:
            <test>.test_function_scope_auto_use_fixture::test_something at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:12
            <test>.test_function_scope_auto_use_fixture::test_something_else at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:15

        test result: FAILED. 0 passed; 2 failed; 0 skipped; finished in [TIME]
        ");
    }
}

#[rstest]
fn test_scope_auto_use_fixture(
    #[values("pytest", "karva")] framework: &str,
    #[values("module", "package", "session")] scope: &str,
) {
    let context = TestContext::with_file(
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
    assert arr == [1, 1]

def test_something_else():
    assert arr == [1, 1]
"#,
            auto_use_kw = get_auto_use_kw(framework),
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @r"
        test failures:

        test `<test>.test_function_scope_auto_use_fixture::test_something` at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:12 failed at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:13

        test `<test>.test_function_scope_auto_use_fixture::test_something_else` at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:15 failed at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:16

        test failures:
            <test>.test_function_scope_auto_use_fixture::test_something at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:12
            <test>.test_function_scope_auto_use_fixture::test_something_else at <temp_dir>/<test>/test_function_scope_auto_use_fixture.py:15

        test result: FAILED. 0 passed; 2 failed; 0 skipped; finished in [TIME]
        ");
    }
}

#[rstest]
fn test_auto_use_fixture(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_nested_generator_fixture.py",
        &format!(
            r#"
                from {framework} import fixture

                @fixture
                def first_entry():
                    return "a"

                @fixture
                def order():
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @r"
        test failures:

        test `<test>.test_nested_generator_fixture::test_string_only` at <temp_dir>/<test>/test_nested_generator_fixture.py:16 failed at <temp_dir>/<test>/test_nested_generator_fixture.py:17

        test `<test>.test_nested_generator_fixture::test_string_and_int` at <temp_dir>/<test>/test_nested_generator_fixture.py:19 failed at <temp_dir>/<test>/test_nested_generator_fixture.py:21

        test failures:
            <test>.test_nested_generator_fixture::test_string_only at <temp_dir>/<test>/test_nested_generator_fixture.py:16
            <test>.test_nested_generator_fixture::test_string_and_int at <temp_dir>/<test>/test_nested_generator_fixture.py:19

        test result: FAILED. 0 passed; 2 failed; 0 skipped; finished in [TIME]
        ");
    }
}

#[test]
fn test_auto_use_for_parametrized_fixtures() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r#"
                import karva

                arr = []

                @karva.fixture(auto_use=True)
                def auto_use_fixture():
                    arr.append(1)
                    yield
                    arr.append(2)

                @karva.fixture(params=[1, 2])
                def multi_fixture(request):
                    if request.param == 1:
                        assert arr == [1], f"Expected [1], got {arr}"
                    elif request.param == 2:
                        assert arr == [1, 2, 1], f"Expected [1, 2, 1], got {arr}"
                    return request.param

                def test_multi_fixture(multi_fixture):
                    assert multi_fixture in [1, 2]
                "#,
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    fixture failures:

    fixture function `<test>.test::multi_fixture` at <temp_dir>/<test>/test.py:12 failed at <temp_dir>/<test>/test.py:15
    Expected [1], got []

    fixture function `<test>.test::multi_fixture` at <temp_dir>/<test>/test.py:12 failed at <temp_dir>/<test>/test.py:17
    Expected [1, 2, 1], got []

    test result: FAILED. 0 passed; 2 failed; 0 skipped; finished in [TIME]
    ");
}
