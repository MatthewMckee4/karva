use karva_core::TestResultStats;
use karva_test::TestContext;

use crate::TestRunnerExt;

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

    let mut expected_stats = TestResultStats::default();

    expected_stats.add_passed();

    assert_eq!(*result.stats(), expected_stats);
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

    let mut expected_stats = TestResultStats::default();

    expected_stats.add_passed();

    assert_eq!(*result.stats(), expected_stats);
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

    let mut expected_stats = TestResultStats::default();

    for _ in 0..2 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats);
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

    let mut expected_stats = TestResultStats::default();

    for _ in 0..2 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats);
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

    let mut expected_stats = TestResultStats::default();

    expected_stats.add_failed();

    assert!(!result.passed());

    assert_eq!(*result.stats(), expected_stats);
}
