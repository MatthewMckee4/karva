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

    assert_cmd_snapshot!(context.command(), @"
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
fn test_custom_tag_with_args() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.tags.benchmark(30, "seconds")
def test_1():
    assert True
        "#,
    );

    assert_cmd_snapshot!(context.command(), @"
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

    assert_cmd_snapshot!(context.command(), @"
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

    assert_cmd_snapshot!(context.command(), @"
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

    assert_cmd_snapshot!(context.command(), @"
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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_runs
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn strict_tags_accept_registered_and_builtin_tags() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[tags]
integration = "Uses an external service"

[profile.default.test]
strict-tags = true
"#,
        ),
        (
            "test.py",
            r#"
import karva

@karva.fixture
def setup():
    return None

@karva.tags.integration
@karva.tags.use_fixtures("setup")
@karva.tags.parametrize("value", [1])
@karva.tags.timeout(2)
@karva.tags.skip(False)
@karva.tags.expect_fail(False)
def test_registered(value):
    assert value == 1
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command().args([
        "-E",
        "tag(integration) & tag(use_fixtures) & tag(parametrize) & tag(timeout) & tag(skip) & tag(expect_fail)",
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_registered(value=1)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn strict_tags_reject_unknown_tag_with_suggestion() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[tags]
integration = "Uses an external service"

[profile.default.test]
strict-tags = true
"#,
        ),
        (
            "test.py",
            r"
import karva

@karva.tags.integraiton
def test_typo():
    pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[unknown-tag]: Tag `integraiton` is not registered
     --> test.py:4:13
      |
    4 | @karva.tags.integraiton
      |             ^^^^^^^^^^^ unregistered tag
      |
    info: Did you mean `integration`?

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn strict_tags_validate_pytest_marks_fixture_tags_and_parameter_tags() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[profile.default.test]
strict-tags = true
",
        ),
        (
            "test.py",
            r#"
import karva
import pytest

@karva.tags.fixture_typo
@karva.fixture
def setup():
    return None

@pytest.mark.pytest_typo
@karva.tags.parametrize("value", [
    karva.param(1, tags=(karva.tags.parameter_typo,)),
])
def test_typos(value):
    pass
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[unknown-tag]: Tag `fixture_typo` is not registered
     --> test.py:5:13
      |
    5 | @karva.tags.fixture_typo
      |             ^^^^^^^^^^^^ unregistered tag
      |
    info: Register `fixture_typo` in the project-wide `[tags]` table.

    error[unknown-tag]: Tag `pytest_typo` is not registered
      --> test.py:10:14
       |
    10 | @pytest.mark.pytest_typo
       |              ^^^^^^^^^^^ unregistered tag
       |
    info: Register `pytest_typo` in the project-wide `[tags]` table.

    error[unknown-tag]: Tag `parameter_typo` is not registered
      --> test.py:12:37
       |
    12 |     karva.param(1, tags=(karva.tags.parameter_typo,)),
       |                                     ^^^^^^^^^^^^^^ unregistered tag
       |
    info: Register `parameter_typo` in the project-wide `[tags]` table.

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn strict_tags_inherit_from_default_profile() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[tags]
integration = ""

[profile.default.test]
strict-tags = true

[profile.ci.test]
retry = 1
"#,
        ),
        (
            "test.py",
            r"
import karva

@karva.tags.integraiton
def test_typo():
    pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command().args(["--profile", "ci"]), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[unknown-tag]: Tag `integraiton` is not registered
     --> test.py:4:13
      |
    4 | @karva.tags.integraiton
      |             ^^^^^^^^^^^ unregistered tag
      |
    info: Did you mean `integration`?

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn strict_tags_validate_aliases_module_marks_and_parameter_tags() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[tags]
database = ""
integration = ""

[profile.default.test]
strict-tags = true
"#,
        ),
        (
            "test.py",
            r#"
import karva as k
import pytest as pt

pytestmark = pt.mark.daatbase

@k.tags.parametrize("value", [
    k.param(1, tags=(k.tags.integraiton,)),
])
def test_aliases(value):
    pass
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[unknown-tag]: Tag `daatbase` is not registered
     --> test.py:5:22
      |
    5 | pytestmark = pt.mark.daatbase
      |                      ^^^^^^^^ unregistered tag
      |
    info: Did you mean `database`?

    error[unknown-tag]: Tag `integraiton` is not registered
     --> test.py:8:29
      |
    8 |     k.param(1, tags=(k.tags.integraiton,)),
      |                             ^^^^^^^^^^^ unregistered tag
      |
    info: Did you mean `integration`?

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}
