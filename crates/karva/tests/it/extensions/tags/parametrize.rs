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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_parametrize_with_fixture(a=1, fixture_value=42)
            PASS [TIME] test::test_parametrize_with_fixture(a=2, fixture_value=42)
            PASS [TIME] test::test_parametrize_with_fixture(a=3, fixture_value=42)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_parametrize_with_fixture(a=1)
            PASS [TIME] test::test_parametrize_with_fixture(a=2)
            PASS [TIME] test::test_parametrize_with_fixture(a=3)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_function(a=1, b=1)
            PASS [TIME] test::test_function(a=2, b=1)
            PASS [TIME] test::test_function(a=1, b=2)
            PASS [TIME] test::test_function(a=2, b=2)
    ────────────
         Summary [TIME] 4 tests run: 4 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_function(a=1, b=1, c=1)
            PASS [TIME] test::test_function(a=2, b=1, c=1)
            PASS [TIME] test::test_function(a=1, b=2, c=1)
            PASS [TIME] test::test_function(a=2, b=2, c=1)
            PASS [TIME] test::test_function(a=1, b=1, c=2)
            PASS [TIME] test::test_function(a=2, b=1, c=2)
            PASS [TIME] test::test_function(a=1, b=2, c=2)
            PASS [TIME] test::test_function(a=2, b=2, c=2)
    ────────────
         Summary [TIME] 8 tests run: 8 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
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
        assert_cmd_snapshot!(test_context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_square(expected=4, input=2)
                PASS [TIME] test::test_square(expected=9, input=3)
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_single_arg(a=1)
            PASS [TIME] test::test_single_arg(a=2)
            PASS [TIME] test::test_single_arg(a=3)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_square(expected=4, input=2)
            PASS [TIME] test::test_square(expected=9, input=3)
            PASS [TIME] test::test_square(expected=16, input=4)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_square(expected=4, input=2)
            PASS [TIME] test::test_square(expected=9, input=3)
            PASS [TIME] test::test_square(expected=16, input=4)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_square(expected=4, input=2)
            PASS [TIME] test::test_square(expected=9, input=3)
            PASS [TIME] test::test_square(expected=16, input=4)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_markup_mode_bullets_single_newline(length=1, nums=[1])
            PASS [TIME] test::test_markup_mode_bullets_single_newline(length=2, nums=[1, 2])
            PASS [TIME] test::test_markup_mode_bullets_single_newline(length=None, nums=[])
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_square(expected=4, input=2)
            PASS [TIME] test::test_square(expected=26, input=5)
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_single_arg(a=1)
            PASS [TIME] test::test_single_arg(a=2)
            PASS [TIME] test::test_single_arg(a=3)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_square(expected=4, input=2)
            PASS [TIME] test::test_square(expected=9, input=3)
            PASS [TIME] test::test_square(expected=16, input=4)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_square(expected=4, input=2)
            PASS [TIME] test::test_square(expected=9, input=3)
            PASS [TIME] test::test_square(expected=16, input=4)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_square(expected=4, input=2)
            PASS [TIME] test::test_square(expected=9, input=3)
            PASS [TIME] test::test_square(expected=16, input=4)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_markup_mode_bullets_single_newline(length=1, nums=[1])
            PASS [TIME] test::test_markup_mode_bullets_single_newline(length=2, nums=[1, 2])
            PASS [TIME] test::test_markup_mode_bullets_single_newline(length=None, nums=[])
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_square(expected=4, input=2)
            PASS [TIME] test::test_square(expected=26, input=5)
            PASS [TIME] test::test_square(expected=50, input=7)
    ────────────
         Summary [TIME] 5 tests run: 3 passed, 2 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_parametrize_with_karva_param_invalid_tag_reports_value() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.parametrize("value", [
    karva.param(1, tags=[123]),
])
def test_value(value):
    assert value == 1
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: Invalid tag `123` of type `int`; expected a karva tag or callable returning one

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_parametrize_with_pytest_param_marks_list() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize("x", [
    pytest.param(1),
    pytest.param(2, marks=[pytest.mark.skip]),
    pytest.param(3, marks=[pytest.mark.xfail]),
])
def test_marks_list(x):
    assert x != 3
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_marks_list(x=1)
            PASS [TIME] test::test_marks_list(x=3)
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_parametrize_with_pytest_param_marks_skip_reason() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize("x", [
    pytest.param(1),
    pytest.param(2, marks=pytest.mark.skip(reason="not ready")),
])
def test_with_skip_reason(x):
    assert x > 0
"#,
    );

    assert_cmd_snapshot!(test_context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_with_skip_reason(x=1)
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_parametrize_with_pytest_param_skipif_string_condition_uses_module_globals() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

SHOULD_SKIP = False

@pytest.mark.parametrize("x", [
    pytest.param(1),
    pytest.param(2, marks=pytest.mark.skipif("SHOULD_SKIP", reason="module global")),
])
def test_with_skipif(x):
    assert x > 0
"#,
    );

    assert_cmd_snapshot!(test_context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_with_skipif(x=1)
            PASS [TIME] test::test_with_skipif(x=2)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_parametrize_kwargs() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize(["input", "expected"], argvalues=[
    pytest.param(2, 4),
    pytest.param(4, 16),
])
def test1(input, expected):
    assert input ** 2 == expected

@pytest.mark.parametrize(argnames=["input", "expected"], argvalues=[
    pytest.param(2, 4),
    pytest.param(4, 16),
])
def test2(input, expected):
    assert input ** 2 == expected
    "#,
    );

    assert_cmd_snapshot!(test_context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test1(expected=4, input=2)
            PASS [TIME] test::test1(expected=16, input=4)
            PASS [TIME] test::test2(expected=4, input=2)
            PASS [TIME] test::test2(expected=16, input=4)
    ────────────
         Summary [TIME] 4 tests run: 4 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_parametrize_invalid_arg_names() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.parametrize(123, [1, 2])
def test_invalid(x):
    assert True
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: Expected a string or a list of strings for the arg_names, and a list of lists of objects for the arg_values

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_parametrize_invalid_arg_names() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.parametrize(123, [1, 2])
def test_invalid(x):
    assert True
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: pytest parametrize mark argnames must be a string or list of strings

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_parametrize_missing_argvalues() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize("x")
def test_invalid(x):
    assert True
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: pytest parametrize mark requires argvalues

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_param_invalid_mark_is_rejected() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.parametrize('x', [
    pytest.param(1, marks=pytest.mark.timeout(0)),
])
def test_invalid(x):
    assert True
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: pytest timeout mark seconds must be a finite, positive number

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}
