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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_fail.py:3:12 in function 'test_fail'
      |
    2 | def test_fail():
    3 |     assert False
      |            ^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_fail2.py:3:12 in function 'test_fail2'
      |
    2 | def test_fail2():
    3 |     assert 1 == 2
      |            ^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_fail.py:3:12 in function 'test_fail'
      |
    2 | def test_fail():
    3 |     assert False
      |            ^^^^^ assertion failed
      |

    ─────────────
    Passed tests: 1
    Failed tests: 1

    ----- stderr -----
    ");

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
    error[assertion-failed]: Assertion failed
     --> test_fail_with_msg.py:3:12 in function 'test_fail_with_message'
      |
    2 | def test_fail_with_message():
    3 |     assert False, "This should not happen"
      |            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ assertion failed
      |

    ─────────────
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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_equality.py:5:12 in function 'test_equality'
      |
    2 | def test_equality():
    3 |     x = 5
    4 |     y = 10
    5 |     assert x == y
      |            ^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_complex.py:4:12 in function 'test_complex'
      |
    2 | def test_complex():
    3 |     data = [1, 2, 3]
    4 |     assert len(data) > 5, 'Data should have more items'
      |            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_long.py:22:12 in function 'test_in_long_file'
       |
    17 | def test_in_long_file():
    18 |     # This test is in a long file
    19 |     result = helper_function_1()
    20 |     expected = 100
    21 |     # This assertion should fail
    22 |     assert result == expected
       |            ^^^^^^^^^^^^^^^^^^ assertion failed
       |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_multiple_assertions.py:6:12 in function 'test_multiple_assertions'
      |
    2 | def test_multiple_assertions():
    3 |     x = 1
    4 |     y = 2
    5 |     assert x == 1  # This passes
    6 |     assert y == 3  # This fails
      |            ^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_nested.py:7:12 in function 'test_with_nested_call'
      |
    5 | def test_with_nested_call():
    6 |     result = helper()
    7 |     assert result == True
      |            ^^^^^^^^^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_complex_expr.py:4:12 in function 'test_complex_expression'
      |
    2 | def test_complex_expression():
    3 |     items = [1, 2, 3, 4, 5]
    4 |     assert len([x for x in items if x > 3]) == 5
      |            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_multiline.py:13:12 in function 'test_multiline_setup'
       |
     8 |     # Multiple operations
     9 |     result = c * 2
    10 |     expected = 100
    11 |
    12 |     # The assertion that fails
    13 |     assert result == expected
       |            ^^^^^^^^^^^^^^^^^^ assertion failed
       |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_very_long_line.py:3:12 in function 'test_very_long_line'
      |
    2 | def test_very_long_line():
    3 |     assert 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10 + 11 + 12 + 13 + 14 + 15 + 16 + 17 + 18 + 19 + 20 == 1000
      |            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

    Ok(())
}

#[test]
fn test_assertion_on_line_1() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_line_1.py",
        r"def test_line_1():
    assert False",
    )?;

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_line_1.py:2:12 in function 'test_line_1'
      |
    1 | def test_line_1():
    2 |     assert False
      |            ^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> helper.py:4:16 in function 'test_with_helper'
      |
    2 | def validate_data(data):
    3 |     if not data:
    4 |         assert False, 'Data validation failed'
      |                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_deep_stack.py:9:12 in function 'test_deep_call_stack'
      |
    8 | def level_3():
    9 |     assert 1 == 2, 'Deep stack assertion failed'
      |            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_class.py:7:16 in function 'test_calculator'
      |
    6 |     def validate_result(self, result):
    7 |         assert result > 0, 'Result must be positive'
      |                ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> validators.py:3:12 in function 'test_imported_validation'
      |
    2 | def is_positive(value):
    3 |     assert value > 0, f'Expected positive value, got {value}'
      |            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

    Ok(())
}
