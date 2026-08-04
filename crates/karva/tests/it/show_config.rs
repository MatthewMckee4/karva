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
    shuffle = false
    flaky-result = "pass"
    no-tests = "auto"

    [coverage]
    data-file = ".karva/coverage/data.json"
    path-aliases = []
    sources = []
    include = []
    omit = []
    exclude-lines = []
    partial-branches = []
    contexts = []
    precision = 0
    append = false
    report = "term"

    [junit]
    report-name = "karva-tests"
    store-failure-output = true
    flaky-fail-status = "failure"

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
    shuffle = false
    flaky-result = "pass"
    no-tests = "auto"

    [coverage]
    data-file = ".karva/coverage/data.json"
    path-aliases = []
    sources = []
    include = []
    omit = []
    exclude-lines = []
    partial-branches = []
    contexts = []
    precision = 0
    append = false
    report = "term"

    [junit]
    report-name = "karva-tests"
    store-failure-output = true
    flaky-fail-status = "failure"

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
    shuffle = false
    flaky-result = "pass"
    no-tests = "auto"

    [coverage]
    data-file = ".karva/coverage/data.json"
    path-aliases = []
    sources = []
    include = []
    omit = []
    exclude-lines = []
    partial-branches = []
    contexts = []
    precision = 0
    append = false
    report = "term"

    [junit]
    report-name = "karva-tests"
    store-failure-output = true
    flaky-fail-status = "failure"

    ----- stderr -----
    "#);
}

#[test]
fn show_config_emits_resolved_environment() {
    let context = TestContext::with_file(
        "karva.toml",
        r#"
[profile.default.env]
APP_ENV = "test"
CACHE_DIR = { value = ".cache/tests", preserve = true }
LIVE_API_TOKEN = { unset = true }

[profile.ci.env]
APP_ENV = "ci"
"#,
    );

    assert_cmd_snapshot!(context.show_config().args(["--profile", "ci"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    [env]
    APP_ENV = "ci"

    [env.CACHE_DIR]
    value = ".cache/tests"
    preserve = true

    [env.LIVE_API_TOKEN]
    unset = true

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
    shuffle = false
    flaky-result = "pass"
    no-tests = "auto"

    [coverage]
    data-file = ".karva/coverage/data.json"
    path-aliases = []
    sources = []
    include = []
    omit = []
    exclude-lines = []
    partial-branches = []
    contexts = []
    precision = 0
    append = false
    report = "term"

    [junit]
    report-name = "karva-tests"
    store-failure-output = true
    flaky-fail-status = "failure"

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
termination-grace-period = 2

[profile.default.coverage]
sources = ["src"]
include = ["src/app/*"]
omit = ["**/generated.py"]
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
    shuffle = false
    flaky-result = "pass"
    no-tests = "auto"
    slow-timeout = 0.5
    timeout = 120.0
    termination-grace-period = 2.0

    [coverage]
    data-file = ".karva/coverage/data.json"
    path-aliases = []
    sources = ["src"]
    include = ["src/app/*"]
    omit = ["**/generated.py"]
    exclude-lines = []
    partial-branches = []
    contexts = []
    precision = 0
    append = false
    report = "term-missing"
    fail-under = 90.0

    [junit]
    report-name = "karva-tests"
    store-failure-output = true
    flaky-fail-status = "failure"

    ----- stderr -----
    "#);
}

#[test]
fn show_config_emits_per_test_overrides() {
    let context = TestContext::with_file(
        "karva.toml",
        r#"
[[profile.default.overrides]]
filter = "tag(network)"
retries = 2

[[profile.default.overrides]]
filter = "tag(slow)"
timeout = 30
slow-timeout = 0.5
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
    shuffle = false
    flaky-result = "pass"
    no-tests = "auto"

    [coverage]
    data-file = ".karva/coverage/data.json"
    path-aliases = []
    sources = []
    include = []
    omit = []
    exclude-lines = []
    partial-branches = []
    contexts = []
    precision = 0
    append = false
    report = "term"

    [junit]
    report-name = "karva-tests"
    store-failure-output = true
    flaky-fail-status = "failure"

    [[overrides]]
    filter = "tag(network)"
    retries = 2

    [[overrides]]
    filter = "tag(slow)"
    timeout = 30.0
    slow-timeout = 0.5

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

    assert_cmd_snapshot!(context.show_config().args(["--profile", "bogus"]), @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: profile `bogus` is not defined in configuration (available: ci, default)
    ");
}
