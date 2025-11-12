use insta::assert_snapshot;
use karva_test::TestContext;

use crate::common::TestRunnerExt;

#[test]
fn test_fixtures_given_by_decorator() {
    let test_context = TestContext::with_file(
        "<test>/test_fixtures_given_by_decorator.py",
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

    let result = test_context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_fixtures_given_by_decorator_and_fixture() {
    let test_context = TestContext::with_file(
        "<test>/test_fixtures_given_by_decorator_and_fixture.py",
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

    let result = test_context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_fixtures_given_by_decorator_and_parametrize() {
    let test_context = TestContext::with_file(
        "<test>/test_fixtures_given_by_decorator_and_parametrize.py",
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

    let result = test_context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_fixtures_given_by_decorator_and_parametrize_and_fixture() {
    let test_context = TestContext::with_file(
        "<test>/test_fixtures_given_by_decorator_and_parametrize_and_fixture.py",
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

    let result = test_context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_fixtures_given_by_decorator_one_missing() {
    let test_context = TestContext::with_file(
        "<test>/test_fixtures_given_by_decorator_one_missing.py",
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

    let result = test_context.test();

    assert_snapshot!(result.display(), @r#"
    failures:

    test `test_fixtures_given_by_decorator` has missing fixtures: ["b"] at <temp_dir>/<test>/test_fixtures_given_by_decorator_one_missing.py:12

    failures:
        test_fixtures_given_by_decorator

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    "#);
}
