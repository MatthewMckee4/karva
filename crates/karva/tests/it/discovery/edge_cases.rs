use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

/// `__pycache__` directories and compiled `.pyc` files alongside source files
/// should not be picked up as tests.
#[test]
fn test_pyc_files_and_pycache_are_ignored() {
    let context = TestContext::with_files([(
        "test_real.py",
        r"
def test_real(): pass
",
    )]);

    let pycache = context.root().join("__pycache__");
    std::fs::create_dir_all(&pycache).expect("failed to create __pycache__");
    std::fs::write(pycache.join("test_real.cpython-313.pyc"), b"bogus")
        .expect("failed to write .pyc");

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_real::test_real
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

/// A package with `__init__.py` should have its tests discovered under the
/// package path, while standalone sibling files stay at the top level.
#[test]
fn test_package_init_and_standalone_siblings() {
    let context = TestContext::with_files([
        ("pkg/__init__.py", ""),
        (
            "pkg/test_in_pkg.py",
            r"
def test_inside_package(): pass
",
        ),
        (
            "test_standalone.py",
            r"
def test_at_root(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

/// A test directory matching a `.gitignore` rule is skipped by default and
/// restored when `--no-ignore` is passed.
#[test]
fn test_gitignore_excludes_directory() {
    let context = TestContext::with_files([
        (".gitignore", "ignored/\n"),
        (
            "ignored/test_skipped.py",
            r"
def test_skipped(): pass
",
        ),
        (
            "test_kept.py",
            r"
def test_kept(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_kept::test_kept
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_no_ignore_includes_gitignored_directory() {
    let context = TestContext::with_files([
        (".gitignore", "ignored/\n"),
        (
            "ignored/test_skipped.py",
            r"
def test_in_ignored(): pass
",
        ),
        (
            "test_kept.py",
            r"
def test_kept(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel().args(["--no-ignore", "--status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

/// A python file that contains no test functions alongside a file that does
/// should be collected silently.
#[test]
fn test_python_file_without_test_functions_is_ignored() {
    let context = TestContext::with_files([
        (
            "test_helpers.py",
            r"
x = 1
def helper():
    return 42
",
        ),
        (
            "test_real.py",
            r"
def test_one(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_real::test_one
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_generator_tests_are_rejected_without_running_body() {
    let context = TestContext::with_file(
        "test_generators.py",
        r#"
from pathlib import Path
import functools
import karva

def mark(name):
    Path(name).write_text("ran")

def decorator(func):
    @functools.wraps(func)
    def wrapper(*args, **kwargs):
        return func(*args, **kwargs)
    return wrapper

def values():
    yield 1

def test_plain_generator():
    mark("plain.txt")
    yield 1

async def test_async_generator():
    mark("async.txt")
    yield 1

@decorator
def test_decorated_generator():
    mark("decorated.txt")
    yield 1

@karva.tags.parametrize("value", [1])
def test_parametrized_generator(value):
    mark("parametrized.txt")
    yield value

@karva.tags.skip(reason="still invalid")
def test_skipped_generator():
    mark("skipped.txt")
    yield 1

@karva.tags.expect_fail(reason="still invalid")
def test_expected_failure_generator():
    mark("expected_failure.txt")
    yield 1

def test_consumes_generator_internally():
    def nested():
        yield 1

    class Iterable:
        def __iter__(self):
            yield 2

    assert list(values()) == [1]
    assert list(nested()) == [1]
    assert list(Iterable()) == [2]
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 7 tests across 1 worker
            PASS [TIME] test_generators::test_consumes_generator_internally

    diagnostics:

    error[invalid-test]: Generator test `test_plain_generator` is not supported
      --> test_generators.py:18:5
       |
    18 | def test_plain_generator():
       |     ^^^^^^^^^^^^^^^^^^^^
       |
    info: Use `@karva.tags.parametrize` to define multiple test cases.

    error[invalid-test]: Generator test `test_async_generator` is not supported
      --> test_generators.py:22:11
       |
    22 | async def test_async_generator():
       |           ^^^^^^^^^^^^^^^^^^^^
       |
    info: Use `@karva.tags.parametrize` to define multiple test cases.

    error[invalid-test]: Generator test `test_decorated_generator` is not supported
      --> test_generators.py:27:5
       |
    27 | def test_decorated_generator():
       |     ^^^^^^^^^^^^^^^^^^^^^^^^
       |
    info: Use `@karva.tags.parametrize` to define multiple test cases.

    error[invalid-test]: Generator test `test_parametrized_generator` is not supported
      --> test_generators.py:32:5
       |
    32 | def test_parametrized_generator(value):
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
       |
    info: Use `@karva.tags.parametrize` to define multiple test cases.

    error[invalid-test]: Generator test `test_skipped_generator` is not supported
      --> test_generators.py:37:5
       |
    37 | def test_skipped_generator():
       |     ^^^^^^^^^^^^^^^^^^^^^^
       |
    info: Use `@karva.tags.parametrize` to define multiple test cases.

    error[invalid-test]: Generator test `test_expected_failure_generator` is not supported
      --> test_generators.py:42:5
       |
    42 | def test_expected_failure_generator():
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
       |
    info: Use `@karva.tags.parametrize` to define multiple test cases.

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    for marker in [
        "plain.txt",
        "async.txt",
        "decorated.txt",
        "parametrized.txt",
        "skipped.txt",
        "expected_failure.txt",
    ] {
        assert!(!context.root().join(marker).exists());
    }
}

/// An empty subdirectory (no Python files at all) is discovered without error.
#[test]
fn test_empty_subdirectory_is_ignored() {
    let context = TestContext::with_file("test_a.py", "def test_a(): pass");

    std::fs::create_dir_all(context.root().join("empty_dir"))
        .expect("failed to create empty directory");

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_a::test_a
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}
