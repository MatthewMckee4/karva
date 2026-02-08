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

    assert_cmd_snapshot!(context.command_no_parallel().arg("test_file1.py"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_file1::test_1 ... ok
    test test_file1::test_2 ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_empty_file() {
    let context = TestContext::with_file("test.py", "");

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_empty_directory() {
    let context = TestContext::new();

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command().arg("test.py::test_1"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().args(["test.py::test_1", "test.py"]), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... ok
    test test::test_2 ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().args(["test.py::test_1", "."]), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... ok
    test test::test_2 ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_no_tests_found() {
    let context = TestContext::with_file("test_no_tests.py", r"");

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_pass::test_pass ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_fail::test_fail ... FAILED

    diagnostics:

    error[test-failure]: Test `test_fail` failed
     --> test_fail.py:2:5
      |
    2 | def test_fail():
      |     ^^^^^^^^^
    3 |     assert False
      |
    info: Test failed here
     --> test_fail.py:3:5
      |
    2 | def test_fail():
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--output-format").arg("concise"), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_fail::test_1(fixture_1=1) ... FAILED
    test test_fail::test_2 ... FAILED
    test test_fail::test_3 ... FAILED

    diagnostics:

    test_fail.py:5:5: warning[invalid-fixture-finalizer] Discovered an invalid fixture finalizer `fixture_1`
    test_fail.py:9:5: error[test-failure] Test `test_1` failed
    test_fail.py:16:5: error[missing-fixtures] Test `test_2` has missing fixtures: `fixture_2`
    test_fail.py:19:5: error[test-failure] Test `test_3` failed

    test result: FAILED. 0 passed; 3 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test tests.test_fail::test_fail ... FAILED
    test tests.test_fail::test_fail2 ... FAILED

    diagnostics:

    error[test-failure]: Test `test_fail` failed
     --> tests/test_fail.py:2:5
      |
    2 | def test_fail():
      |     ^^^^^^^^^
    3 |     assert False
      |
    info: Test failed here
     --> tests/test_fail.py:3:5
      |
    2 | def test_fail():
    3 |     assert False
      |     ^^^^^^^^^^^^
    4 |
    5 | def test_fail2():
      |

    error[test-failure]: Test `test_fail2` failed
     --> tests/test_fail.py:5:5
      |
    3 |     assert False
    4 |
    5 | def test_fail2():
      |     ^^^^^^^^^^
    6 |     assert False, 'Test failed'
      |
    info: Test failed here
     --> tests/test_fail.py:6:5
      |
    5 | def test_fail2():
    6 |     assert False, 'Test failed'
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed

    test result: FAILED. 0 passed; 2 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_cross_file::test_with_helper ... FAILED

    diagnostics:

    error[test-failure]: Test `test_with_helper` failed
     --> test_cross_file.py:4:5
      |
    2 | from helper import validate_data
    3 |
    4 | def test_with_helper():
      |     ^^^^^^^^^^^^^^^^
    5 |     validate_data([])
      |
    info: Test failed here
     --> helper.py:4:9
      |
    2 | def validate_data(data):
    3 |     if not data:
    4 |         assert False, 'Data validation failed'
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    5 |     return True
      |
    info: Data validation failed

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("-s"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Hello, world!
    test test_std_out_redirected::test_std_out_redirected ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.command().arg("-s"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Hello, world!
    test test_std_out_redirected::test_std_out_redirected ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_std_out_redirected::test_std_out_redirected ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_multiple_fixtures_not_found() {
    let context = TestContext::with_file(
        "test_multiple_fixtures_not_found.py",
        "def test_multiple_fixtures_not_found(a, b, c): ...",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_multiple_fixtures_not_found::test_multiple_fixtures_not_found ... FAILED

    diagnostics:

    error[missing-fixtures]: Test `test_multiple_fixtures_not_found` has missing fixtures
     --> test_multiple_fixtures_not_found.py:1:5
      |
    1 | def test_multiple_fixtures_not_found(a, b, c): ...
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `a`, `b`, `c`

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_text_file_in_directory() {
    let context = TestContext::with_files([
        ("test_sample.py", "def test_sample(): assert True"),
        ("random.txt", "pass"),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_sample::test_sample ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command().args(["-q"]), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command().args(["-q"]), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_fixture_generator(fixture_generator=1) ... ok

    diagnostics:

    warning[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `fixture_generator`
     --> test.py:5:5
      |
    4 | @karva.fixture
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
    6 |     yield 1
    7 |     yield 2
      |
    info: Fixture had more than one yield statement

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_fixture_generator(fixture_generator=1) ... FAILED

    diagnostics:

    warning[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `fixture_generator`
     --> test.py:5:5
      |
    4 | @karva.fixture
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
    6 |     yield 1
    7 |     yield 2
      |
    info: Fixture had more than one yield statement

    error[test-failure]: Test `test_fixture_generator` failed
      --> test.py:9:5
       |
     7 |     yield 2
     8 |
     9 | def test_fixture_generator(fixture_generator):
       |     ^^^^^^^^^^^^^^^^^^^^^^
    10 |     assert fixture_generator == 2
       |
    info: Test ran with arguments:
    info: `fixture_generator`: `1`
    info: Test failed here
      --> test.py:10:5
       |
     9 | def test_fixture_generator(fixture_generator):
    10 |     assert fixture_generator == 2
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
       |

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_fixture_generator(fixture_generator=1) ... ok

    diagnostics:

    warning[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `fixture_generator`
     --> test.py:5:5
      |
    4 | @karva.fixture
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
    6 |     yield 1
    7 |     raise ValueError("fixture error")
      |
    info: Failed to reset fixture: fixture error

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
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

    assert_cmd_snapshot!(context.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_fixture_generator ... FAILED

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `fixture_generator`
     --> test.py:5:5
      |
    4 | @karva.fixture(scope='ssession')
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
    6 |     raise ValueError("fixture-error")
      |
    info: Invalid fixture scope: ssession

    error[missing-fixtures]: Test `test_fixture_generator` has missing fixtures
     --> test.py:8:5
      |
    6 |     raise ValueError("fixture-error")
    7 |
    8 | def test_fixture_generator(fixture_generator):
      |     ^^^^^^^^^^^^^^^^^^^^^^
    9 |     assert fixture_generator == 1
      |
    info: Missing fixtures: `fixture_generator`

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
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

    assert_cmd_snapshot!(context.command_no_parallel().args(["--fail-fast"]), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_failfast::test_first_fail ... FAILED

    diagnostics:

    error[test-failure]: Test `test_first_fail` failed
     --> test_failfast.py:2:5
      |
    2 | def test_first_fail():
      |     ^^^^^^^^^^^^^^^
    3 |     assert False, 'First test fails'
      |
    info: Test failed here
     --> test_failfast.py:3:5
      |
    2 | def test_first_fail():
    3 |     assert False, 'First test fails'
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    4 |
    5 | def test_second():
      |
    info: First test fails

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
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

    assert_cmd_snapshot!(context.command().arg("--test-prefix").arg("tests_"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_fail::tests_1 ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command().arg("-s"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_fail::test_1 ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command().arg("-s"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_fail::test_1 ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_file::test_1(fixture_very_very_very_very...=fixture_very_very_very_very...) ... FAILED

    diagnostics:

    error[test-failure]: Test `test_1` failed
     --> test_file.py:8:5
      |
    6 |     return 'fixture_very_very_very_very_very_long_name'
    7 |
    8 | def test_1(fixture_very_very_very_very_very_long_name):
      |     ^^^^^^
    9 |     assert False
      |
    info: Test ran with arguments:
    info: `fixture_very_very_very_very...`: `fixture_very_very_very_very...`
    info: Test failed here
     --> test_file.py:9:5
      |
    8 | def test_1(fixture_very_very_very_very_very_long_name):
    9 |     assert False
      |     ^^^^^^^^^^^^
      |

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("-s"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_setenv(monkeypatch=<MockEnv object>) ... ok
    test test::test_1 ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--no-progress"), @r"
    success: true
    exit_code: 0
    ----- stdout -----

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--try-import-fixtures"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_file::test_1(x=1) ... ok
    test test_file::test_2(y=1) ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--try-import-fixtures"), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_file::test_1 ... FAILED
    test test_file::test_2(y=1) ... ok

    diagnostics:

    error[missing-fixtures]: Test `test_1` has missing fixtures
     --> test_file.py:3:5
      |
    2 | from foo import x, y
    3 | def test_1(x): pass
      |     ^^^^^^
    4 | def test_2(y): pass
      |
    info: Missing fixtures: `x`
    info: Fixture `x` failed here
     --> foo.py:6:5
      |
    4 | @karva.fixture
    5 | def x():
    6 |     raise ValueError('Invalid fixture')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    7 |
    8 | @karva.fixture()
      |
    info: Invalid fixture

    test result: FAILED. 1 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry").arg("5"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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
    assert_cmd_snapshot!(context.command().args(["-v", "--num-workers", "8"]), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_a::test_1 ... ok
    test test_a::test_2 ... ok
    test test_a::test_3 ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

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
