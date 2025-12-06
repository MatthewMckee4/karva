use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
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

    assert_cmd_snapshot!(context.command().arg("test_file1.py"), @r"
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_single_function_shadowed_by_file() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
def test_2(): pass",
    );

    assert_cmd_snapshot!(context.command().args(["test.py::test_1", "test.py"]), @r"
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
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_single_function_shadowed_by_directory() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_1(): pass
def test_2(): pass",
    );

    assert_cmd_snapshot!(context.command().args(["test.py::test_1", "."]), @r"
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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

    assert_cmd_snapshot!(context.command().arg("--output-format").arg("concise"), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_fail::test_1(fixture_1=1) ... FAILED
    test test_fail::test_2 ... FAILED
    test test_fail::test_3 ... FAILED

    diagnostics:

    test_fail.py:5:5: warning[invalid-fixture-finalizer] Discovered an invalid fixture finalizer `fixture_1`
    test_fail.py:9:5: error[test-failure] Test `test_1` failed
    test_fail.py:13:5: error[fixture-failure] Fixture `fixture_2` failed
    test_fail.py:16:5: error[missing-fixtures] Test `test_2` has missing fixtures: `fixture_2`
    test_fail.py:19:5: error[test-failure] Test `test_3` failed

    test result: FAILED. 0 passed; 3 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
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

    assert_cmd_snapshot!(context.command(), @r"
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
#[ignore = "Will fail unless `maturin build` is ran"]
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

fn get_parametrize_function(package: &str) -> String {
    if package == "pytest" {
        "pytest.mark.parametrize".to_string()
    } else {
        "karva.tags.parametrize".to_string()
    }
}

fn get_skip_decorator(framework: &str) -> &str {
    if framework == "pytest" {
        "pytest.mark.skip"
    } else {
        "karva.tags.skip"
    }
}

fn get_skipif_decorator(framework: &str) -> &str {
    if framework == "pytest" {
        "pytest.mark.skipif"
    } else {
        "karva.tags.skip"
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrize(#[values("pytest", "karva")] package: &str) {
    let context = TestContext::with_file(
        "test_parametrize.py",
        &format!(
            r"
        import {package}

        @{parametrize_function}(('a', 'b', 'expected'), [
            (1, 2, 3),
            (2, 3, 5),
            (3, 4, 7),
        ])
        def test_parametrize(a, b, expected):
            assert a + b == expected
    ",
            parametrize_function = &get_parametrize_function(package),
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test_parametrize::test_parametrize(a=1, b=2, expected=3) ... ok
        test test_parametrize::test_parametrize(a=2, b=3, expected=5) ... ok
        test test_parametrize::test_parametrize(a=3, b=4, expected=7) ... ok

        test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_stdout() {
    let context = TestContext::with_file(
        "test_std_out_redirected.py",
        r"
        def test_std_out_redirected():
            print('Hello, world!')
        ",
    );

    assert_cmd_snapshot!(context.command().args(["-s"]), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_std_out_redirected::test_std_out_redirected ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]
    Hello, world!

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
#[ignore = "Will fail unless `maturin build` is ran"]
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

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_functionality(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_skip_decorator(framework);

    let context = TestContext::with_file(
        "test_skip.py",
        &format!(
            r"
        import {framework}

        @{decorator}('This test is skipped')
        def test_1():
            assert False

        ",
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test_skip::test_1 ... skipped: This test is skipped

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_text_file() {
    let context = TestContext::with_file("random.txt", "pass");

    assert_cmd_snapshot!(
        context.command().args(["random.txt"]),
        @r"
    success: false
    exit_code: 1
    ----- stdout -----
    discovery diagnostics:

    error[invalid-path]: Invalid path: path `<temp_dir>/random.txt` has a wrong file extension

    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_invalid_path() {
    let context = TestContext::new();

    assert_cmd_snapshot!(context.command().arg("non_existing_path.py"), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    discovery diagnostics:

    error[invalid-path]: Invalid path: path `<temp_dir>/non_existing_path.py` could not be found

    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_runtime_skip_pytest(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test_pytest_skip.py",
        &format!(
            r"
import {framework}

def test_skip_with_reason():
    {framework}.skip('This test is skipped at runtime')
    assert False, 'This should not be reached'

def test_skip_without_reason():
    {framework}.skip()
    assert False, 'This should not be reached'

def test_conditional_skip():
    condition = True
    if condition:
        {framework}.skip('Condition was true')
    assert False, 'This should not be reached'
        "
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test_pytest_skip::test_skip_with_reason ... skipped: This test is skipped at runtime
        test test_pytest_skip::test_skip_without_reason ... skipped
        test test_pytest_skip::test_conditional_skip ... skipped: Condition was true

        test result: ok. 0 passed; 0 failed; 3 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skipif_true_condition(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_skipif_decorator(framework);

    let context = TestContext::with_file(
        "test_skipif.py",
        &format!(
            r"
import {framework}

@{decorator}(True, reason='Condition is true')
def test_1():
    assert False
        ",
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test_skipif::test_1 ... skipped: Condition is true

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skipif_false_condition(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_skipif_decorator(framework);

    let context = TestContext::with_file(
        "test_skipif.py",
        &format!(
            r"
import {framework}

@{decorator}(False, reason='Should not skip')
def test_1():
    assert True
        ",
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test_skipif::test_1 ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skipif_multiple_conditions(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_skipif_decorator(framework);

    let context = TestContext::with_file(
        "test_skipif.py",
        &format!(
            r"
import {framework}

@{decorator}(False, True, False, reason='One condition is true')
def test_1():
    assert False
        ",
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test_skipif::test_1 ... skipped: One condition is true

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skipif_mixed_tests(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_skipif_decorator(framework);

    let context = TestContext::with_file(
        "test_skipif.py",
        &format!(
            r"
import {framework}

@{decorator}(True, reason='Skipped')
def test_skip_this():
    assert False

@{decorator}(False, reason='Not skipped')
def test_run_this():
    assert True

def test_normal():
    assert True
        ",
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test_skipif::test_skip_this ... skipped: Skipped
        test test_skipif::test_run_this ... ok
        test test_skipif::test_normal ... ok

        test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
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

    assert_cmd_snapshot!(context.command().args(["--fail-fast"]), @r"
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

fn get_expect_fail_decorator(framework: &str) -> &str {
    if framework == "pytest" {
        "pytest.mark.xfail"
    } else {
        "karva.tags.expect_fail"
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_that_fails(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_expect_fail_decorator(framework);

    let context = TestContext::with_file(
        "test_expect_fail.py",
        &format!(
            r"
import {framework}

@{decorator}(reason='Known bug')
def test_1():
    assert False
        "
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test_expect_fail::test_1 ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_that_passes_karva() {
    let decorator = get_expect_fail_decorator("karva");

    let context = TestContext::with_file(
        "test_expect_fail.py",
        &format!(
            r"
import karva

@{decorator}(reason='Expected to fail but passes')
def test_1():
    assert True
        "
        ),
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_expect_fail::test_1 ... FAILED

    diagnostics:

    error[test-pass-on-expect-failure]: Test `test_1` passes when expected to fail
     --> test_expect_fail.py:5:5
      |
    4 | @karva.tags.expect_fail(reason='Expected to fail but passes')
    5 | def test_1():
      |     ^^^^^^
    6 |     assert True
      |
    info: Reason: Expected to fail but passes

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_that_passes_pytest() {
    let decorator = get_expect_fail_decorator("pytest");

    let context = TestContext::with_file(
        "test_expect_fail.py",
        &format!(
            r"
import pytest

@{decorator}(reason='Expected to fail but passes')
def test_1():
    assert True
        "
        ),
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_expect_fail::test_1 ... FAILED

    diagnostics:

    error[test-pass-on-expect-failure]: Test `test_1` passes when expected to fail
     --> test_expect_fail.py:5:5
      |
    4 | @pytest.mark.xfail(reason='Expected to fail but passes')
    5 | def test_1():
      |     ^^^^^^
    6 |     assert True
      |
    info: Reason: Expected to fail but passes

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_with_true_condition(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_expect_fail_decorator(framework);

    let context = TestContext::with_file(
        "test_expect_fail.py",
        &format!(
            r"
import {framework}

@{decorator}(True, reason='Condition is true')
def test_1():
    assert False
        "
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test_expect_fail::test_1 ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_with_false_condition(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_expect_fail_decorator(framework);

    let context = TestContext::with_file(
        "test_expect_fail.py",
        &format!(
            r"
import {framework}

@{decorator}(False, reason='Condition is false')
def test_1():
    assert True
        "
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test_expect_fail::test_1 ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_mixed_tests_karva() {
    let decorator = get_expect_fail_decorator("karva");

    let context = TestContext::with_file(
        "test_expect_fail.py",
        &format!(
            r"
import karva

@{decorator}(reason='Expected to fail')
def test_expected_to_fail():
    assert False

def test_normal_pass():
    assert True

@{decorator}
def test_expected_fail_passes():
    assert True
        "
        ),
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_expect_fail::test_expected_to_fail ... ok
    test test_expect_fail::test_normal_pass ... ok
    test test_expect_fail::test_expected_fail_passes ... FAILED

    diagnostics:

    error[test-pass-on-expect-failure]: Test `test_expected_fail_passes` passes when expected to fail
      --> test_expect_fail.py:12:5
       |
    11 | @karva.tags.expect_fail
    12 | def test_expected_fail_passes():
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^
    13 |     assert True
       |

    test result: FAILED. 2 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_mixed_tests_pytest() {
    let decorator = get_expect_fail_decorator("pytest");

    let context = TestContext::with_file(
        "test_expect_fail.py",
        &format!(
            r"
import pytest

@{decorator}(reason='Expected to fail')
def test_expected_to_fail():
    assert False

def test_normal_pass():
    assert True

@{decorator}
def test_expected_fail_passes():
    assert True
        "
        ),
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_expect_fail::test_expected_to_fail ... ok
    test test_expect_fail::test_normal_pass ... ok
    test test_expect_fail::test_expected_fail_passes ... FAILED

    diagnostics:

    error[test-pass-on-expect-failure]: Test `test_expected_fail_passes` passes when expected to fail
      --> test_expect_fail.py:12:5
       |
    11 | @pytest.mark.xfail
    12 | def test_expected_fail_passes():
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^
    13 |     assert True
       |

    test result: FAILED. 2 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fail_function() {
    let context = TestContext::with_file(
        "test_fail.py",
        r"
import karva

def test_with_fail():
    karva.fail('This is a custom failure message')

def test_normal():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_fail::test_with_fail ... FAILED
    test test_fail::test_normal ... ok

    diagnostics:

    error[test-failure]: Test `test_with_fail` failed
     --> test_fail.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_with_fail():
      |     ^^^^^^^^^^^^^^
    5 |     karva.fail('This is a custom failure message')
      |
    info: Test failed here
     --> test_fail.py:5:5
      |
    4 | def test_with_fail():
    5 |     karva.fail('This is a custom failure message')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    6 |
    7 | def test_normal():
      |
    info: This is a custom failure message

    test result: FAILED. 1 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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
#[ignore = "Will fail unless `maturin build` is ran"]
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

    assert_cmd_snapshot!(context.command().arg("-s"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_setenv(monkeypatch=<MockEnv object>) ... ok
    test test::test_1 ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
