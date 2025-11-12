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
    running 0 tests

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
    running 1 test

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
    running 1 test

    test test_fail::test_fail ... FAILED

    failures:

    test `test_fail` failed at <temp_dir>/test_fail.py:3

    failures:
        test_fail

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
    running 2 tests

    test tests.test_fail::test_fail ... FAILED
    test tests.test_fail::test_fail2 ... FAILED

    failures:

    test `test_fail` failed at <temp_dir>/tests/test_fail.py:3

    test `test_fail2` failed at <temp_dir>/tests/test_fail.py:6
    Test failed

    failures:
        test_fail
        test_fail2

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
    running 1 test

    test test_cross_file::test_with_helper ... FAILED

    failures:

    test `test_with_helper` failed at <temp_dir>/helper.py:4
    Data validation failed

    failures:
        test_with_helper

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
        running 3 tests

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
    running 1 test

    test test_std_out_redirected::test_std_out_redirected ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]
    Hello, world!

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    running 1 test

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
    running 1 test

    test test_multiple_fixtures_not_found::test_multiple_fixtures_not_found ... FAILED

    failures:

    test `test_multiple_fixtures_not_found` has missing fixtures: ["a", "b", "c"] at <temp_dir>/test_multiple_fixtures_not_found.py:1

    failures:
        test_multiple_fixtures_not_found

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_functionality(#[values("pytest", "karva")] framework: &str) {
    let decorator = if framework == "pytest" {
        "pytest.mark.skip"
    } else {
        "karva.tags.skip"
    };

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
        running 1 test

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
    running 1 test

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

    running 0 tests

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

    running 0 tests

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
    running 1 test

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
    running 1 test

    test test::test_fixture_generator [fixture_generator=1] ... FAILED

    failures:

    test `test_fixture_generator` failed at <temp_dir>/test.py:10

    warnings:

    warning: Fixture test::fixture_generator had more than one yield statement

    failures:
        test_fixture_generator

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
            raise ValueError("fixture-error")

        def test_fixture_generator(fixture_generator):
            assert fixture_generator == 1
"#,
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    running 1 test

    test test::test_fixture_generator [fixture_generator=1] ... ok

    warnings:

    warning: Failed to reset fixture test::fixture_generator

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
    discovery failures:

    invalid fixture `fixture_generator`: Invalid fixture scope: ssession at <temp_dir>/test.py:4

    running 1 test

    test test::test_fixture_generator ... FAILED

    failures:

    test `test_fixture_generator` has missing fixtures: ["fixture_generator"] at <temp_dir>/test.py:8

    failures:
        test_fixture_generator

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}

#[rstest]
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
        running 3 tests

        test <test>.test_pytest_skip::test_skip_with_reason ... skipped: This test is skipped at runtime
        test <test>.test_pytest_skip::test_skip_without_reason ... skipped: 
        test <test>.test_pytest_skip::test_conditional_skip ... skipped: Condition was true

        test result: ok. 0 passed; 0 failed; 3 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}
