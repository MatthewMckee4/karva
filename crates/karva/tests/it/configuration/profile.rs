use insta_cmd::assert_cmd_snapshot;
use karva_static::EnvVars;

use crate::common::TestContext;

#[test]
fn profile_named_overrides_default_profile() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.test]
test-function-prefix = "test"

[profile.ci.test]
test-function-prefix = "check"
"#,
        ),
        (
            "test.py",
            r"
def check_example(): pass
def test_should_not_run_under_ci_profile(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command().args(["--profile", "ci"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::check_example
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn profile_short_flag_works() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.fast.test]
test-function-prefix = "check"
"#,
        ),
        (
            "test.py",
            r"
def check_quick(): pass
def test_normal(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command().args(["-P", "fast"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::check_quick
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn profile_default_overrides_apply_when_no_profile_selected() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.test]
test-function-prefix = "check"
"#,
        ),
        (
            "test.py",
            r"
def check_example(): pass
def test_not_run(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::check_example
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn profile_named_layers_on_top_of_default_overrides() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.test]
retry = 1

[profile.ci.test]
test-function-prefix = "check"
"#,
        ),
        (
            "test.py",
            r"
def check_example(): pass
def test_not_run(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command().args(["--profile", "ci"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::check_example
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn profile_unknown_errors_with_listing() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.ci.test]
retry = 1

[profile.fast.test]
test-function-prefix = "check"
"#,
        ),
        ("test.py", "def test_a(): pass"),
    ]);

    assert_cmd_snapshot!(context.command().args(["--profile", "missing"]), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: profile `missing` is not defined in configuration (available: ci, default, fast)
    ");
}

#[test]
fn profile_env_var_selects_profile() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.fast.test]
test-function-prefix = "check"
"#,
        ),
        (
            "test.py",
            r"
def check_example(): pass
def test_not_run(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command().env(EnvVars::KARVA_PROFILE, "fast"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::check_example
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn profile_cli_flag_takes_precedence_over_env_var() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.fast.test]
test-function-prefix = "check"

[profile.ci.test]
test-function-prefix = "verify"
"#,
        ),
        (
            "test.py",
            r"
def check_a(): pass
def verify_b(): pass
def test_not_run(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command()
            .env(EnvVars::KARVA_PROFILE, "fast")
            .args(["--profile", "ci"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::verify_b
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn profile_cli_options_override_resolved_profile() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.ci.test]
test-function-prefix = "check"
"#,
        ),
        (
            "test.py",
            r"
def check_a(): pass
def verify_b(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command()
            .args(["--profile", "ci", "--test-prefix", "verify"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::verify_b
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn profile_pyproject_toml() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r#"
[tool.karva.profile.ci.test]
test-function-prefix = "check"
"#,
        ),
        (
            "test.py",
            r"
def check_example(): pass
def test_not_run(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command().args(["--profile", "ci"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::check_example
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn top_level_option_groups_rejected() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[test]
test-function-prefix = "check"
"#,
        ),
        ("test.py", "def test_a(): pass"),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: <temp_dir>/karva.toml is not a valid `karva.toml`: TOML parse error at line 2, column 2
      |
    2 | [test]
      |  ^^^^
    unknown field `test`, expected `required-version` or `profile`

      Cause: TOML parse error at line 2, column 2
      |
    2 | [test]
      |  ^^^^
    unknown field `test`, expected `required-version` or `profile`
    ");
}

#[test]
fn profile_reserved_default_prefix_rejected() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[profile.default-ci.test]
retry = 1
",
        ),
        ("test.py", "def test_a(): pass"),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: <temp_dir>/karva.toml is not a valid `karva.toml`: invalid profile name `default-ci`: the `default-` prefix is reserved for built-in profiles
      Cause: invalid profile name `default-ci`: the `default-` prefix is reserved for built-in profiles
    ");
}
