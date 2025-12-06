use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_src_respect_ignore_files_false() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[src]
respect-ignore-files = false
",
        ),
        (".gitignore", "ignored_test.py"),
        (
            "ignored_test.py",
            r"
def test_ignored(): pass
",
        ),
        (
            "test_main.py",
            r"
def test_main(): pass
",
        ),
    ]);

    // With respect-ignore-files = false, the ignored file should be included
    assert_cmd_snapshot!(context.command().arg("-q"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_src_respect_ignore_files_true() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[src]
respect-ignore-files = true
",
        ),
        (".gitignore", "ignored_test.py"),
        (
            "ignored_test.py",
            r"
def test_ignored(): pass
",
        ),
        (
            "test_main.py",
            r"
def test_main(): pass
",
        ),
    ]);

    // With respect-ignore-files = true, the ignored file should be excluded
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_main::test_main ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_src_include_paths() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[src]
include = ["src", "tests"]
"#,
        ),
        (
            "src/test_src.py",
            r"
def test_in_src(): pass
",
        ),
        (
            "tests/test_tests.py",
            r"
def test_in_tests(): pass
",
        ),
        (
            "other/test_other.py",
            r"
def test_in_other(): pass
",
        ),
    ]);

    // Only files in 'src' and 'tests' should be included
    assert_cmd_snapshot!(context.command().arg("-q"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_src_include_single_file() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[src]
include = ["test_specific.py"]
"#,
        ),
        (
            "test_specific.py",
            r"
def test_specific(): pass
",
        ),
        (
            "test_other.py",
            r"
def test_other(): pass
",
        ),
    ]);

    // Only the specifically included file should be tested
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_specific::test_specific ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_terminal_output_format_concise() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[terminal]
output-format = "concise"
"#,
        ),
        (
            "test.py",
            r"
def test_example(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_example ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_terminal_output_format_full() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[terminal]
output-format = "full"
"#,
        ),
        (
            "test.py",
            r"
def test_example(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_example ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_terminal_show_python_output_false() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[terminal]
show-python-output = false
",
        ),
        (
            "test.py",
            r#"
def test_with_print():
    print("This should not be visible")
    pass
"#,
        ),
    ]);

    // Python output should be hidden
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_with_print ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_terminal_show_python_output_true() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[terminal]
show-python-output = true
",
        ),
        (
            "test.py",
            r#"
def test_with_print():
    print("This should be visible")
    pass
"#,
        ),
    ]);

    // Python output should be visible
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_with_print ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]
    This should be visible

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_test_function_prefix_custom() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[test]
test-function-prefix = "check"
"#,
        ),
        (
            "test.py",
            r"
def check_example(): pass
def test_should_not_run(): pass
",
        ),
    ]);

    // Only functions with 'check' prefix should run
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::check_example ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_test_function_prefix_default() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[test]
test-function-prefix = "test"
"#,
        ),
        (
            "test.py",
            r"
def test_example(): pass
def check_should_not_run(): pass
",
        ),
    ]);

    // Only functions with 'test' prefix should run
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_example ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fail_fast_true() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[test]
fail-fast = true
",
        ),
        (
            "test.py",
            r"
def test_first():
    assert False

def test_second():
    pass
",
        ),
    ]);

    // Should stop after first failure
    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_first ... FAILED

    diagnostics:

    error[test-failure]: Test `test_first` failed
     --> test.py:2:5
      |
    2 | def test_first():
      |     ^^^^^^^^^^
    3 |     assert False
      |
    info: Test failed here
     --> test.py:3:5
      |
    2 | def test_first():
    3 |     assert False
      |     ^^^^^^^^^^^^
    4 |
    5 | def test_second():
      |

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fail_fast_false() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[test]
fail-fast = false
",
        ),
        (
            "test.py",
            r"
def test_first():
    assert False

def test_second():
    pass

def test_third():
    assert False
",
        ),
    ]);

    // Should run all tests even after failures
    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_first ... FAILED
    test test::test_second ... ok
    test test::test_third ... FAILED

    diagnostics:

    error[test-failure]: Test `test_first` failed
     --> test.py:2:5
      |
    2 | def test_first():
      |     ^^^^^^^^^^
    3 |     assert False
      |
    info: Test failed here
     --> test.py:3:5
      |
    2 | def test_first():
    3 |     assert False
      |     ^^^^^^^^^^^^
    4 |
    5 | def test_second():
      |

    error[test-failure]: Test `test_third` failed
     --> test.py:8:5
      |
    6 |     pass
    7 |
    8 | def test_third():
      |     ^^^^^^^^^^
    9 |     assert False
      |
    info: Test failed here
     --> test.py:9:5
      |
    8 | def test_third():
    9 |     assert False
      |     ^^^^^^^^^^^^
      |

    test result: FAILED. 1 passed; 2 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_combined_all_options() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[src]
respect-ignore-files = false
include = ["tests"]

[terminal]
output-format = "concise"
show-python-output = false

[test]
test-function-prefix = "check"
fail-fast = true
"#,
        ),
        (
            "tests/test.py",
            r#"
def check_example():
    print("Test output")
    pass
"#,
        ),
        (
            "other/test.py",
            r"
def check_other(): pass
",
        ),
    ]);

    // Should respect all configuration options
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test tests.test::check_example ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_combined_src_and_test_options() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[src]
include = ["src"]

[test]
test-function-prefix = "verify"
"#,
        ),
        (
            "src/module.py",
            r"
def verify_in_src(): pass
def test_should_not_run(): pass
",
        ),
        (
            "tests/test.py",
            r"
def verify_in_tests(): pass
",
        ),
    ]);

    // Should only run verify_* functions in src directory
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test src.module::verify_in_src ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_pyproject_src_options() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r#"
[project]
name = "test-project"

[tool.karva.src]
respect-ignore-files = false
include = ["src"]
"#,
        ),
        (".gitignore", "src/ignored.py"),
        (
            "src/ignored.py",
            r"
def test_ignored(): pass
",
        ),
        (
            "src/test.py",
            r"
def test_main(): pass
",
        ),
        (
            "other/test.py",
            r"
def test_other(): pass
",
        ),
    ]);

    // Should respect pyproject.toml configuration
    assert_cmd_snapshot!(context.command().arg("-q"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_pyproject_terminal_options() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r#"
[project]
name = "test-project"

[tool.karva.terminal]
output-format = "concise"
show-python-output = false
"#,
        ),
        (
            "test.py",
            r#"
def test_example():
    print("Hidden output")
    pass
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_example ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_pyproject_test_options() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r#"
[project]
name = "test-project"

[tool.karva.test]
test-function-prefix = "spec"
fail-fast = true
"#,
        ),
        (
            "test.py",
            r"
def spec_example(): pass
def test_should_not_run(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::spec_example ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_pyproject_all_options() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r#"
[project]
name = "test-project"

[tool.karva.src]
respect-ignore-files = false
include = ["tests"]

[tool.karva.terminal]
output-format = "full"
show-python-output = true

[tool.karva.test]
test-function-prefix = "it"
fail-fast = false
"#,
        ),
        (
            "tests/spec.py",
            r#"
def it_works():
    print("Output visible")
    pass

def it_also_works():
    pass
"#,
        ),
        (
            "src/test.py",
            r"
def it_should_not_run(): pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test tests.spec::it_works ... ok
    test tests.spec::it_also_works ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]
    Output visible

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_karva_toml_takes_precedence_over_pyproject() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[test]
test-function-prefix = "karva"
"#,
        ),
        (
            "pyproject.toml",
            r#"
[project]
name = "test-project"

[tool.karva.test]
test-function-prefix = "pyproject"
"#,
        ),
        (
            "test.py",
            r"
def karva_test(): pass
def pyproject_test(): pass
",
        ),
    ]);

    // karva.toml should take precedence, so only karva_* functions run
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::karva_test ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    WARN Ignoring the `tool.ty` section in `<temp_dir>/pyproject.toml` because `<temp_dir>/karva.toml` takes precedence.
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_empty_config() {
    let context = TestContext::with_files([
        ("karva.toml", ""),
        (
            "test.py",
            r"
def test_default(): pass
",
        ),
    ]);

    // Should use default settings
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_default ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_partial_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[test]
test-function-prefix = "custom"
"#,
        ),
        (
            "test.py",
            r"
def custom_test(): pass
",
        ),
    ]);

    // Should use custom prefix but default for other options
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::custom_test ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_cli_test_prefix_overrides_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[test]
test-function-prefix = "config"
"#,
        ),
        (
            "test.py",
            r"
def config_should_not_run(): pass
def cli_should_run(): pass
",
        ),
    ]);

    // CLI argument --test-prefix should override config file
    assert_cmd_snapshot!(context.command().arg("--test-prefix").arg("cli"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::cli_should_run ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_cli_output_format_overrides_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[terminal]
output-format = "full"
"#,
        ),
        (
            "test.py",
            r"
def test_example(): pass
",
        ),
    ]);

    // CLI argument --output-format should override config file
    assert_cmd_snapshot!(context.command().arg("--output-format").arg("concise"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_example ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_cli_show_output_overrides_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[terminal]
show-python-output = false
",
        ),
        (
            "test.py",
            r#"
def test_with_print():
    print("This should be visible with -s flag")
    pass
"#,
        ),
    ]);

    // CLI argument -s should override config file and show output
    assert_cmd_snapshot!(context.command().arg("-s"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_with_print ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]
    This should be visible with -s flag

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_cli_no_ignore_overrides_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[src]
respect-ignore-files = true
",
        ),
        (".gitignore", "ignored_test.py"),
        (
            "ignored_test.py",
            r"
def test_ignored(): pass
",
        ),
        (
            "test_main.py",
            r"
def test_main(): pass
",
        ),
    ]);

    // CLI argument --no-ignore should override config and include ignored files
    assert_cmd_snapshot!(context.command().arg("--no-ignore").arg("-q"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_cli_fail_fast_overrides_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[test]
fail-fast = false
",
        ),
        (
            "test.py",
            r"
def test_first():
    assert False

def test_second():
    pass
",
        ),
    ]);

    // CLI argument --fail-fast should override config and stop after first failure
    assert_cmd_snapshot!(context.command().arg("--fail-fast"), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_first ... FAILED

    diagnostics:

    error[test-failure]: Test `test_first` failed
     --> test.py:2:5
      |
    2 | def test_first():
      |     ^^^^^^^^^^
    3 |     assert False
      |
    info: Test failed here
     --> test.py:3:5
      |
    2 | def test_first():
    3 |     assert False
      |     ^^^^^^^^^^^^
    4 |
    5 | def test_second():
      |

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_cli_paths_override_config_include() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[src]
include = ["config_dir"]
"#,
        ),
        (
            "config_dir/test_config.py",
            r"
def test_from_config(): pass
",
        ),
        (
            "cli_dir/test_cli.py",
            r"
def test_from_cli(): pass
",
        ),
    ]);

    // CLI path argument should add to config include
    assert_cmd_snapshot!(context.command().arg("cli_dir").arg("-q"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_cli_multiple_arguments_override_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[src]
respect-ignore-files = true
include = ["config_dir"]

[terminal]
output-format = "full"
show-python-output = false

[test]
test-function-prefix = "config"
fail-fast = false
"#,
        ),
        (".gitignore", "custom_dir/ignored.py"),
        (
            "custom_dir/ignored.py",
            r#"
def custom_test():
    print("CLI output visible")
    pass
"#,
        ),
        (
            "config_dir/test.py",
            r"
def config_should_not_run(): pass
",
        ),
    ]);

    // Multiple CLI arguments should all override their respective config values
    assert_cmd_snapshot!(
        context
            .command()
            .arg("--test-prefix")
            .arg("custom")
            .arg("--output-format")
            .arg("concise")
            .arg("-s")
            .arg("--no-ignore")
            .arg("custom_dir"),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test custom_dir.ignored::custom_test ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]
    CLI output visible

    ----- stderr -----
    "
    );
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_cli_overrides_pyproject_toml() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r#"
[project]
name = "test-project"

[tool.karva.test]
test-function-prefix = "pyproject"
fail-fast = true

[tool.karva.terminal]
show-python-output = false
"#,
        ),
        (
            "test.py",
            r#"
def pyproject_should_not_run(): pass
def cli_should_run():
    print("Output from CLI override")
    pass
"#,
        ),
    ]);

    // CLI arguments should override pyproject.toml configuration
    assert_cmd_snapshot!(
        context
            .command()
            .arg("--test-prefix")
            .arg("cli")
            .arg("-s")
            .arg("--fail-fast"),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::cli_should_run ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]
    Output from CLI override

    ----- stderr -----
    "
    );
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_cli_overrides_both_config_files() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[test]
test-function-prefix = "karva"
"#,
        ),
        (
            "pyproject.toml",
            r#"
[project]
name = "test-project"

[tool.karva.test]
test-function-prefix = "pyproject"
"#,
        ),
        (
            "test.py",
            r"
def karva_should_not_run(): pass
def pyproject_should_not_run(): pass
def cli_should_run(): pass
",
        ),
    ]);

    // CLI argument should override both karva.toml and pyproject.toml
    assert_cmd_snapshot!(context.command().arg("--test-prefix").arg("cli"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::cli_should_run ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    WARN Ignoring the `tool.ty` section in `<temp_dir>/pyproject.toml` because `<temp_dir>/karva.toml` takes precedence.
    ");
}
