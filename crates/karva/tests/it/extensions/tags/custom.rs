use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_custom_tag_basic() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
def test_1():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
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
fn test_custom_tag_with_args() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.timeout(30, "seconds")
def test_1():
    assert True
        "#,
    );

    assert_cmd_snapshot!(context.command(), @r"
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
fn test_custom_tag_with_kwargs() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.flaky(retries=3, delay=1.5)
def test_1():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
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
fn test_custom_tag_with_mixed_args_and_kwargs() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.marker("value1", 42, key="value2")
def test_1():
    assert True
        "#,
    );

    assert_cmd_snapshot!(context.command(), @r"
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
fn test_multiple_custom_tags() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.integration
@karva.tags.priority(1)
def test_1():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
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
fn test_custom_tags_combined_with_builtin_tags() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.skip
def test_skipped():
    assert False

@karva.tags.integration
def test_runs():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_skipped ... skipped
    test test::test_runs ... ok

    test result: ok. 1 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
