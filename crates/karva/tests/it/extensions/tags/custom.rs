use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
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

#[test]
fn test_tag_filter_include() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
def test_slow():
    assert True

def test_fast():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("slow"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_slow ... ok
    test test::test_fast ... skipped

    test result: ok. 1 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_tag_filter_exclude() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
def test_slow():
    assert True

def test_fast():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("not slow"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_slow ... skipped
    test test::test_fast ... ok

    test result: ok. 1 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_tag_filter_and_expression() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.integration
def test_slow_integration():
    assert True

@karva.tags.slow
def test_slow_only():
    assert True

@karva.tags.integration
def test_integration_only():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("slow and integration"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_slow_integration ... ok
    test test::test_slow_only ... skipped
    test test::test_integration_only ... skipped

    test result: ok. 1 passed; 0 failed; 2 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_tag_filter_or_expression() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
def test_slow():
    assert True

@karva.tags.integration
def test_integration():
    assert True

def test_untagged():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("slow or integration"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_slow ... ok
    test test::test_integration ... ok
    test test::test_untagged ... skipped

    test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_tag_filter_multiple_flags() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.integration
def test_both():
    assert True

@karva.tags.slow
def test_slow_only():
    assert True

@karva.tags.integration
def test_integration_only():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("slow").arg("-t").arg("integration"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_both ... ok
    test test::test_slow_only ... ok
    test test::test_integration_only ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_tag_filter_no_matches() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_untagged():
    assert True

@karva.tags.fast
def test_fast():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("slow"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_untagged ... skipped
    test test::test_fast ... skipped

    test result: ok. 0 passed; 0 failed; 2 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_tag_filter_with_parametrize() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.parametrize('x', [1, 2])
def test_param(x):
    assert x > 0

def test_untagged():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("slow"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_param(x=1) ... ok
    test test::test_param(x=2) ... ok
    test test::test_untagged ... skipped

    test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_tag_filter_not_with_and() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.flaky
def test_slow_flaky():
    assert True

@karva.tags.slow
def test_slow_stable():
    assert True

def test_untagged():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("slow and not flaky"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_slow_flaky ... skipped
    test test::test_slow_stable ... ok
    test test::test_untagged ... skipped

    test result: ok. 1 passed; 0 failed; 2 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_tag_filter_parenthesized_expression() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.linux
def test_slow_linux():
    assert True

@karva.tags.fast
@karva.tags.linux
def test_fast_linux():
    assert True

@karva.tags.slow
def test_slow_only():
    assert True

@karva.tags.linux
def test_linux_only():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("(slow or fast) and linux"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_slow_linux ... ok
    test test::test_fast_linux ... ok
    test test::test_slow_only ... skipped
    test test::test_linux_only ... skipped

    test result: ok. 2 passed; 0 failed; 2 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_tag_filter_with_skip() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.skip
def test_slow_skipped():
    assert False

@karva.tags.slow
def test_slow_runs():
    assert True

def test_untagged():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("slow"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_slow_skipped ... skipped
    test test::test_slow_runs ... ok
    test test::test_untagged ... skipped

    test result: ok. 1 passed; 0 failed; 2 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
