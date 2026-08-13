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
    strict-tags = false
    try-import-fixtures = false
    doctest-modules = false
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
