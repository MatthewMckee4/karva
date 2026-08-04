use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn profile_environment_is_applied_before_test_module_import() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.env]
APP_ENV = "test"
CACHE_DIR = { value = ".cache/tests", preserve = true }
PROFILE_ENV_TEST_FALLBACK = { value = "configured", preserve = true }
LIVE_API_TOKEN = { unset = true }
PROFILE_VALUE = "default"
PYTHONUNBUFFERED = "configured"

[profile.ci.env]
PROFILE_VALUE = "ci"
"#,
        ),
        (
            "test_environment.py",
            r#"
import os

assert os.environ["APP_ENV"] == "test"
assert os.environ["CACHE_DIR"] == "from-parent"
assert os.environ["PROFILE_ENV_TEST_FALLBACK"] == "configured"
assert "LIVE_API_TOKEN" not in os.environ
assert os.environ["PROFILE_VALUE"] == "ci"
assert os.environ["PYTHONUNBUFFERED"] == "configured"

def test_environment(): pass
"#,
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command()
            .args(["--profile", "ci"])
            .env("CACHE_DIR", "from-parent")
            .env("LIVE_API_TOKEN", "secret"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_environment::test_environment
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn pyproject_environment_is_applied_before_conftest_import() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r#"
[tool.karva.profile.default.env]
CONFTEST_ENV = "ready"
"#,
        ),
        (
            "conftest.py",
            r#"
import os

assert os.environ["CONFTEST_ENV"] == "ready"
"#,
        ),
        ("test_environment.py", "def test_environment(): pass"),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_environment::test_environment
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn environment_is_applied_to_every_worker() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.env]
WORKER_ENV = "ready"
"#,
        ),
        (
            "test_first.py",
            r#"
import os

def test_first(): assert os.environ["WORKER_ENV"] == "ready"
"#,
        ),
        (
            "test_second.py",
            r#"
import os

def test_second(): assert os.environ["WORKER_ENV"] == "ready"
"#,
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command()
            .args(["--num-workers", "2", "--status-level=none"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn named_profile_can_unset_inherited_value() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.env]
INHERITED_ENV = "default"

[profile.ci.env]
INHERITED_ENV = { unset = true }
"#,
        ),
        (
            "test_environment.py",
            r#"
import os

def test_environment(): assert "INHERITED_ENV" not in os.environ
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command().args(["--profile", "ci"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_environment::test_environment
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn configured_values_are_absent_from_result_report() {
    const SECRET: &str = "super-secret-profile-value";
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.env]
SECRET_VALUE = "super-secret-profile-value"
"#,
        ),
        ("test_environment.py", "def test_environment(): pass"),
    ]);

    assert_cmd_snapshot!(
        context
            .command()
            .args(["--status-level=none", "--result-output=results.json"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
    assert!(!context.read_file("results.json").contains(SECRET));
}

#[test]
fn karva_environment_variables_are_reserved() {
    let context = TestContext::with_file(
        "karva.toml",
        r#"
[profile.default.env]
KARVA_RUN_ID = "mine"
"#,
    );

    assert_cmd_snapshot!(context.command(), @r#"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: <temp_dir>/karva.toml is not a valid `karva.toml`
      Cause: TOML parse error at line 3, column 1
      |
    3 | KARVA_RUN_ID = "mine"
      | ^^^^^^^^^^^^
    environment variable `KARVA_RUN_ID` is reserved by Karva
    "#);
}

#[test]
fn environment_operation_requires_supported_shape() {
    let context = TestContext::with_file(
        "karva.toml",
        r#"
[profile.default.env]
APP_ENV = { value = "test", unset = true }
"#,
    );

    assert_cmd_snapshot!(context.command(), @r#"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: <temp_dir>/karva.toml is not a valid `karva.toml`
      Cause: TOML parse error at line 3, column 11
      |
    3 | APP_ENV = { value = "test", unset = true }
      |           ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    expected exactly `{ value = "...", preserve = true }` or `{ unset = true }`
    "#);
}
