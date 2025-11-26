use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use karva_test::IntegrationTestContext;
use rstest::rstest;

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_no_tests_found() {
    let context = IntegrationTestContext::with_file("test_no_tests.py", r"");

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
    let context = IntegrationTestContext::with_file(
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
fn test_one_test_fails() {
    let context = IntegrationTestContext::with_file(
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

    test failures:

    test `test_fail::test_fail` at test_fail.py:2 failed at test_fail.py:3
    note: run with `--show-traceback` to see the full traceback

    test failures:
        test_fail::test_fail at test_fail.py:2

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_two_test_fails() {
    let context = IntegrationTestContext::with_file(
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

    test failures:

    test `tests.test_fail::test_fail` at tests/test_fail.py:2 failed at tests/test_fail.py:3
    note: run with `--show-traceback` to see the full traceback

    test `tests.test_fail::test_fail2` at tests/test_fail.py:5 failed at tests/test_fail.py:6
    Test failed
    note: run with `--show-traceback` to see the full traceback

    test failures:
        tests.test_fail::test_fail at tests/test_fail.py:2
        tests.test_fail::test_fail2 at tests/test_fail.py:5

    test result: FAILED. 0 passed; 2 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_file_importing_another_file() {
    let context = IntegrationTestContext::with_files([
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

    test failures:

    test `test_cross_file::test_with_helper` at test_cross_file.py:4 failed at helper.py:4
    Data validation failed
    note: run with `--show-traceback` to see the full traceback

    test failures:
        test_cross_file::test_with_helper at test_cross_file.py:4

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
    let context = IntegrationTestContext::with_file(
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
        test test_parametrize::test_parametrize [a=1, b=2, expected=3] ... ok
        test test_parametrize::test_parametrize [a=2, b=3, expected=5] ... ok
        test test_parametrize::test_parametrize [a=3, b=4, expected=7] ... ok

        test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_stdout() {
    let context = IntegrationTestContext::with_file(
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
    let context = IntegrationTestContext::with_file(
        "test_multiple_fixtures_not_found.py",
        "def test_multiple_fixtures_not_found(a, b, c): ...",
    );

    assert_cmd_snapshot!(context.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_multiple_fixtures_not_found::test_multiple_fixtures_not_found ... FAILED

    test failures:

    test `test_multiple_fixtures_not_found::test_multiple_fixtures_not_found` has missing fixtures: ["a", "b", "c"] at test_multiple_fixtures_not_found.py:1

    test failures:
        test_multiple_fixtures_not_found::test_multiple_fixtures_not_found at test_multiple_fixtures_not_found.py:1

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_functionality(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_skip_decorator(framework);

    let context = IntegrationTestContext::with_file(
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
    let context = IntegrationTestContext::with_files([
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
    let context = IntegrationTestContext::with_file("random.txt", "pass");

    assert_cmd_snapshot!(
        context.command().args(["random.txt"]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
    discovery failures:

    path `<temp_dir>/random.txt` has a wrong file extension

    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_quiet_output() {
    let context = IntegrationTestContext::with_file(
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
fn test_invalid_path() {
    let context = IntegrationTestContext::new();

    assert_cmd_snapshot!(context.command().arg("non_existing_path.py"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    discovery failures:

    path `<temp_dir>/non_existing_path.py` could not be found

    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_generator_two_yields_passing_test() {
    let context = IntegrationTestContext::with_file(
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
    test test::test_fixture_generator [fixture_generator=1] ... ok

    warnings:

    warning: Fixture test::fixture_generator had more than one yield statement

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_generator_two_yields_failing_test() {
    let context = IntegrationTestContext::with_file(
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
    test test::test_fixture_generator [fixture_generator=1] ... FAILED

    test failures:

    test `test::test_fixture_generator [fixture_generator=1]` at test.py:9 failed at test.py:10
    note: run with `--show-traceback` to see the full traceback

    warnings:

    warning: Fixture test::fixture_generator had more than one yield statement

    test failures:
        test::test_fixture_generator [fixture_generator=1] at test.py:9

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_generator_fail_in_teardown() {
    let context = IntegrationTestContext::with_file(
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

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_fixture_generator [fixture_generator=1] ... ok

    warnings:

    warning: Failed to reset fixture test::fixture_generator
    fixture error

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_invalid_fixture() {
    let context = IntegrationTestContext::with_file(
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

    discovery failures:

    invalid fixture `fixture_generator` at test.py:4: Invalid fixture scope: ssession

    test failures:

    test `test::test_fixture_generator` has missing fixtures: ["fixture_generator"] at test.py:8

    test failures:
        test::test_fixture_generator at test.py:8

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_runtime_skip_pytest(#[values("pytest", "karva")] framework: &str) {
    let context = IntegrationTestContext::with_file(
        "<test>/test_pytest_skip.py",
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
        test <test>.test_pytest_skip::test_skip_with_reason ... skipped: This test is skipped at runtime
        test <test>.test_pytest_skip::test_skip_without_reason ... skipped
        test <test>.test_pytest_skip::test_conditional_skip ... skipped: Condition was true

        test result: ok. 0 passed; 0 failed; 3 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skipif_true_condition(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_skipif_decorator(framework);

    let context = IntegrationTestContext::with_file(
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

    let context = IntegrationTestContext::with_file(
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

    let context = IntegrationTestContext::with_file(
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

    let context = IntegrationTestContext::with_file(
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
    let context = IntegrationTestContext::with_file(
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

    test failures:

    test `test_failfast::test_first_fail` at test_failfast.py:2 failed at test_failfast.py:3
    First test fails
    note: run with `--show-traceback` to see the full traceback

    test failures:
        test_failfast::test_first_fail at test_failfast.py:2

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

    let context = IntegrationTestContext::with_file(
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
fn test_expect_fail_that_passes(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_expect_fail_decorator(framework);

    let context = IntegrationTestContext::with_file(
        "test_expect_fail.py",
        &format!(
            r"
import {framework}

@{decorator}(reason='Expected to fail but passes')
def test_1():
    assert True
        "
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: false
        exit_code: 1
        ----- stdout -----
        test test_expect_fail::test_1 ... FAILED

        test failures:

        test `test_expect_fail::test_1` at test_expect_fail.py:4 passed when it was expected to fail
        reason: Expected to fail but passes

        test failures:
            test_expect_fail::test_1 at test_expect_fail.py:4

        test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_with_true_condition(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_expect_fail_decorator(framework);

    let context = IntegrationTestContext::with_file(
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

    let context = IntegrationTestContext::with_file(
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
fn test_expect_fail_mixed_tests(#[values("pytest", "karva")] framework: &str) {
    let decorator = get_expect_fail_decorator(framework);

    let context = IntegrationTestContext::with_file(
        "test_expect_fail.py",
        &format!(
            r"
import {framework}

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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: false
        exit_code: 1
        ----- stdout -----
        test test_expect_fail::test_expected_to_fail ... ok
        test test_expect_fail::test_normal_pass ... ok
        test test_expect_fail::test_expected_fail_passes ... FAILED

        test failures:

        test `test_expect_fail::test_expected_fail_passes` at test_expect_fail.py:11 passed when it was expected to fail

        test failures:
            test_expect_fail::test_expected_fail_passes at test_expect_fail.py:11

        test result: FAILED. 2 passed; 1 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fail_function() {
    let context = IntegrationTestContext::with_file(
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

    test failures:

    test `test_fail::test_with_fail` at test_fail.py:4 failed at test_fail.py:5
    This is a custom failure message
    note: run with `--show-traceback` to see the full traceback

    test failures:
        test_fail::test_with_fail at test_fail.py:4

    test result: FAILED. 1 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_test_prefix() {
    let context = IntegrationTestContext::with_file(
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
fn test_show_traceback() {
    let context = IntegrationTestContext::with_file(
        "test_fail.py",
        r"
import karva

def foo():
    raise ValueError('bar')

def test_1():
    foo()

        ",
    );

    assert_cmd_snapshot!(context.command().arg("--show-traceback"), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_fail::test_1 ... FAILED

    test failures:

    test `test_fail::test_1` at test_fail.py:7 failed at test_fail.py:5
     | File "<temp_dir>/test_fail.py", line 8, in test_1
     |   foo()
     | File "<temp_dir>/test_fail.py", line 5, in foo
     |   raise ValueError('bar')

    test failures:
        test_fail::test_1 at test_fail.py:7

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}
