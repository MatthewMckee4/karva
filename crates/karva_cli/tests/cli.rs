use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;
use insta::internals::SettingsBindDropGuard;
use insta_cmd::{assert_cmd_snapshot, get_cargo_bin};
use tempfile::TempDir;

struct TestCase {
    _temp_dir: TempDir,
    _settings_scope: SettingsBindDropGuard,
    project_dir: PathBuf,
}

impl TestCase {
    fn new() -> anyhow::Result<Self> {
        let temp_dir = TempDir::new()?;

        // Canonicalize the tempdir path because macos uses symlinks for tempdirs
        // and that doesn't play well with our snapshot filtering.
        // Simplify with dunce because otherwise we get UNC paths on Windows.
        let project_dir = dunce::simplified(
            &temp_dir
                .path()
                .canonicalize()
                .context("Failed to canonicalize project path")?,
        )
        .to_path_buf();

        let mut settings = insta::Settings::clone_current();
        settings.add_filter(&tempdir_filter(&project_dir), "<temp_dir>/");
        settings.add_filter(r#"\\(\w\w|\s|\.|")"#, "/$1");

        let settings_scope = settings.bind_to_scope();

        Ok(Self {
            project_dir,
            _temp_dir: temp_dir,
            _settings_scope: settings_scope,
        })
    }

    fn with_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a str)>) -> anyhow::Result<Self> {
        let case = Self::new()?;
        case.write_files(files)?;
        Ok(case)
    }

    fn with_file(path: impl AsRef<Path>, content: &str) -> anyhow::Result<Self> {
        let case = Self::new()?;
        case.write_file(path, content)?;
        Ok(case)
    }

    fn write_files<'a>(
        &self,
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> anyhow::Result<()> {
        for (path, content) in files {
            self.write_file(path, content)?;
        }

        Ok(())
    }

    fn write_file(&self, path: impl AsRef<Path>, content: &str) -> anyhow::Result<()> {
        let path = path.as_ref();
        let path = self.project_dir.join(path);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create directory `{}`", parent.display()))?;
        }
        std::fs::write(&path, &*ruff_python_trivia::textwrap::dedent(content))
            .with_context(|| format!("Failed to write file `{path}`", path = path.display()))?;

        Ok(())
    }

    fn command(&self) -> Command {
        let mut command = Command::new(get_cargo_bin("karva"));
        command.current_dir(&self.project_dir).arg("test");
        command
    }
}

fn tempdir_filter(path: &Path) -> String {
    format!(r"{}\\?/?", regex::escape(path.to_str().unwrap()))
}

#[test]
fn test_one_test_passes() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_pass.py",
        r"
        def test_pass():
            assert True
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!

    ----- stderr -----
    ");

    Ok(())
}

#[test]
fn test_two_tests_pass() -> anyhow::Result<()> {
    let case = TestCase::with_files([
        (
            "test_pass.py",
            r"
        def test_pass():
            assert True

    ",
        ),
        (
            "test_pass2.py",
            r"
        def test_pass2():
            assert True
    ",
        ),
    ])?;

    assert_cmd_snapshot!(case.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!

    ----- stderr -----
    ");

    Ok(())
}

#[test]
fn test_one_test_fails() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_fail.py",
        r"
        def test_fail():
            assert False
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_fail.py
     | File "<temp_dir>/test_fail.py", line 3, in test_fail
     |   assert False
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_multiple_tests_fail() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_fail2.py",
        r"
        def test_fail2():
            assert 1 == 2
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_fail2.py
     | File "<temp_dir>/test_fail2.py", line 3, in test_fail2
     |   assert 1 == 2
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_mixed_pass_and_fail() -> anyhow::Result<()> {
    let case = TestCase::with_files([
        (
            "test_pass.py",
            r"
        def test_pass():
            assert True
    ",
        ),
        (
            "test_fail.py",
            r"
        def test_fail():
            assert False
    ",
        ),
    ])?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_fail.py
     | File "<temp_dir>/test_fail.py", line 3, in test_fail
     |   assert False

    Passed tests: 1
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_assertion_with_message() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_fail_with_msg.py",
        r#"
        def test_fail_with_message():
            assert False, "This should not happen"
    "#,
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_fail_with_msg.py
     | File "<temp_dir>/test_fail_with_msg.py", line 3, in test_fail_with_message
     |   assert False, "This should not happen"
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_equality_assertion_fail() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_equality.py",
        r"
        def test_equality():
            x = 5
            y = 10
            assert x == y
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_equality.py
     | File "<temp_dir>/test_equality.py", line 5, in test_equality
     |   assert x == y
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_complex_assertion_fail() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_complex.py",
        r"
        def test_complex():
            data = [1, 2, 3]
            assert len(data) > 5, 'Data should have more items'
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_complex.py
     | File "<temp_dir>/test_complex.py", line 4, in test_complex
     |   assert len(data) > 5, 'Data should have more items'
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_long_file() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_long.py",
        r"
        # This is a long test file with many comments and functions
        # to test that we can handle files with many lines

        def helper_function_1():
            '''Helper function 1'''
            return 42

        def helper_function_2():
            '''Helper function 2'''
            return 'hello'

        def helper_function_3():
            '''Helper function 3'''
            return [1, 2, 3]

        def test_in_long_file():
            # This test is in a long file
            result = helper_function_1()
            expected = 100
            # This assertion should fail
            assert result == expected
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_long.py
     | File "<temp_dir>/test_long.py", line 22, in test_in_long_file
     |   assert result == expected
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_multiple_assertions_in_function() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_multiple_assertions.py",
        r"
        def test_multiple_assertions():
            x = 1
            y = 2
            assert x == 1  # This passes
            assert y == 3  # This fails
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_multiple_assertions.py
     | File "<temp_dir>/test_multiple_assertions.py", line 6, in test_multiple_assertions
     |   assert y == 3  # This fails
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_assertion_in_nested_function() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_nested.py",
        r"
        def helper():
            return False

        def test_with_nested_call():
            result = helper()
            assert result == True
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_nested.py
     | File "<temp_dir>/test_nested.py", line 7, in test_with_nested_call
     |   assert result == True
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_assertion_with_complex_expression() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_complex_expr.py",
        r"
        def test_complex_expression():
            items = [1, 2, 3, 4, 5]
            assert len([x for x in items if x > 3]) == 5
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_complex_expr.py
     | File "<temp_dir>/test_complex_expr.py", line 4, in test_complex_expression
     |   assert len([x for x in items if x > 3]) == 5
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_assertion_with_multiline_setup() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_multiline.py",
        r"
        def test_multiline_setup():
            # Setup with multiple lines
            a = 10
            b = 20
            c = a + b

            # Multiple operations
            result = c * 2
            expected = 100

            # The assertion that fails
            assert result == expected
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_multiline.py
     | File "<temp_dir>/test_multiline.py", line 13, in test_multiline_setup
     |   assert result == expected
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_assertion_with_very_long_line() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_very_long_line.py",
        r"
        def test_very_long_line():
            assert 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10 + 11 + 12 + 13 + 14 + 15 + 16 + 17 + 18 + 19 + 20 == 1000
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_very_long_line.py
     | File "<temp_dir>/test_very_long_line.py", line 3, in test_very_long_line
     |   assert 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10 + 11 + 12 + 13 + 14 + 15 + 16 + 17 + 18 + 19 + 20 == 1000
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_assertion_on_line_1() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_line_1.py",
        r"def test_line_1():
    assert False",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_line_1.py
     | File "<temp_dir>/test_line_1.py", line 2, in test_line_1
     |   assert False
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_multiple_files_with_cross_function_calls() -> anyhow::Result<()> {
    let case = TestCase::with_files([
        (
            "helper.py",
            r"
            def validate_data(data):
                if not data:
                    assert False, 'Data validation failed'
                return True
        ",
        ),
        (
            "test_cross_file.py",
            r"
            from helper import validate_data

            def test_with_helper():
                validate_data([])
        ",
        ),
    ])?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_cross_file.py
     | File "<temp_dir>/test_cross_file.py", line 5, in test_with_helper
     |   validate_data([])
     | File "<temp_dir>/helper.py", line 4, in validate_data
     |   assert False, 'Data validation failed'
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_nested_function_calls_deep_stack() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_deep_stack.py",
        r"
        def level_1():
            level_2()

        def level_2():
            level_3()

        def level_3():
            assert 1 == 2, 'Deep stack assertion failed'

        def test_deep_call_stack():
            level_1()
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_deep_stack.py
     | File "<temp_dir>/test_deep_stack.py", line 12, in test_deep_call_stack
     |   level_1()
     | File "<temp_dir>/test_deep_stack.py", line 3, in level_1
     |   level_2()
     | File "<temp_dir>/test_deep_stack.py", line 6, in level_2
     |   level_3()
     | File "<temp_dir>/test_deep_stack.py", line 9, in level_3
     |   assert 1 == 2, 'Deep stack assertion failed'
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_assertion_in_class_method() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_class.py",
        r"
        class Calculator:
            def add(self, a, b):
                return a + b

            def validate_result(self, result):
                assert result > 0, 'Result must be positive'

        def test_calculator():
            calc = Calculator()
            result = calc.add(-5, 3)
            calc.validate_result(result)
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_class.py
     | File "<temp_dir>/test_class.py", line 12, in test_calculator
     |   calc.validate_result(result)
     | File "<temp_dir>/test_class.py", line 7, in validate_result
     |   assert result > 0, 'Result must be positive'
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_assertion_in_imported_function() -> anyhow::Result<()> {
    let case = TestCase::with_files([
        (
            "validators.py",
            r"
            def is_positive(value):
                assert value > 0, f'Expected positive value, got {value}'
                return True
        ",
        ),
        (
            "test_import.py",
            r"
            from validators import is_positive

            def test_imported_validation():
                is_positive(-10)
        ",
        ),
    ])?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    fail[assertion-failed] in <temp_dir>/test_import.py
     | File "<temp_dir>/test_import.py", line 5, in test_imported_validation
     |   is_positive(-10)
     | File "<temp_dir>/validators.py", line 3, in is_positive
     |   assert value > 0, f'Expected positive value, got {value}'
    -------------
    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}
