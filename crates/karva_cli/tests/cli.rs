use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use karva_test::IntegrationTestContext;
use rstest::rstest;

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_no_tests_found() {
    let case = IntegrationTestContext::with_file("test_no_tests.py", r"");

    assert_cmd_snapshot!(case.command(), @r"
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
    let case = IntegrationTestContext::with_file(
        "test_pass.py",
        r"
        def test_pass():
            assert True
        ",
    );

    assert_cmd_snapshot!(case.command(), @r"
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
    let case = IntegrationTestContext::with_file(
        "test_fail.py",
        r"
        def test_fail():
            assert False
    ",
    );

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    running 1 test
    test test_fail::test_fail ... FAILED

    fail[assertion-error]
     --> test_fail::test_fail at <temp_dir>/test_fail.py:2
     | File "<temp_dir>/test_fail.py", line 3, in test_fail
     |   assert False

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_file_importing_another_file() {
    let case = IntegrationTestContext::with_files([
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

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    running 1 test
    test test_cross_file::test_with_helper ... FAILED

    fail[assertion-error]: Data validation failed
     --> test_cross_file::test_with_helper at <temp_dir>/test_cross_file.py:4
     | File "<temp_dir>/test_cross_file.py", line 5, in test_with_helper
     |   validate_data([])
     | File "<temp_dir>/helper.py", line 4, in validate_data
     |   assert False, 'Data validation failed'

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
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
    let case = IntegrationTestContext::with_file(
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
            package = package,
            parametrize_function = &get_parametrize_function(package),
        ),
    );

    allow_duplicates!(assert_cmd_snapshot!(case.command(), @r"
    allow_duplicates!(success: true
    allow_duplicates!(exit_code: 0
    allow_duplicates!(----- stdout -----
    allow_duplicates!(running 3 tests
    allow_duplicates!(test test_parametrize::test_parametrize [a=1, b=2, expected=3] ... ok
    allow_duplicates!(test test_parametrize::test_parametrize [a=2, b=3, expected=5] ... ok
    allow_duplicates!(test test_parametrize::test_parametrize [a=3, b=4, expected=7] ... ok

    allow_duplicates!(test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    allow_duplicates!(----- stderr -----
    allow_duplicates!("));
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_stdout_is_captured_and_displayed() {
    let case = IntegrationTestContext::with_file(
        "test_std_out_redirected.py",
        r"
        def test_std_out_redirected():
            print('Hello, world!')
        ",
    );

    assert_cmd_snapshot!(case.command().args(["-s"]), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    running 1 test
    test test_std_out_redirected::test_std_out_redirected ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]
    Hello, world!

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_stdout_is_captured_and_displayed_with_args() {
    let case = IntegrationTestContext::with_file(
        "test_std_out_redirected.py",
        r"
        def test_std_out_redirected():
            print('Hello, world!')
        ",
    );

    assert_cmd_snapshot!(case.command(), @r"
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
    let case = IntegrationTestContext::with_file(
        "test_multiple_fixtures_not_found.py",
        "def test_multiple_fixtures_not_found(a, b, c): ...",
    );

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    running 1 test
    test test_multiple_fixtures_not_found::test_multiple_fixtures_not_found ... FAILED

    error[fixtures-not-found]: Fixture(s) not found for test_multiple_fixtures_not_found::test_multiple_fixtures_not_found
     --> test_multiple_fixtures_not_found::test_multiple_fixtures_not_found at <temp_dir>/test_multiple_fixtures_not_found.py:1
    error[fixture-not-found]: fixture 'a' not found
    error[fixture-not-found]: fixture 'b' not found
    error[fixture-not-found]: fixture 'c' not found

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_functionality(#[values("pytest", "karva")] framework: &str) {
    let decorator = if framework == "pytest" {
        "pytest.mark.skip"
    } else {
        "karva.tags.skip"
    };

    let test_code = format!(
        r"
import {framework}

@{decorator}('This test is skipped with decorator')
def test_1():
    assert False

",
    );

    let case = IntegrationTestContext::with_file("test_skip.py", &test_code);

    allow_duplicates!(assert_cmd_snapshot!(case.command(), @r"
    allow_duplicates!(success: true
    allow_duplicates!(exit_code: 0
    allow_duplicates!(----- stdout -----
    allow_duplicates!(running 1 test
    allow_duplicates!(test test_skip::test_1 ... skipped: This test is skipped with decorator

    allow_duplicates!(test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

    allow_duplicates!(----- stderr -----
    allow_duplicates!("));
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_text_file_in_directory() {
    let case = IntegrationTestContext::with_files([
        ("test_sample.py", "def test_sample(): assert True"),
        ("random.txt", "pass"),
    ]);

    assert_cmd_snapshot!(case.command(), @r"
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
    let case = IntegrationTestContext::with_file("random.txt", "pass");

    assert_cmd_snapshot!(
        case.command().args(["random.txt"]),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
    running 0 tests

    error[invalid-path]: Path `<temp_dir>/random.txt` has a wrong file extension

    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_quiet_output() {
    let case = IntegrationTestContext::with_file(
        "test.py",
        "
        def test_quiet_output():
            assert True
        ",
    );

    assert_cmd_snapshot!(case.command().args(["-q"]), @r"
    success: true
    exit_code: 0
    ----- stdout -----

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
