use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

fn get_parametrize_function(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.parametrize",
        "karva" => "karva.tags.parametrize",
        _ => panic!("Invalid framework"),
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_fixture() {
    let test_context = TestContext::with_file(
        "test.py",
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

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_parametrize_with_fixture(a=1, fixture_value=42) ... ok
    test test::test_parametrize_with_fixture(a=2, fixture_value=42) ... ok
    test test::test_parametrize_with_fixture(a=3, fixture_value=42) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_fixture_parametrize_priority() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"import karva

@karva.fixture
def a():
    return -1

@karva.tags.parametrize("a", [1, 2, 3])
def test_parametrize_with_fixture(a):
    assert a > 0"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_parametrize_with_fixture(a=1) ... ok
    test test::test_parametrize_with_fixture(a=2) ... ok
    test test::test_parametrize_with_fixture(a=3) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_two_decorators() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_function(a=1, b=1) ... ok
    test test::test_function(a=2, b=1) ... ok
    test test::test_function(a=1, b=2) ... ok
    test test::test_function(a=2, b=2) ... ok

    test result: ok. 4 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_three_decorators() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
@karva.tags.parametrize("c", [1, 2])
def test_function(a: int, b: int, c: int):
    assert a > 0 and b > 0 and c > 0
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_function(a=1, b=1, c=1) ... ok
    test test::test_function(a=2, b=1, c=1) ... ok
    test test::test_function(a=1, b=2, c=1) ... ok
    test test::test_function(a=2, b=2, c=1) ... ok
    test test::test_function(a=1, b=1, c=2) ... ok
    test test::test_function(a=2, b=1, c=2) ... ok
    test test::test_function(a=1, b=2, c=2) ... ok
    test test::test_function(a=2, b=2, c=2) ... ok

    test result: ok. 8 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_multiple_args_single_string(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_square(expected=4, input=2) ... ok
        test test::test_square(expected=9, input=3) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_pytest_param_single_arg() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize("a", [
    pytest.param(1),
    pytest.param(2),
    pytest.param(3),
])
def test_single_arg(a):
    assert a > 0
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_single_arg(a=1) ... ok
    test test::test_single_arg(a=2) ... ok
    test test::test_single_arg(a=3) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_pytest_param_multiple_args() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize("input,expected", [
    pytest.param(2, 4),
    pytest.param(3, 9),
    pytest.param(4, 16),
])
def test_square(input, expected):
    assert input ** 2 == expected
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_square(expected=4, input=2) ... ok
    test test::test_square(expected=9, input=3) ... ok
    test test::test_square(expected=16, input=4) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_pytest_param_list_args() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize(["input", "expected"], [
    pytest.param(2, 4),
    pytest.param(3, 9),
    pytest.param(4, 16),
])
def test_square(input, expected):
    assert input ** 2 == expected
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_square(expected=4, input=2) ... ok
    test test::test_square(expected=9, input=3) ... ok
    test test::test_square(expected=16, input=4) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_mixed_pytest_param_and_tuples() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize("input,expected", [
    pytest.param(2, 4),
    (3, 9),
    pytest.param(4, 16),
])
def test_square(input, expected):
    assert input ** 2 == expected
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_square(expected=4, input=2) ... ok
    test test::test_square(expected=9, input=3) ... ok
    test test::test_square(expected=16, input=4) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_list_inside_param() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize(
    "length,nums",
    [
        pytest.param(1, [1]),
        pytest.param(2, [1, 2]),
        pytest.param(None, []),
    ],
)
def test_markup_mode_bullets_single_newline(length: int | None, nums: list[int]):
    if length is not None:
        assert len(nums) == length
    else:
        assert len(nums) == 0
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_markup_mode_bullets_single_newline(length=1, nums=[1]) ... ok
    test test::test_markup_mode_bullets_single_newline(length=2, nums=[1, 2]) ... ok
    test test::test_markup_mode_bullets_single_newline(length=None, nums=[]) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_pytest_param_and_skip() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize("input,expected", [
    pytest.param(2, 4),
    pytest.param(4, 17, marks=pytest.mark.skip),
    pytest.param(5, 26, marks=pytest.mark.xfail),
])
def test_square(input, expected):
    assert input ** 2 == expected
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_square(expected=4, input=2) ... ok
    test test::test_square ... skipped
    test test::test_square(expected=26, input=5) ... ok

    test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_karva_param_single_arg() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.parametrize("a", [
    karva.param(1),
    karva.param(2),
    karva.param(3),
])
def test_single_arg(a):
    assert a > 0
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_single_arg(a=1) ... ok
    test test::test_single_arg(a=2) ... ok
    test test::test_single_arg(a=3) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_karva_param_multiple_args() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.parametrize("input,expected", [
    karva.param(2, 4),
    karva.param(3, 9),
    karva.param(4, 16),
])
def test_square(input, expected):
    assert input ** 2 == expected
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_square(expected=4, input=2) ... ok
    test test::test_square(expected=9, input=3) ... ok
    test test::test_square(expected=16, input=4) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_karva_param_list_args() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.parametrize(["input", "expected"], [
    karva.param(2, 4),
    karva.param(3, 9),
    karva.param(4, 16),
])
def test_square(input, expected):
    assert input ** 2 == expected
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_square(expected=4, input=2) ... ok
    test test::test_square(expected=9, input=3) ... ok
    test test::test_square(expected=16, input=4) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_mixed_karva_param_and_tuples() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.parametrize("input,expected", [
    karva.param(2, 4),
    (3, 9),
    karva.param(4, 16),
])
def test_square(input, expected):
    assert input ** 2 == expected
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_square(expected=4, input=2) ... ok
    test test::test_square(expected=9, input=3) ... ok
    test test::test_square(expected=16, input=4) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_karva_list_inside_param() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.parametrize(
    "length,nums",
    [
        karva.param(1, [1]),
        karva.param(2, [1, 2]),
        karva.param(None, []),
    ],
)
def test_markup_mode_bullets_single_newline(length: int | None, nums: list[int]):
    if length is not None:
        assert len(nums) == length
    else:
        assert len(nums) == 0
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_markup_mode_bullets_single_newline(length=1, nums=[1]) ... ok
    test test::test_markup_mode_bullets_single_newline(length=2, nums=[1, 2]) ... ok
    test test::test_markup_mode_bullets_single_newline(length=None, nums=[]) ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize_with_karva_param_and_skip() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.parametrize("input,expected", [
    karva.param(2, 4),
    karva.param(4, 17, tags=(karva.tags.skip,)),
    karva.param(5, 26, tags=(karva.tags.expect_fail,)),
    karva.param(6, 36, tags=(karva.tags.skip(True),)),
    karva.param(7, 50, tags=(karva.tags.expect_fail(True),)),
])
def test_square(input, expected):
    assert input ** 2 == expected
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_square(expected=4, input=2) ... ok
    test test::test_square ... skipped
    test test::test_square(expected=26, input=5) ... ok
    test test::test_square ... skipped
    test test::test_square(expected=50, input=7) ... ok

    test result: ok. 3 passed; 0 failed; 2 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
