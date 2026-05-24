use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_single_file() {
    let context = TestContext::with_files([
        (
            "test_file1.py",
            r"
def test_1(): pass
def test_2(): pass",
        ),
        (
            "test_file2.py",
            r"
def test_3(): pass
def test_4(): pass",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel().arg("test_file1.py"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_file1::test_1
            PASS [TIME] test_file1::test_2
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_empty_file() {
    let context = TestContext::with_file("test.py", "");

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped
    error: no tests to run
    (hint: use `--no-tests` to customize)

    ----- stderr -----
    ");
}

#[test]
fn test_empty_directory() {
    let context = TestContext::new();

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped
    error: no tests to run
    (hint: use `--no-tests` to customize)

    ----- stderr -----
    ");
}

#[test]
fn test_single_function() {
    let context = TestContext::with_file(
        "test.py",
        r"
            def test_1(): pass
            def test_2(): pass",
    );

    assert_cmd_snapshot!(context.command().arg("test.py::test_1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_single_function_shadowed_by_file() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
def test_2(): pass",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["test.py::test_1", "test.py"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_1
            PASS [TIME] test::test_2
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_single_function_shadowed_by_directory() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
def test_2(): pass",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["test.py::test_1", "."]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_1
            PASS [TIME] test::test_2
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_no_tests_found() {
    let context = TestContext::with_file("test_no_tests.py", r"");

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped
    error: no tests to run
    (hint: use `--no-tests` to customize)

    ----- stderr -----
    ");
}

#[test]
fn test_one_test_passes() {
    let context = TestContext::with_file(
        "test_pass.py",
        r"
        def test_pass():
            assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_pass::test_pass
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_one_test_fail() {
    let context = TestContext::with_file(
        "test_fail.py",
        r"
        def test_fail():
            assert False
    ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_fail::test_fail

    diagnostics:

    error[test-failure]: Test `test_fail` failed
     --> test_fail.py:2:5
      |
    2 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> test_fail.py:3:5
      |
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fail_concise_output() {
    let context = TestContext::with_file(
        "test_fail.py",
        r"
        import karva

        @karva.fixture
        def fixture_1():
            yield 1
            raise ValueError('Teardown error')

        def test_1(fixture_1):
            assert fixture == 2

        @karva.fixture
        def fixture_2():
            raise ValueError('fixture error')

        def test_2(fixture_2):
            assert False

        def test_3():
            assert False
    ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--output-format").arg("concise"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
            FAIL [TIME] test_fail::test_1(fixture_1=1)
            FAIL [TIME] test_fail::test_2
            FAIL [TIME] test_fail::test_3

    diagnostics:

    test_fail.py:5:5: error[invalid-fixture-finalizer] Discovered an invalid fixture finalizer `fixture_1`
    test_fail.py:9:5: error[test-failure] Test `test_1` failed
    test_fail.py:16:5: error[missing-fixtures] Test `test_2` has missing fixtures: `fixture_2`
    test_fail.py:19:5: error[test-failure] Test `test_3` failed
    ────────────
         Summary [TIME] 3 tests run: 0 passed, 3 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_two_test_fails() {
    let context = TestContext::with_file(
        "tests/test_fail.py",
        r"
        def test_fail():
            assert False

        def test_fail2():
            assert False, 'Test failed'
    ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] tests.test_fail::test_fail
            FAIL [TIME] tests.test_fail::test_fail2

    diagnostics:

    error[test-failure]: Test `test_fail` failed
     --> tests/test_fail.py:2:5
      |
    2 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> tests/test_fail.py:3:5
      |
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    error[test-failure]: Test `test_fail2` failed
     --> tests/test_fail.py:5:5
      |
    5 | def test_fail2():
      |     ^^^^^^^^^^
      |
    info: Test failed here
     --> tests/test_fail.py:6:5
      |
    6 |     assert False, 'Test failed'
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_file_importing_another_file() {
    let context = TestContext::with_files([
        (
            "helper.py",
            r"
            def validate_data(data):
                if not data:
                    assert False, 'Data validation failed'
                return True
        ",
        ),
        (
            "test_cross_file.py",
            r"
            from helper import validate_data

            def test_with_helper():
                validate_data([])
        ",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_cross_file::test_with_helper

    diagnostics:

    error[test-failure]: Test `test_with_helper` failed
     --> test_cross_file.py:4:5
      |
    4 | def test_with_helper():
      |     ^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> helper.py:4:9
      |
    4 |         assert False, 'Data validation failed'
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Data validation failed

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_stdout() {
    let context = TestContext::with_file(
        "test_std_out_redirected.py",
        r"
        def test_std_out_redirected():
            print('Hello, world!')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-s"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    Hello, world!
            PASS [TIME] test_std_out_redirected::test_std_out_redirected
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.command().arg("-s"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    Hello, world!
            PASS [TIME] test_std_out_redirected::test_std_out_redirected
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_std_out_redirected::test_std_out_redirected
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_multiple_fixtures_not_found() {
    let context = TestContext::with_file(
        "test_multiple_fixtures_not_found.py",
        "def test_multiple_fixtures_not_found(a, b, c): ...",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_multiple_fixtures_not_found::test_multiple_fixtures_not_found

    diagnostics:

    error[missing-fixtures]: Test `test_multiple_fixtures_not_found` has missing fixtures
     --> test_multiple_fixtures_not_found.py:1:5
      |
    1 | def test_multiple_fixtures_not_found(a, b, c): ...
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `a`, `b`, `c`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_text_file_in_directory() {
    let context = TestContext::with_files([
        ("test_sample.py", "def test_sample(): assert True"),
        ("random.txt", "pass"),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_sample::test_sample
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_text_file() {
    let context = TestContext::with_file("random.txt", "pass");

    assert_cmd_snapshot!(
        context.command().args(["random.txt"]),
        @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: path `<temp_dir>/random.txt` has a wrong file extension
    ");
}

#[test]
fn test_quiet_output_passing() {
    let context = TestContext::with_file(
        "test.py",
        "
        def test_quiet_output():
            assert True
        ",
    );

    assert_cmd_snapshot!(context.command().args(["--status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_quiet_output_failing() {
    let context = TestContext::with_file(
        "test.py",
        "
        def test_quiet_output():
            assert False
        ",
    );

    assert_cmd_snapshot!(context.command().args(["--status-level=none"]), @"
    success: false
    exit_code: 1
    ----- stdout -----

    diagnostics:

    error[test-failure]: Test `test_quiet_output` failed
     --> test.py:2:5
      |
    2 | def test_quiet_output():
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:3:5
      |
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_invalid_path() {
    let context = TestContext::new();

    assert_cmd_snapshot!(context.command().arg("non_existing_path.py"), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: path `<temp_dir>/non_existing_path.py` could not be found
    ");
}

#[test]
fn test_fixture_generator_two_yields_passing_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
            import karva

            @karva.fixture
            def fixture_generator():
                yield 1
                yield 2

            def test_fixture_generator(fixture_generator):
                assert fixture_generator == 1
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_fixture_generator(fixture_generator=1)

    diagnostics:

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `fixture_generator`
     --> test.py:5:5
      |
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Fixture had more than one yield statement

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_generator_two_yields_failing_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
            import karva

            @karva.fixture
            def fixture_generator():
                yield 1
                yield 2

            def test_fixture_generator(fixture_generator):
                assert fixture_generator == 2
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_fixture_generator(fixture_generator=1)

    diagnostics:

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `fixture_generator`
     --> test.py:5:5
      |
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Fixture had more than one yield statement

    error[test-failure]: Test `test_fixture_generator` failed
     --> test.py:9:5
      |
    9 | def test_fixture_generator(fixture_generator):
      |     ^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Test ran with arguments:
    info: `fixture_generator`: `1`
    info: Test failed here
      --> test.py:10:5
       |
    10 |     assert fixture_generator == 2
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
       |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_generator_fail_in_teardown() {
    let context = TestContext::with_file(
        "test.py",
        r#"
        import karva

        @karva.fixture
        def fixture_generator():
            yield 1
            raise ValueError("fixture error")

        def test_fixture_generator(fixture_generator):
            assert fixture_generator == 1
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_fixture_generator(fixture_generator=1)

    diagnostics:

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `fixture_generator`
     --> test.py:5:5
      |
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Failed to reset fixture: fixture error

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_invalid_fixture() {
    let context = TestContext::with_file(
        "test.py",
        r#"
        import karva

        @karva.fixture(scope='ssession')
        def fixture_generator():
            raise ValueError("fixture-error")

        def test_fixture_generator(fixture_generator):
            assert fixture_generator == 1
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_fixture_generator

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `fixture_generator`
     --> test.py:5:5
      |
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Invalid fixture scope: ssession

    error[missing-fixtures]: Test `test_fixture_generator` has missing fixtures
     --> test.py:8:5
      |
    8 | def test_fixture_generator(fixture_generator):
      |     ^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `fixture_generator`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_failfast() {
    let context = TestContext::with_file(
        "test_failfast.py",
        r"
        def test_first_fail():
            assert False, 'First test fails'

        def test_second():
            assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["--fail-fast"]), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test_failfast::test_first_fail

    diagnostics:

    error[test-failure]: Test `test_first_fail` failed
     --> test_failfast.py:2:5
      |
    2 | def test_first_fail():
      |     ^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test_failfast.py:3:5
      |
    3 |     assert False, 'First test fails'
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: First test fails

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_failfast_multiple_threads() {
    let context = TestContext::with_file(
        "test_a.py",
        r"
import time

def test_fail():
    assert False

def test_1():
    time.sleep(0.5)
    assert True

def test_2():
    time.sleep(0.5)
    assert True

def test_3():
    time.sleep(0.5)
    assert True

def test_4():
    time.sleep(0.5)
    assert True

def test_5():
    time.sleep(0.5)
    assert True

def test_6():
    time.sleep(0.5)
    assert True

def test_7():
    time.sleep(0.5)
    assert True

def test_8():
    time.sleep(0.5)
    assert True

def test_9():
    time.sleep(0.5)
    assert True
    ",
    );

    assert_cmd_snapshot!(context.command().arg("--fail-fast").arg("--num-workers").arg("2").arg("-v"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 10 tests across 2 workers
            FAIL [TIME] test_a::test_fail

    diagnostics:

    error[test-failure]: Test `test_fail` failed
     --> test_a.py:4:5
      |
    4 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> test_a.py:5:5
      |
    5 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    INFO Collected all tests in [TIME]
    INFO Spawning 2 workers
    INFO Worker 0 spawned with 5 tests
    INFO Worker 1 spawned with 5 tests
    INFO Waiting for 2 workers to complete (Ctrl+C to cancel)
    INFO Fail-fast signal received — stopping remaining workers
    ");
}

#[test]
fn test_test_prefix() {
    let context = TestContext::with_file(
        "test_fail.py",
        r"
import karva

def test_1(): ...
def tests_1(): ...

        ",
    );

    assert_cmd_snapshot!(context.command().arg("--test-prefix").arg("tests_"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_fail::tests_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_unused_files_are_imported() {
    let context = TestContext::with_file(
        "test_fail.py",
        r"
def test_1():
    assert True

        ",
    );

    context.write_file("foo.py", "print('hello world')");

    assert_cmd_snapshot!(context.command().arg("-s"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_fail::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_unused_files_that_fail_are_not_imported() {
    let context = TestContext::with_file(
        "test_fail.py",
        r"
def test_1():
    assert True

        ",
    );

    context.write_file(
        "foo.py",
        "
    import sys
    sys.exit(1)",
    );

    assert_cmd_snapshot!(context.command().arg("-s"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_fail::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_argument_truncated() {
    let context = TestContext::with_file(
        "test_file.py",
        r"
import karva

@karva.fixture
def fixture_very_very_very_very_very_long_name():
    return 'fixture_very_very_very_very_very_long_name'

def test_1(fixture_very_very_very_very_very_long_name):
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_file::test_1(fixture_very_very_very_very...=fixture_very_very_very_very...)

    diagnostics:

    error[test-failure]: Test `test_1` failed
     --> test_file.py:8:5
      |
    8 | def test_1(fixture_very_very_very_very_very_long_name):
      |     ^^^^^^
      |
    info: Test ran with arguments:
    info: `fixture_very_very_very_very...`: `fixture_very_very_very_very...`
    info: Test failed here
     --> test_file.py:9:5
      |
    9 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_finalizer() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os

def test_setenv(monkeypatch):
    monkeypatch.setenv('TEST_VAR_5', 'test_value_5')
    assert os.environ['TEST_VAR_5'] == 'test_value_5'

def test_1():
    assert 'TEST_VAR_5' not in os.environ
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-s"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_setenv(monkeypatch=<MockEnv object>)
            PASS [TIME] test::test_1
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_no_progress() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_try_import_fixtures() {
    let context = TestContext::with_files([
        (
            "foo.py",
            r"
import karva

@karva.fixture
def x():
    return 1

@karva.fixture()
def y():
    return 1
                ",
        ),
        (
            "test_file.py",
            "
from foo import x, y
def test_1(x): pass
def test_2(y): pass
                ",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel().arg("--try-import-fixtures"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_file::test_1(x=1)
            PASS [TIME] test_file::test_2(y=1)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_try_import_fixtures_invalid_fixtures() {
    let context = TestContext::with_files([
        (
            "foo.py",
            r"
import karva

@karva.fixture
def x():
    raise ValueError('Invalid fixture')

@karva.fixture()
def y():
    return 1
                ",
        ),
        (
            "test_file.py",
            "
from foo import x, y
def test_1(x): pass
def test_2(y): pass
                ",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel().arg("--try-import-fixtures"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test_file::test_1
            PASS [TIME] test_file::test_2(y=1)

    diagnostics:

    error[missing-fixtures]: Test `test_1` has missing fixtures
     --> test_file.py:3:5
      |
    3 | def test_1(x): pass
      |     ^^^^^^
      |
    info: Missing fixtures: `x`
    info: Fixture `x` failed here
     --> foo.py:6:5
      |
    6 |     raise ValueError('Invalid fixture')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Invalid fixture

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_retry() {
    let context = TestContext::with_file(
        "test.py",
        r"
a = 3

def test_1():
    global a
    if a == 0:
        assert True
    else:
        a -= 1
        assert False
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry").arg("5"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_1
      TRY 2 FAIL [TIME] test::test_1
      TRY 3 FAIL [TIME] test::test_1
      TRY 4 PASS [TIME] test::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 4/6 [TIME] test::test_1

    ----- stderr -----
    ");
}

#[test]
fn test_parallel_worker_capping() {
    let context = TestContext::with_file(
        "test_a.py",
        r"
def test_1(): pass
def test_2(): pass
def test_3(): pass",
    );

    // With 3 tests and 8 requested workers, worker capping reduces to 1 worker
    // (ceil(3/5) = 1). The -v flag shows info logs confirming "Spawning 1 workers".
    assert_cmd_snapshot!(context.command().args(["-v", "--num-workers", "8"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test_a::test_1
            PASS [TIME] test_a::test_2
            PASS [TIME] test_a::test_3
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    INFO Collected all tests in [TIME]
    INFO Capped worker count to avoid underutilized workers total_tests=3 requested_workers=8 capped_workers=1
    INFO Spawning 1 workers
    INFO Worker 0 spawned with 3 tests
    INFO Waiting for 1 workers to complete (Ctrl+C to cancel)
    INFO Worker 0 completed successfully in [TIME]
    INFO All workers completed
    ");
}

#[test]
fn test_concise_output_format_with_failure() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_pass():
    assert True

def test_fail():
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--output-format=concise"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_pass
            FAIL [TIME] test::test_fail

    diagnostics:

    test.py:5:5: error[test-failure] Test `test_fail` failed
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_concise_output_format_with_discovery_error() {
    let context = TestContext::with_file(
        "test.py",
        r"
import nonexistent_module_xyz

def test_pass():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--output-format=concise"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module] Failed to import python module `test`: No module named 'nonexistent_module_xyz'
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_collection_error_with_passing_tests_exits_nonzero() {
    let context = TestContext::with_files([
        (
            "test_bad.py",
            r"
import nonexistent_module_xyz

def test_unreachable():
    assert True
            ",
        ),
        (
            "test_good.py",
            r"
def test_pass():
    assert True
            ",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_good::test_pass

    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test_bad`: No module named 'nonexistent_module_xyz'

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

/// `--max-fail=2` should run exactly two failing tests and then stop scheduling
/// the rest. The summary reflects only the tests that actually ran.
#[test]
fn test_max_fail_stops_after_n_failures() {
    let context = TestContext::with_file(
        "test_max_fail.py",
        r"
def test_first_fail():
    assert False, 'boom 1'

def test_second_fail():
    assert False, 'boom 2'

def test_third_fail():
    assert False, 'boom 3'

def test_fourth_skipped():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--max-fail=2"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 4 tests across 1 worker
            FAIL [TIME] test_max_fail::test_first_fail
            FAIL [TIME] test_max_fail::test_second_fail

    diagnostics:

    error[test-failure]: Test `test_first_fail` failed
     --> test_max_fail.py:2:5
      |
    2 | def test_first_fail():
      |     ^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test_max_fail.py:3:5
      |
    3 |     assert False, 'boom 1'
      |     ^^^^^^^^^^^^^^^^^^^^^^
      |
    info: boom 1

    error[test-failure]: Test `test_second_fail` failed
     --> test_max_fail.py:5:5
      |
    5 | def test_second_fail():
      |     ^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test_max_fail.py:6:5
      |
    6 |     assert False, 'boom 2'
      |     ^^^^^^^^^^^^^^^^^^^^^^
      |
    info: boom 2

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");
}

/// `--no-fail-fast` disables the limit, so every test runs even when some fail.
#[test]
fn test_no_fail_fast_runs_every_test() {
    let context = TestContext::with_file(
        "test_no_fail_fast.py",
        r"
def test_a():
    assert False, 'a boom'

def test_b():
    assert False, 'b boom'

def test_c():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--no-fail-fast"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
            FAIL [TIME] test_no_fail_fast::test_a
            FAIL [TIME] test_no_fail_fast::test_b
            PASS [TIME] test_no_fail_fast::test_c

    diagnostics:

    error[test-failure]: Test `test_a` failed
     --> test_no_fail_fast.py:2:5
      |
    2 | def test_a():
      |     ^^^^^^
      |
    info: Test failed here
     --> test_no_fail_fast.py:3:5
      |
    3 |     assert False, 'a boom'
      |     ^^^^^^^^^^^^^^^^^^^^^^
      |
    info: a boom

    error[test-failure]: Test `test_b` failed
     --> test_no_fail_fast.py:5:5
      |
    5 | def test_b():
      |     ^^^^^^
      |
    info: Test failed here
     --> test_no_fail_fast.py:6:5
      |
    6 |     assert False, 'b boom'
      |     ^^^^^^^^^^^^^^^^^^^^^^
      |
    info: b boom

    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");
}

/// `--max-fail=1` is the generalized form of `--fail-fast` and should stop
/// scheduling once a single test has failed.
#[test]
fn test_max_fail_one_is_equivalent_to_fail_fast() {
    let context = TestContext::with_file(
        "test_max_fail_one.py",
        r"
def test_first():
    assert True

def test_second():
    assert False, 'stop here'

def test_third():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--max-fail=1"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test_max_fail_one::test_first
            FAIL [TIME] test_max_fail_one::test_second

    diagnostics:

    error[test-failure]: Test `test_second` failed
     --> test_max_fail_one.py:5:5
      |
    5 | def test_second():
      |     ^^^^^^^^^^^
      |
    info: Test failed here
     --> test_max_fail_one.py:6:5
      |
    6 |     assert False, 'stop here'
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: stop here

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_show_python_output() {
    let context = TestContext::with_file(
        "test.py",
        r#"
def test_with_print():
    print("hello from test")
    assert True
        "#,
    );

    assert_cmd_snapshot!(context.command().arg("-s"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    hello from test
            PASS [TIME] test::test_with_print
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_no_capture() {
    let context = TestContext::with_file(
        "test.py",
        r#"
def test_with_print():
    print("hello from test")
    assert True
        "#,
    );

    assert_cmd_snapshot!(context.command().arg("--no-capture"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    hello from test
            PASS [TIME] test::test_with_print
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_retry_flag() {
    let context = TestContext::with_file(
        "test.py",
        r"
counter = 0

def test_flaky():
    global counter
    counter += 1
    assert counter >= 2
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=2"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_flaky
      TRY 2 PASS [TIME] test::test_flaky
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/3 [TIME] test::test_flaky

    ----- stderr -----
    ");
}

#[test]
fn test_extra_verbose_output() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
def test_2(): pass
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-vv"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_1
            PASS [TIME] test::test_2
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    [DATETIME] DEBUG Working directory cwd=<temp_dir>/
    [DATETIME] DEBUG Searching for a project in '<temp_dir>/'
    [DATETIME] DEBUG The ancestor directories contain no `pyproject.toml`. Falling back to a virtual project.
    [DATETIME] DEBUG Found test paths path_count=1
    [DATETIME] INFO Collected all tests in [TIME]
    [DATETIME] DEBUG Partitioning tests num_workers=1
    [DATETIME] INFO Spawning 1 workers
    [DATETIME] INFO Worker 0 spawned with 2 tests
    [DATETIME] INFO Waiting for 1 workers to complete (Ctrl+C to cancel)
    [DATETIME] DEBUG Trying to parse `monkeypatch` as a fixture
    [DATETIME] DEBUG Trying to parse `capsys` as a fixture
    [DATETIME] DEBUG Trying to parse `capfd` as a fixture
    [DATETIME] DEBUG Trying to parse `capsysbinary` as a fixture
    [DATETIME] DEBUG Trying to parse `capfdbinary` as a fixture
    [DATETIME] DEBUG Trying to parse `caplog` as a fixture
    [DATETIME] DEBUG Trying to parse `tmp_path` as a fixture
    [DATETIME] DEBUG Trying to parse `temp_path` as a fixture
    [DATETIME] DEBUG Trying to parse `temp_dir` as a fixture
    [DATETIME] DEBUG Trying to parse `tmpdir` as a fixture
    [DATETIME] DEBUG Trying to parse `tmp_path_factory` as a fixture
    [DATETIME] DEBUG Trying to parse `tmpdir_factory` as a fixture
    [DATETIME] DEBUG Trying to parse `recwarn` as a fixture
    [DATETIME] DEBUG Running test `test::test_1`
    [DATETIME] DEBUG Running test `test::test_2`
    [DATETIME] INFO Worker 0 completed successfully in [TIME]
    [DATETIME] INFO All workers completed
    ");
}

#[test]
fn test_trace_verbose_output() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-vvv"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    1   [TIME] DEBUG karva::commands::test Working directory, cwd=<temp_dir>/
    1   [TIME] DEBUG karva_metadata Searching for a project in '<temp_dir>/'
    1   [TIME] DEBUG karva_metadata The ancestor directories contain no `pyproject.toml`. Falling back to a virtual project.
    1   [TIME] DEBUG karva_runner::orchestration Found test paths, path_count=1
    1   [TIME] INFO karva_runner::orchestration Collected all tests in [TIME]
    1   [TIME] DEBUG karva_runner::orchestration Partitioning tests, num_workers=1
    1   [TIME] INFO karva_runner::orchestration Spawning 1 workers
    1   [TIME] INFO karva_runner::orchestration Worker 0 spawned with 1 tests
    1   [TIME] INFO karva_runner::orchestration Waiting for 1 workers to complete (Ctrl+C to cancel)
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `monkeypatch` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `capsys` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `capfd` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `capsysbinary` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `capfdbinary` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `caplog` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `tmp_path` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `temp_path` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `temp_dir` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `tmpdir` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `tmp_path_factory` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `tmpdir_factory` as a fixture
    1   [TIME] DEBUG karva_test_semantic::extensions::fixtures Trying to parse `recwarn` as a fixture
    1   [TIME] DEBUG karva_test_semantic::runner::package_runner Running test `test::test_1`
    1   [TIME] INFO karva_runner::orchestration Worker 0 completed successfully in [TIME]
    1   [TIME] INFO karva_runner::orchestration All workers completed
    ");
}

#[test]
fn test_quiet_output() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
def test_2(): pass
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

/// `-qq` is silent: even the summary line emitted by `-q` must be suppressed,
/// so a failing run under `-qq` produces no stdout at all.
#[test]
fn qq_is_silent_not_quiet() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_pass(): pass
def test_fail(): assert False
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["--status-level=none", "--final-status-level=none"]), @"
    success: false
    exit_code: 1
    ----- stdout -----

    ----- stderr -----
    ");
}

#[test]
fn test_color_never_strips_ansi() {
    let context = TestContext::with_file("test.py", "def test_1(): pass");

    assert_cmd_snapshot!(context.command_no_parallel().args(["--color", "never"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_color_invalid_value() {
    let context = TestContext::with_file("test.py", "def test_1(): pass");

    assert_cmd_snapshot!(context.command().args(["--color", "rainbow"]), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    error: invalid value 'rainbow' for '--color <COLOR>'
      [possible values: auto, always, never]

    For more information, try '--help'.
    ");
}

/// `--no-cache` disables reading duration history but the run should still succeed.
#[test]
fn test_no_cache_flag() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
def test_2(): pass
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--no-cache"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_1
            PASS [TIME] test::test_2
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_no_progress_hides_per_test_lines() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
def test_2(): pass
def test_3(): pass
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

/// `--no-progress` still emits diagnostics for failing tests.
#[test]
fn test_no_progress_with_failure_shows_diagnostics() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
def test_2(): assert False
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: false
    exit_code: 1
    ----- stdout -----

    diagnostics:

    error[test-failure]: Test `test_2` failed
     --> test.py:3:5
      |
    3 | def test_2(): assert False
      |     ^^^^^^
      |
    info: Test failed here
     --> test.py:3:1
      |
    3 | def test_2(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// `--retry 0` is a no-op — failing tests still fail and are not re-run.
#[test]
fn test_retry_zero_is_noop() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_fail(): assert False
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry").arg("0"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_fail

    diagnostics:

    error[test-failure]: Test `test_fail` failed
     --> test.py:2:5
      |
    2 | def test_fail(): assert False
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> test.py:2:1
      |
    2 | def test_fail(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// A test that always fails exhausts retries and ends up reported as failed.
#[test]
fn test_retry_exhausts_on_always_failing_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_always_fails(): assert False
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry").arg("2"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_always_fails
      TRY 2 FAIL [TIME] test::test_always_fails
      TRY 3 FAIL [TIME] test::test_always_fails

    diagnostics:

    error[test-failure]: Test `test_always_fails` failed
     --> test.py:2:5
      |
    2 | def test_always_fails(): assert False
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:2:1
      |
    2 | def test_always_fails(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// `--status-level=fail` suppresses per-attempt `TRY N FAIL` lines because
/// retry attempts only show at level `retry` or higher.
#[test]
fn test_retry_attempts_hidden_below_retry_status_level() {
    let context = TestContext::with_file(
        "test.py",
        r"
counter = 0

def test_flaky():
    global counter
    counter += 1
    assert counter >= 2
        ",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--retry=2", "--status-level=fail"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/3 [TIME] test::test_flaky

    ----- stderr -----
    "
    );
}

/// `--final-status-level=retry` elevates the summary so it shows when any
/// test was retried, even if everything ultimately passed.
#[test]
fn test_final_status_level_retry_shows_summary_on_retried_success() {
    let context = TestContext::with_file(
        "test.py",
        r"
counter = 0

def test_flaky():
    global counter
    counter += 1
    assert counter >= 2
        ",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--retry=2",
            "--status-level=none",
            "--final-status-level=retry",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/3 [TIME] test::test_flaky

    ----- stderr -----
    "
    );
}

/// `--final-status-level=fail` does not show the summary on a successful
/// retried run — the elevation is specific to `retry` and above.
#[test]
fn test_final_status_level_fail_hides_summary_on_retried_success() {
    let context = TestContext::with_file(
        "test.py",
        r"
counter = 0

def test_flaky():
    global counter
    counter += 1
    assert counter >= 2
        ",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--retry=2",
            "--status-level=none",
            "--final-status-level=fail",
        ]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    "
    );
}

/// Multiple flaky tests in a single run produce a summary count of `(N flaky)`
/// and one `FLAKY` line per test in the post-summary block.
#[test]
fn test_multiple_flaky_tests_summary_and_block() {
    let context = TestContext::with_file(
        "test.py",
        r"
counter_a = 0
counter_b = 0

def test_flaky_a():
    global counter_a
    counter_a += 1
    assert counter_a >= 2

def test_flaky_b():
    global counter_b
    counter_b += 1
    assert counter_b >= 2
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["--retry=2"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
      TRY 1 FAIL [TIME] test::test_flaky_a
      TRY 2 PASS [TIME] test::test_flaky_a
      TRY 1 FAIL [TIME] test::test_flaky_b
      TRY 2 PASS [TIME] test::test_flaky_b
    ────────────
         Summary [TIME] 2 tests run: 2 passed (2 flaky), 0 skipped
       FLAKY 2/3 [TIME] test::test_flaky_a
       FLAKY 2/3 [TIME] test::test_flaky_b

    ----- stderr -----
    ");
}

/// A run mixing a clean pass, a flaky-then-pass, and an outright failure
/// reports each component correctly: `passed` includes the flaky one,
/// `failed` is the outright failure, and the FLAKY block lists only the
/// retried-then-passed test.
#[test]
fn test_mixed_pass_flaky_fail_summary() {
    let context = TestContext::with_file(
        "test.py",
        r"
counter = 0

def test_clean_pass(): pass

def test_flaky():
    global counter
    counter += 1
    assert counter >= 2

def test_always_fails(): assert False
        ",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--retry=2", "--status-level=retry", "--no-fail-fast"]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
      TRY 1 FAIL [TIME] test::test_flaky
      TRY 2 PASS [TIME] test::test_flaky
      TRY 1 FAIL [TIME] test::test_always_fails
      TRY 2 FAIL [TIME] test::test_always_fails
      TRY 3 FAIL [TIME] test::test_always_fails

    diagnostics:

    error[test-failure]: Test `test_always_fails` failed
      --> test.py:11:5
       |
    11 | def test_always_fails(): assert False
       |     ^^^^^^^^^^^^^^^^^
       |
    info: Test failed here
      --> test.py:11:1
       |
    11 | def test_always_fails(): assert False
       | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
       |

    ────────────
         Summary [TIME] 3 tests run: 2 passed (1 flaky), 1 failed, 0 skipped
       FLAKY 2/3 [TIME] test::test_flaky

    ----- stderr -----
    "
    );
}

/// A run with no retries triggered must produce no `TRY` lines, no `FLAKY`
/// block, and no `(0 flaky)` text in the summary — guards against the new
/// machinery accidentally firing on non-retried tests.
#[test]
fn test_no_retry_run_unchanged_output() {
    let context = TestContext::with_file("test.py", "def test_a(): pass\ndef test_b(): pass\n");

    assert_cmd_snapshot!(context.command_no_parallel().args(["--retry=2"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_a
            PASS [TIME] test::test_b
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

/// `--status-level=none` suppresses both the regular result lines AND the
/// per-attempt `TRY` lines.
#[test]
fn test_status_level_none_suppresses_try_lines() {
    let context = TestContext::with_file(
        "test.py",
        r"
counter = 0

def test_flaky():
    global counter
    counter += 1
    assert counter >= 2
        ",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--retry=2", "--status-level=none"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/3 [TIME] test::test_flaky

    ----- stderr -----
    "
    );
}

/// At `--status-level=fail` the per-attempt `TRY` lines stay hidden, but a
/// final FAIL line for an exhausted-retry test does still display.
#[test]
fn test_status_level_fail_hides_try_but_shows_final_fail() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_always_fails(): assert False
        ",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--retry=1", "--status-level=fail"]),
        @r"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker

    diagnostics:

    error[test-failure]: Test `test_always_fails` failed
     --> test.py:2:5
      |
    2 | def test_always_fails(): assert False
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:2:1
      |
    2 | def test_always_fails(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );
}

/// `KARVA`, `KARVA_WORKER_ID`, `KARVA_RUN_ID`, `KARVA_WORKSPACE_ROOT`,
/// `KARVA_PROFILE`, `KARVA_TEST_THREADS`, and `KARVA_VERSION` are exposed to
/// the test process. The values come from the worker spawn (not from the
/// test's own `os.environ` writes), so a single passing test is enough to
/// exercise all of them.
#[test]
fn test_karva_static_env_vars() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import re

def test_static_env():
    assert os.environ["KARVA"] == "1"
    assert os.environ["KARVA_WORKER_ID"] == "0"
    assert re.fullmatch(
        r"\d+-[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}",
        os.environ["KARVA_RUN_ID"],
    ), os.environ["KARVA_RUN_ID"]
    assert os.environ["KARVA_WORKSPACE_ROOT"] == os.getcwd()
    assert os.environ["KARVA_PROFILE"] == "default"
    assert os.environ["KARVA_TEST_THREADS"] == "1"
    assert re.fullmatch(r"\d+\.\d+\.\d+(-[A-Za-z0-9.]+)?", os.environ["KARVA_VERSION"]), \
        os.environ["KARVA_VERSION"]
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_static_env
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

/// `KARVA_PROFILE` reflects the active profile when `--profile` is passed,
/// rather than always being `"default"`.
#[test]
fn test_karva_profile_env_reflects_active_profile() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[profile.fast.test]
",
        ),
        (
            "test.py",
            r#"
import os

def test_profile_env():
    assert os.environ["KARVA_PROFILE"] == "fast"
        "#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel().arg("--profile=fast"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_profile_env
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

/// `KARVA_TEST_NAME` is set to the qualified test name before each attempt,
/// including the parametrize-arg suffix.
#[test]
fn test_karva_test_name_env() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import karva

def test_plain():
    assert os.environ["KARVA_TEST_NAME"] == "test::test_plain"

@karva.tags.parametrize("value", [1, 2])
def test_param(value):
    assert os.environ["KARVA_TEST_NAME"] == f"test::test_param(value={value})"
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_plain
            PASS [TIME] test::test_param(value=1)
            PASS [TIME] test::test_param(value=2)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

/// `KARVA_ATTEMPT` and `KARVA_TOTAL_ATTEMPTS` are exposed to the test process.
/// Without `--retry`, both are `"1"`.
#[test]
fn test_karva_attempt_env_default() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os

def test_default_attempt():
    assert os.environ['KARVA_ATTEMPT'] == '1'
    assert os.environ['KARVA_TOTAL_ATTEMPTS'] == '1'
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_default_attempt
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

/// With `--retry=N`, `KARVA_TOTAL_ATTEMPTS` is `N+1` and `KARVA_ATTEMPT`
/// increments on each retry. The test below asserts the attempt number and
/// only passes on attempt 3, exercising both env vars across the retry loop.
#[test]
fn test_karva_attempt_env_increments_on_retry() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os

def test_attempt_env():
    assert os.environ['KARVA_TOTAL_ATTEMPTS'] == '5'
    assert int(os.environ['KARVA_ATTEMPT']) == 3
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=4"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_attempt_env
      TRY 2 FAIL [TIME] test::test_attempt_env
      TRY 3 PASS [TIME] test::test_attempt_env
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 3/5 [TIME] test::test_attempt_env

    ----- stderr -----
    ");
}

/// `--max-fail` must reject zero because the underlying type is `NonZeroU32`.
#[test]
fn test_max_fail_zero_is_rejected() {
    let context = TestContext::with_file("test.py", "def test_1(): pass");

    assert_cmd_snapshot!(context.command().args(["--max-fail", "0"]), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    error: invalid value '0' for '--max-fail <N>': number would be zero for non-zero type

    For more information, try '--help'.
    ");
}

/// `--num-workers` followed by a non-numeric value should trigger clap's parser.
#[test]
fn test_num_workers_invalid_value() {
    let context = TestContext::with_file("test.py", "def test_1(): pass");

    assert_cmd_snapshot!(context.command().args(["--num-workers", "abc"]), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    error: invalid value 'abc' for '--num-workers <NUM_WORKERS>': invalid digit found in string

    For more information, try '--help'.
    ");
}

/// `--num-workers 1` behaves like `--no-parallel`: one worker handles every test.
#[test]
fn test_num_workers_one_matches_no_parallel() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
def test_2(): pass
",
    );

    assert_cmd_snapshot!(context.command().args(["--num-workers", "1"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_1
            PASS [TIME] test::test_2
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

/// `--durations` requires a numeric argument.
#[test]
fn test_durations_invalid_value() {
    let context = TestContext::with_file("test.py", "def test_1(): pass");

    assert_cmd_snapshot!(context.command().args(["--durations", "abc"]), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    error: invalid value 'abc' for '--durations <N>': invalid digit found in string

    For more information, try '--help'.
    ");
}

/// When `--fail-fast` and `--no-fail-fast` are mixed, clap's `overrides_with`
/// wires them so that whichever flag appears last wins.
#[test]
fn test_no_fail_fast_after_fail_fast_wins() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): assert False
def test_2(): assert False
def test_3(): pass
",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--fail-fast", "--no-fail-fast", "--status-level=none"]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    diagnostics:

    error[test-failure]: Test `test_1` failed
     --> test.py:2:5
      |
    2 | def test_1(): assert False
      |     ^^^^^^
      |
    info: Test failed here
     --> test.py:2:1
      |
    2 | def test_1(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    error[test-failure]: Test `test_2` failed
     --> test.py:3:5
      |
    3 | def test_2(): assert False
      |     ^^^^^^
      |
    info: Test failed here
     --> test.py:3:1
      |
    3 | def test_2(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 failed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn test_fail_fast_after_no_fail_fast_wins() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): assert False
def test_2(): assert False
def test_3(): pass
",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--no-fail-fast", "--fail-fast", "--status-level=none"]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    diagnostics:

    error[test-failure]: Test `test_1` failed
     --> test.py:2:5
      |
    2 | def test_1(): assert False
      |     ^^^^^^
      |
    info: Test failed here
     --> test.py:2:1
      |
    2 | def test_1(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );
}

/// `--max-fail` wins over `--no-fail-fast` regardless of order.
#[test]
fn test_max_fail_beats_no_fail_fast() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): assert False
def test_2(): assert False
def test_3(): assert False
",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--no-fail-fast", "--max-fail=2", "--status-level=none"]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    diagnostics:

    error[test-failure]: Test `test_1` failed
     --> test.py:2:5
      |
    2 | def test_1(): assert False
      |     ^^^^^^
      |
    info: Test failed here
     --> test.py:2:1
      |
    2 | def test_1(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    error[test-failure]: Test `test_2` failed
     --> test.py:3:5
      |
    3 | def test_2(): assert False
      |     ^^^^^^
      |
    info: Test failed here
     --> test.py:3:1
      |
    3 | def test_2(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    "
    );
}

/// `karva test nonexistent.py` should exit with code 2 and an error message
/// that points at the missing path.
#[test]
fn test_nonexistent_path_exits_nonzero() {
    let context = TestContext::new();

    assert_cmd_snapshot!(context.command().arg("missing.py"), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: path `<temp_dir>/missing.py` could not be found
    ");
}

/// `karva` with no subcommand is a clap error: exit code 2, help on stderr.
#[test]
fn test_no_subcommand_prints_help() {
    let context = TestContext::new();

    assert_cmd_snapshot!(context.karva_command_in(context.root()), @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    A Python test runner.

    Usage: karva <COMMAND>

    Commands:
      test         Run tests
      snapshot     Manage snapshots created by `karva.assert_snapshot()`
      cache        Manage the karva cache
      show-config  Print the resolved configuration karva would run with
      version      Display Karva's version
      help         Print this message or the help of the given subcommand(s)

    Options:
      -h, --help     Print help
      -V, --version  Print version
    ");
}

/// `karva testx` (typo of `test`) should suggest the closest subcommand.
#[test]
fn test_unknown_subcommand_suggests_correction() {
    let context = TestContext::new();

    let mut command = context.karva_command_in(context.root());
    command.arg("testx");

    assert_cmd_snapshot!(command, @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    error: unrecognized subcommand 'testx'

      tip: a similar subcommand exists: 'test'

    Usage: karva <COMMAND>

    For more information, try '--help'.
    ");
}

/// `--test-prefix` requires a value.
#[test]
fn test_test_prefix_requires_value() {
    let context = TestContext::with_file("test.py", "def test_1(): pass");

    assert_cmd_snapshot!(context.command().arg("--test-prefix"), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    error: a value is required for '--test-prefix <TEST_PREFIX>' but none was supplied

    For more information, try '--help'.
    ");
}

/// `--slow-timeout` flags tests whose total duration exceeds the threshold.
/// At `--status-level=slow` the SLOW line precedes the PASS line and the
/// summary picks up a `1 slow` counter.
#[test]
fn test_slow_timeout_emits_slow_line_and_counter() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time

def test_slow():
    time.sleep(0.05)

def test_fast():
    pass
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--slow-timeout=0.001",
            "--status-level=slow",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            SLOW [TIME] test::test_slow
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped, 1 slow

    ----- stderr -----
    "
    );
}

/// Below `--status-level=slow` the SLOW line is suppressed, but the slow
/// counter still appears in the summary so `--final-status-level=slow` users
/// can elevate the summary independently.
#[test]
fn test_slow_timeout_counter_visible_below_slow_status_level() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time

def test_slow():
    time.sleep(0.05)
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--slow-timeout=0.001",
            "--status-level=fail",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped, 1 slow

    ----- stderr -----
    "
    );
}

/// A test that completes well under the threshold is not reported as slow
/// and does not bump the counter.
#[test]
fn test_slow_timeout_does_not_flag_fast_tests() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_fast(): pass
",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--slow-timeout=60",
            "--status-level=slow",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
}
