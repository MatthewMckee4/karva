use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::{TestRunnerExt, get_parametrize_function};

#[test]
fn test_parametrize_with_fixture() {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        r#"
import karva

@karva.fixture
def fixture_value():
    return 42

@karva.tags.parametrize("a", [1, 2, 3])
def test_parametrize_with_fixture(a, fixture_value):
    assert a > 0
    assert fixture_value == 42"#,
    );

    let result = test_context.test();

    assert_snapshot!(result.display(), @"test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_parametrize_with_fixture_parametrize_priority() {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        r#"import karva

@karva.fixture
def a():
    return -1

@karva.tags.parametrize("a", [1, 2, 3])
def test_parametrize_with_fixture(a):
    assert a > 0"#,
    );

    let result = test_context.test();

    assert_snapshot!(result.display(), @"test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_parametrize_two_decorators() {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        r#"import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
"#,
    );

    let result = test_context.test();

    assert_snapshot!(result.display(), @"test result: ok. 4 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_parametrize_three_decorators() {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        r#"
import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
@karva.tags.parametrize("c", [1, 2])
def test_function(a: int, b: int, c: int):
    assert a > 0 and b > 0 and c > 0
"#,
    );

    let result = test_context.test();

    assert_snapshot!(result.display(), @"test result: ok. 8 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[rstest]
fn test_parametrize_multiple_args_single_string(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test.py",
        &format!(
            r#"
                import {}

                @{}("input,expected", [
                    (2, 4),
                    (3, 9),
                ])
                def test_square(input, expected):
                    assert input ** 2 == expected
                "#,
            framework,
            get_parametrize_function(framework)
        ),
    );

    let result = test_context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}
