use insta_cmd::assert_cmd_snapshot;
use karva_test::IntegrationTestContext;

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_single_file() {
    let context = IntegrationTestContext::with_files([
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
    let context = IntegrationTestContext::with_file("test.py", "");

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
    let context = IntegrationTestContext::new();

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
    let context = IntegrationTestContext::with_file(
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
    let context = IntegrationTestContext::with_file(
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
    let context = IntegrationTestContext::with_file(
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
