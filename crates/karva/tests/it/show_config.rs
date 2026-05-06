use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn show_config_default_profile() {
    let context = TestContext::default();

    assert_cmd_snapshot!(context.show_config(), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    [src]
    respect-ignore-files = true
    include = []

    [terminal]
    output-format = "full"
    show-python-output = false
    status-level = "pass"
    final-status-level = "pass"

    [test]
    test-function-prefix = "test"
    try-import-fixtures = false
    retry = 0
    no-tests = "auto"

    [coverage]
    sources = []
    report = "term"

    ----- stderr -----
    "#);
}

#[test]
fn show_config_resolves_pyproject_options() {
    let context = TestContext::with_file(
        "pyproject.toml",
        r#"
[tool.karva.profile.default.test]
test-function-prefix = "check"
fail-fast = true

[tool.karva.profile.default.terminal]
output-format = "concise"
"#,
    );

    assert_cmd_snapshot!(context.show_config(), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    [src]
    respect-ignore-files = true
    include = []

    [terminal]
    output-format = "concise"
    show-python-output = false
    status-level = "pass"
    final-status-level = "pass"

    [test]
    test-function-prefix = "check"
    max-fail = 1
    try-import-fixtures = false
    retry = 0
    no-tests = "auto"

    [coverage]
    sources = []
    report = "term"

    ----- stderr -----
    "#);
}

#[test]
fn show_config_named_profile_layers_over_default() {
    let context = TestContext::with_file(
        "karva.toml",
        r#"
[profile.default.test]
test-function-prefix = "check"

[profile.ci.test]
retry = 3

[profile.ci.terminal]
output-format = "concise"
"#,
    );

    assert_cmd_snapshot!(context.show_config().args(["--profile", "ci"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    [src]
    respect-ignore-files = true
    include = []

    [terminal]
    output-format = "concise"
    show-python-output = false
    status-level = "pass"
    final-status-level = "pass"

    [test]
    test-function-prefix = "check"
    try-import-fixtures = false
    retry = 3
    no-tests = "auto"

    [coverage]
    sources = []
    report = "term"

    ----- stderr -----
    "#);
}

#[test]
fn show_config_emits_set_timeouts_and_coverage() {
    let context = TestContext::with_file(
        "karva.toml",
        r#"
[profile.default.test]
slow-timeout = 0.5
timeout = 120

[profile.default.coverage]
sources = ["src"]
report = "term-missing"
fail-under = 90
"#,
    );

    assert_cmd_snapshot!(context.show_config(), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    [src]
    respect-ignore-files = true
    include = []

    [terminal]
    output-format = "full"
    show-python-output = false
    status-level = "pass"
    final-status-level = "pass"

    [test]
    test-function-prefix = "test"
    try-import-fixtures = false
    retry = 0
    no-tests = "auto"
    slow-timeout = 0.5
    timeout = 120.0

    [coverage]
    sources = ["src"]
    report = "term-missing"
    fail-under = 90.0

    ----- stderr -----
    "#);
}

#[test]
fn show_config_unknown_profile_errors() {
    let context = TestContext::with_file(
        "karva.toml",
        r"
[profile.ci.test]
retry = 3
",
    );

    assert_cmd_snapshot!(context.show_config().args(["--profile", "bogus"]), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: profile `bogus` is not defined in configuration (available: ci, default)
    ");
}
