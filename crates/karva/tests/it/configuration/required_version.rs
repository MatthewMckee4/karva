use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn required_version_satisfied_in_karva_toml() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
required-version = ">=0.0.1-alpha.1"
"#,
        ),
        ("test.py", "def test_pass(): pass\n"),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_pass
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn required_version_unsatisfied_in_karva_toml_fails_before_running() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
required-version = ">=999.0.0"
"#,
        ),
        ("test.py", "def test_pass(): pass\n"),
    ]);

    assert_cmd_snapshot!(context.command(), @r#"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: <temp_dir>/karva.toml has an incompatible `required-version`
      Cause: the installed karva [VERSION] does not satisfy `required-version = ">=999.0.0"`
    "#);
}

#[test]
fn required_version_unsatisfied_in_pyproject_toml_fails() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r#"
[project]
name = "test-project"

[tool.karva]
required-version = ">=999.0.0"
"#,
        ),
        ("test.py", "def test_pass(): pass\n"),
    ]);

    assert_cmd_snapshot!(context.command(), @r#"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: <temp_dir>/pyproject.toml has an incompatible `required-version`
      Cause: the installed karva [VERSION] does not satisfy `required-version = ">=999.0.0"`
    "#);
}

#[test]
fn required_version_invalid_specifier_is_a_parse_error() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
required-version = "not a version"
"#,
        ),
        ("test.py", "def test_pass(): pass\n"),
    ]);

    assert_cmd_snapshot!(context.command(), @r#"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: <temp_dir>/karva.toml is not a valid `karva.toml`: TOML parse error at line 2, column 20
      |
    2 | required-version = "not a version"
      |                    ^^^^^^^^^^^^^^^
    unexpected character 'n' while parsing major version number

      Cause: TOML parse error at line 2, column 20
      |
    2 | required-version = "not a version"
      |                    ^^^^^^^^^^^^^^^
    unexpected character 'n' while parsing major version number
    "#);
}
