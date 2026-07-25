use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_fixtures_given_by_decorator() {
    let test_context = TestContext::with_file(
        "test.py",
        r"
import functools

def given(**kwargs):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **wrapper_kwargs):
            return func(*args, **kwargs, **wrapper_kwargs)
        return wrapper
    return decorator

@given(a=1)
def test_fixtures_given_by_decorator(a):
    assert a == 1
",
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_fixtures_given_by_decorator
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixtures_given_by_decorator_and_fixture() {
    let test_context = TestContext::with_file(
        "test.py",
        r"
import karva

def given(**kwargs):
    import functools
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **wrapper_kwargs):
            return func(*args, **kwargs, **wrapper_kwargs)
        return wrapper
    return decorator

@karva.fixture
def b():
    return 1

@given(a=1)
def test_func(a, b):
    assert a == 1
    assert b == 1
",
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_func(b=1)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixtures_given_by_decorator_and_parametrize() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva
import functools

def given(**kwargs):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **wrapper_kwargs):
            return func(*args, **kwargs, **wrapper_kwargs)
        return wrapper
    return decorator

@given(a=1)
@karva.tags.parametrize("b", [1, 2])
def test_func(a, b):
    assert a == 1
    assert b in [1, 2]
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_func(b=1)
            PASS [TIME] test::test_func(b=2)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixtures_given_by_decorator_and_parametrize_and_fixture() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"
import karva
import functools

def given(**kwargs):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **wrapper_kwargs):
            return func(*args, **kwargs, **wrapper_kwargs)
        return wrapper
    return decorator

@karva.fixture
def c():
    return 1

@given(a=1)
@karva.tags.parametrize("b", [1, 2])
def test_func(a, b, c):
    assert a == 1
    assert b in [1, 2]
    assert c == 1
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_func(b=1, c=1)
            PASS [TIME] test::test_func(b=2, c=1)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixtures_given_by_decorator_one_missing() {
    let test_context = TestContext::with_file(
        "test.py",
        r"
import functools

def given(**kwargs):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **wrapper_kwargs):
            return func(*args, **kwargs, **wrapper_kwargs)
        return wrapper
    return decorator

@given(a=1)
def test_fixtures_given_by_decorator(a, b):
    assert a == 1
    assert b == 1
",
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_fixtures_given_by_decorator

    failures:

    test::test_fixtures_given_by_decorator:

    error[missing-fixtures]: Test `test_fixtures_given_by_decorator` has missing fixtures
      --> test.py:13:5
       |
    13 | def test_fixtures_given_by_decorator(a, b):
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
       |
    info: Missing fixtures: `b`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}
