use std::{
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::Context;
use insta::{allow_duplicates, internals::SettingsBindDropGuard};
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;
use tempfile::TempDir;

/// Find the karva wheel in the target/wheels directory.
/// Returns the path to the wheel file.
fn find_karva_wheel() -> anyhow::Result<PathBuf> {
    let karva_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| anyhow::anyhow!("Could not determine KARVA_ROOT"))?
        .to_path_buf();

    let wheels_dir = karva_root.join("target").join("wheels");

    let entries = std::fs::read_dir(&wheels_dir)
        .with_context(|| format!("Could not read wheels directory: {}", wheels_dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        if let Some(name) = file_name.to_str() {
            if name.starts_with("karva-")
                && std::path::Path::new(name)
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("whl"))
            {
                return Ok(entry.path());
            }
        }
    }

    anyhow::bail!("Could not find karva wheel in target/wheels directory");
}

struct TestCase {
    _temp_dir: TempDir,
    _settings_scope: SettingsBindDropGuard,
    project_dir: PathBuf,
}

impl TestCase {
    fn new() -> anyhow::Result<Self> {
        let temp_dir = TempDir::with_prefix("karva-test-env")?;

        let project_dir = dunce::simplified(
            &temp_dir
                .path()
                .canonicalize()
                .context("Failed to canonicalize project path")?,
        )
        .to_path_buf();

        let karva_wheel = find_karva_wheel()?;

        let venv_path = project_dir.join(".venv");

        // Set up a bare uv project and install pytest, mimicking the Python TestEnv
        let commands = [
            vec![
                "uv",
                "init",
                "--bare",
                "--directory",
                project_dir.to_str().unwrap(),
            ],
            vec!["uv", "venv", venv_path.to_str().unwrap()],
            vec![
                "uv",
                "pip",
                "install",
                "--python",
                venv_path.to_str().unwrap(),
                karva_wheel.to_str().unwrap(),
                "pytest",
            ],
            vec!["tree", "-a", project_dir.to_str().unwrap()],
        ];

        for command in &commands {
            let output = Command::new(command[0])
                .args(&command[1..])
                .current_dir(&project_dir)
                .output()
                .with_context(|| format!("Failed to run command: {command:?}"))?;
            if output.status.success() {
                eprintln!(
                    "Command succeeded: {:?}\nstdout:\n{}\nstderr:\n{}",
                    command,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
            } else {
                eprintln!(
                    "Command failed: {:?}\nstdout:\n{}\nstderr:\n{}",
                    command,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                anyhow::bail!("Command failed: {:?}", command);
            }
        }

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

    fn karva_bin(&self) -> PathBuf {
        let venv_bin =
            self.project_dir
                .join(".venv")
                .join(if cfg!(windows) { "Scripts" } else { "bin" });
        venv_bin.join(if cfg!(windows) { "karva.exe" } else { "karva" })
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
        let mut command = Command::new(self.karva_bin());
        command.current_dir(&self.project_dir).arg("test");
        command
    }

    fn command_with_args(&self, args: &[&str]) -> Command {
        let mut command = self.command();
        command.args(args);
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

    Failed tests: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_fixture_initialization_order(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="session")
                def session_calculator() -> Calculator:
                    print("Session calculator initialized")
                    return Calculator()

                @fixture(scope="module")
                def module_calculator() -> Calculator:
                    print("Module calculator initialized")
                    return Calculator()

                @fixture(scope="package")
                def package_calculator() -> Calculator:
                    print("Package calculator initialized")
                    return Calculator()

                @fixture
                def function_calculator() -> Calculator:
                    print("Function calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_all_scopes(
                session_calculator: Calculator,
                module_calculator: Calculator,
                package_calculator: Calculator,
                function_calculator: Calculator,
            ) -> None:
                assert session_calculator.add(1, 2) == 3
                assert module_calculator.add(1, 2) == 3
                assert package_calculator.add(1, 2) == 3
                assert function_calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!
    Session calculator initialized
    Package calculator initialized
    Module calculator initialized
    Function calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[test]
fn test_empty_conftest() -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "conftest.py".to_string(),
            r"
            # Empty conftest file
            "
            .to_string(),
        ),
        (
            "tests/conftest.py".to_string(),
            r"
            # Another empty conftest file
            "
            .to_string(),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_no_fixtures() -> None:
                calculator = Calculator()
                assert calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!

    ----- stderr -----
    "#);

    Ok(())
}

fn get_parametrize_function(package: &str) -> String {
    if package == "pytest" {
        "pytest.mark.parametrize".to_string()
    } else {
        "karva.tags.parametrize".to_string()
    }
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_parametrize(#[case] package: &str) -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_parametrize.py",
        &format!(
            r"
        import {package}

        @{parametrize_function}(('a', 'b', 'expected'), [
            (1, 2, 3),
            (2, 3, 5),
            (3, 4, 7),
        ])
        def test_parametrize(a, b, expected):
            assert a + b == expected
    ",
            package = package,
            parametrize_function = &get_parametrize_function(package),
        ),
    )?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 3
    All checks passed!

    ----- stderr -----
    "#));

    Ok(())
}

fn get_source_code(constructor_body: &str) -> Vec<(String, String)> {
    vec![
        (
            "src/__init__.py".to_string(),
            "from .calculator import Calculator".to_string(),
        ),
        (
            "src/calculator.py".to_string(),
            format!(
                r"
                class Calculator:
                    def __init__(self) -> None:
                        {constructor_body}

                    def add(self, a: int, b: int) -> int:
                        return a + b

                    def subtract(self, a: int, b: int) -> int:
                        return a - b

                    def multiply(self, a: int, b: int) -> int:
                        return a * b

                    def divide(self, a: int, b: int) -> float:
                        return a / b
                ",
            ),
        ),
    ]
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_function_scopes(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("print('Calculator initialized')");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def calculator() -> Calculator:
                    return Calculator()
                ",
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_add(calculator: Calculator) -> None:
                assert calculator.add(1, 2) == 3

            def test_subtract(calculator: Calculator) -> None:
                assert calculator.subtract(1, 2) == -1
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!
    Calculator initialized
    Calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_module_scopes(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("print('Calculator initialized')");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="module")
                def calculator() -> Calculator:
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_add(calculator: Calculator) -> None:
                assert calculator.add(1, 2) == 3

            def test_add_2(calculator: Calculator) -> None:
                assert calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!
    Calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_package_scopes(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("print('Calculator initialized')");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="package")
                def calculator() -> Calculator:
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_add(calculator: Calculator) -> None:
                assert calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
        (
            "tests/test_calculator_2.py".to_string(),
            r"
            from src import Calculator

            def test_add(calculator: Calculator) -> None:
                assert calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!
    Calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_session_scopes(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("print('Calculator initialized')");
    files.extend([
        (
            "conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="session")
                def calculator() -> Calculator:
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_add(calculator: Calculator) -> None:
                assert calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
        (
            "tests/test_calculator_2.py".to_string(),
            r"
            from src import Calculator

            def test_add(calculator: Calculator) -> None:
                assert calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
        (
            "tests/inner/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_add(calculator: Calculator) -> None:
                assert calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 3
    All checks passed!
    Calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_mixed_scopes(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="session")
                def session_calculator() -> Calculator:
                    print("Session calculator initialized")
                    return Calculator()

                @fixture
                def function_calculator() -> Calculator:
                    print("Function calculator initialized")
                    return Calculator()
                "#,

            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_session_fixture(session_calculator: Calculator) -> None:
                assert session_calculator.add(1, 2) == 3

            def test_function_fixture(function_calculator: Calculator) -> None:
                assert function_calculator.add(1, 2) == 3

            def test_both_fixtures(session_calculator: Calculator, function_calculator: Calculator) -> None:
                assert session_calculator.add(1, 2) == 3
                assert function_calculator.add(1, 2) == 3
            ".to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 3
    All checks passed!
    Session calculator initialized
    Function calculator initialized
    Function calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_fixture_across_files(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="package")
                def package_calculator() -> Calculator:
                    print("Package calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_package_fixture(package_calculator: Calculator) -> None:
                assert package_calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
        (
            "tests/another_test.py".to_string(),
            r"
            from src import Calculator

            def test_same_package_fixture(package_calculator: Calculator) -> None:
                assert package_calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!
    Package calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_named_fixtures(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("print('Named calculator initialized')");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(name="named_calculator")
                def calculator() -> Calculator:
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_named_fixture(named_calculator: Calculator) -> None:
                assert named_calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!
    Named calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_nested_package_scopes(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/test_calculator.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def calculator() -> Calculator:
                    print("Calculator initialized")
                    return Calculator()

                def test_add(calculator: Calculator) -> None:
                    assert calculator.add(1, 2) == 3
                "#,
            ),
        ),
        (
            "tests/inner/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="package")
                def package_calculator() -> Calculator:
                    print("Package calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/inner/sub/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_add(package_calculator: Calculator) -> None:
                assert package_calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!
    Calculator initialized
    Package calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_independent_fixtures(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def calculator_a() -> Calculator:
                    print("Calculator A initialized")
                    return Calculator()

                @fixture
                def calculator_b() -> Calculator:
                    print("Calculator B initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_a(calculator_a: Calculator) -> None:
                assert calculator_a.add(1, 2) == 3

            def test_b(calculator_b: Calculator) -> None:
                assert calculator_b.multiply(2, 3) == 6
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!
    Calculator A initialized
    Calculator B initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_multiple_files_independent_fixtures(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="module")
                def multiply_calculator() -> Calculator:
                    print("Multiply calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_add.py".to_string(),
            r"
            from src import Calculator

            def test_add_1(multiply_calculator: Calculator) -> None:
                assert multiply_calculator.add(1, 2) == 3

            def test_add_2(multiply_calculator: Calculator) -> None:
                assert multiply_calculator.add(3, 4) == 7
            "
            .to_string(),
        ),
        (
            "tests/test_multiply.py".to_string(),
            r"
            from src import Calculator

            def test_multiply_1(multiply_calculator: Calculator) -> None:
                assert multiply_calculator.multiply(2, 3) == 6

            def test_multiply_2(multiply_calculator: Calculator) -> None:
                assert multiply_calculator.multiply(4, 5) == 20
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 4
    All checks passed!
    Multiply calculator initialized
    Multiply calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_basic_error_handling(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def failing_calculator() -> Calculator:
                    raise RuntimeError("Fixture initialization failed")

                @fixture
                def working_calculator() -> Calculator:
                    print("Working calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_working(working_calculator: Calculator) -> None:
                assert working_calculator.add(1, 2) == 3

            def test_failing(failing_calculator: Calculator) -> None:
                assert failing_calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    error[fixture-not-found] in <temp_dir>/tests/test_calculator.py
     | Fixture failing_calculator not found

    Passed tests: 1
    Errored tests: 1
    Working calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_different_scopes_independent(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="session")
                def session_calculator() -> Calculator:
                    print("Session calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="package")
                def package_calculator() -> Calculator:
                    print("Package calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/inner/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="module")
                def module_calculator() -> Calculator:
                    print("Module calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/inner/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_session(session_calculator: Calculator) -> None:
                assert session_calculator.add(1, 2) == 3

            def test_package(package_calculator: Calculator) -> None:
                assert package_calculator.subtract(5, 3) == 2

            def test_module(module_calculator: Calculator) -> None:
                assert module_calculator.multiply(2, 3) == 6
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 3
    All checks passed!
    Session calculator initialized
    Package calculator initialized
    Module calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_invalid_scope_value(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="invalid_scope")
                def calculator() -> Calculator:
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_calc(calculator: Calculator) -> None:
                assert calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    error[invalid-fixture] in <temp_dir>/tests/conftest.py
     | Invalid fixture scope: invalid_scope

    error[fixture-not-found] in <temp_dir>/tests/test_calculator.py
     | Fixture calculator not found

    Errored tests: 1

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_invalid_fixture_name(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(name="123invalid")
                def calculator() -> Calculator:
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_calc(calculator: Calculator) -> None:
                assert calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    error[fixture-not-found] in <temp_dir>/tests/test_calculator.py
     | Fixture calculator not found

    Errored tests: 1

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_multiple_conftest_same_dir(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def calculator_1() -> Calculator:
                    print("Calculator 1 initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/conftest_more.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def calculator_2() -> Calculator:
                    print("Calculator 2 initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_calc(calculator_1: Calculator, calculator_2: Calculator) -> None:
                assert calculator_1.add(1, 2) == 3
                assert calculator_2.multiply(2, 3) == 6
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    error[fixture-not-found] in <temp_dir>/tests/test_calculator.py
     | Fixture calculator_2 not found

    Errored tests: 1
    Calculator 1 initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_very_deep_directory_structure(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="session")
                def root_calc() -> Calculator:
                    print("Root calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/level1/level2/level3/level4/level5/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="package")
                def deep_calc() -> Calculator:
                    print("Deep calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/level1/level2/level3/level4/level5/test_deep.py".to_string(),
            r"
            from src import Calculator

            def test_deep(root_calc: Calculator, deep_calc: Calculator) -> None:
                assert root_calc.add(1, 2) == 3
                assert deep_calc.multiply(2, 3) == 6
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!
    Root calculator initialized
    Deep calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_fixture_in_init_file(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/__init__.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def init_calculator() -> Calculator:
                    print("Init calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_init_fixture(init_calculator: Calculator) -> None:
                assert init_calculator.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    error[fixture-not-found] in <temp_dir>/tests/test_calculator.py
     | Fixture init_calculator not found

    Errored tests: 1

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_same_fixture_name_different_types(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/math/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def value() -> Calculator:
                    print("Calculator value initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/math/test_math.py".to_string(),
            r"
            from src import Calculator

            def test_math_value(value: Calculator) -> None:
                assert value.add(1, 2) == 3
            "
            .to_string(),
        ),
        (
            "tests/string/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture

                @fixture
                def value() -> str:
                    print("String value initialized")
                    return "test"
                "#,
            ),
        ),
        (
            "tests/string/test_string.py".to_string(),
            r#"
            def test_string_value(value: str) -> None:
                assert value == "test"
            "#
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command(), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_fixture_dependencies(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("print('Calculator initialized')");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def calculator() -> Calculator:
                    print("Calculator fixture initialized")
                    return Calculator()

                @fixture
                def calculator_with_value(calculator: Calculator) -> Calculator:
                    print("Calculator with value fixture initialized")
                    calculator.add(5, 5)
                    return calculator
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_calculator_with_value(calculator_with_value: Calculator) -> None:
                assert calculator_with_value.add(1, 2) == 3

            def test_calculator_dependency(calculator: Calculator, calculator_with_value: Calculator) -> None:
                assert calculator.add(1, 2) == 3
                assert calculator_with_value.add(1, 2) == 3
            ".to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!
    Calculator fixture initialized
    Calculator initialized
    Calculator with value fixture initialized
    Calculator fixture initialized
    Calculator initialized
    Calculator with value fixture initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_dependent_fixtures_different_scopes(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="session")
                def session_calculator() -> Calculator:
                    print("Session calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="package")
                def package_calculator(session_calculator: Calculator) -> Calculator:
                    print("Package calculator initialized")
                    session_calculator.add(1, 1)
                    return session_calculator
                "#,
            ),
        ),
        (
            "tests/inner/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="module")
                def module_calculator(package_calculator: Calculator) -> Calculator:
                    print("Module calculator initialized")
                    package_calculator.add(2, 2)
                    return package_calculator
                "#,
            ),
        ),
        (
            "tests/inner/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_calculator_chain(module_calculator: Calculator) -> None:
                assert module_calculator.add(1, 2) == 3

            def test_calculator_chain_2(module_calculator: Calculator) -> None:
                assert module_calculator.add(3, 4) == 7
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!
    Session calculator initialized
    Package calculator initialized
    Module calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_complex_dependency_chain(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def base_calculator() -> Calculator:
                    print("Base calculator initialized")
                    return Calculator()

                @fixture
                def add_calculator(base_calculator: Calculator) -> Calculator:
                    print("Add calculator initialized")
                    base_calculator.add(1, 1)
                    return base_calculator

                @fixture
                def multiply_calculator(add_calculator: Calculator) -> Calculator:
                    print("Multiply calculator initialized")
                    add_calculator.multiply(2, 2)
                    return add_calculator

                @fixture
                def final_calculator(multiply_calculator: Calculator, base_calculator: Calculator) -> Calculator:
                    print("Final calculator initialized")
                    return multiply_calculator
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_complex_chain(final_calculator: Calculator) -> None:
                assert final_calculator.add(1, 2) == 3
                assert final_calculator.multiply(2, 3) == 6
            ".to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!
    Base calculator initialized
    Add calculator initialized
    Multiply calculator initialized
    Final calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_mixed_scope_dependencies(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="session")
                def session_base() -> Calculator:
                    print("Session base initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture(scope="package")
                def package_calc(session_base: Calculator) -> Calculator:
                    print("Package calc initialized")
                    return session_base

                @fixture
                def function_calc(package_calc: Calculator) -> Calculator:
                    print("Function calc initialized")
                    return package_calc
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_mixed_scopes_1(function_calc: Calculator) -> None:
                assert function_calc.add(1, 2) == 3

            def test_mixed_scopes_2(function_calc: Calculator) -> None:
                assert function_calc.multiply(2, 3) == 6
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!
    Session base initialized
    Package calc initialized
    Function calc initialized
    Function calc initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_diamond_dependency(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def base_calc() -> Calculator:
                    print("Base calc initialized")
                    return Calculator()

                @fixture
                def left_calc(base_calc: Calculator) -> Calculator:
                    print("Left calc initialized")
                    base_calc.add(1, 1)
                    return base_calc

                @fixture
                def right_calc(base_calc: Calculator) -> Calculator:
                    print("Right calc initialized")
                    base_calc.multiply(2, 2)
                    return base_calc

                @fixture
                def final_calc(left_calc: Calculator, right_calc: Calculator) -> Calculator:
                    print("Final calc initialized")
                    return left_calc
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_diamond(final_calc: Calculator) -> None:
                assert final_calc.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!
    Base calc initialized
    Left calc initialized
    Right calc initialized
    Final calc initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_generator_fixture(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def generator_fixture():
                    yield Calculator()
                    print("Generator fixture teardown")
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r"
            from src import Calculator

            def test_generator_fixture(generator_fixture: Calculator) -> None:
                assert generator_fixture.add(1, 2) == 3
            "
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!
    Generator fixture teardown

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_fixture_called_for_each_parametrization(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def calculator() -> Calculator:
                    print("Calculator initialized")
                    return Calculator()
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            format!(
                r#"
                from src import Calculator
                import {}

                @{}.{}(
                    "value",
                    [1, 2, 3],
                )
                def test_calculator(calculator: Calculator, value: int) -> None:
                    assert calculator.add(1, value) == value + 1
                "#,
                framework,
                framework,
                if framework == "karva" {
                    "tags.parametrize"
                } else {
                    "mark.parametrize"
                }
            ),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 3
    All checks passed!
    Calculator initialized
    Calculator initialized
    Calculator initialized

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_fixture_finalizer_called_after_test(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def calculator() -> Calculator:
                    print("Calculator initialized")
                    yield Calculator()
                    print("Calculator finalizer called")
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r#"
            from src import Calculator

            def test_calculator(calculator: Calculator) -> None:
                print("Test function called")
                assert calculator.add(1, 2) == 3
            "#
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!
    Calculator initialized
    Test function called
    Calculator finalizer called

    ----- stderr -----
    "#));

    Ok(())
}

#[rstest]
#[case("pytest")]
#[case("karva")]
fn test_fixture_finalizer_called_at_correct_time(#[case] framework: &str) -> anyhow::Result<()> {
    let mut files = get_source_code("pass");
    files.extend([
        (
            "tests/conftest.py".to_string(),
            format!(
                r#"
                from {framework} import fixture
                from src import Calculator

                @fixture
                def calculator() -> Calculator:
                    print("Calculator initialized")
                    yield Calculator()
                    print("Calculator finalizer called")
                "#,
            ),
        ),
        (
            "tests/test_calculator.py".to_string(),
            r#"
            from src import Calculator

            def test_calculator(calculator: Calculator) -> None:
                print("Test function called")
                assert calculator.add(1, 2) == 3

            def test_calculator_2(calculator: Calculator) -> None:
                print("Test function 2 called")
                assert calculator.add(1, 2) == 3
            "#
            .to_string(),
        ),
    ]);

    let case = TestCase::with_files(files.iter().map(|(k, v)| (k.as_str(), v.as_str())))?;

    allow_duplicates!(assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 2
    All checks passed!
    Calculator initialized
    Calculator initialized
    Test function called
    Calculator finalizer called
    Test function 2 called
    Calculator finalizer called

    ----- stderr -----
    "#));

    Ok(())
}

#[test]
fn test_stdout_is_captured_and_displayed() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_std_out_redirected.py",
        r"
        def test_std_out_redirected():
            print('Hello, world!')
        ",
    )?;

    assert_cmd_snapshot!(case.command_with_args(&["-s"]), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!
    Hello, world!

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn test_stdout_is_captured_and_displayed_with_args() -> anyhow::Result<()> {
    let case = TestCase::with_file(
        "test_std_out_redirected.py",
        r"
        def test_std_out_redirected():
            print('Hello, world!')
        ",
    )?;

    assert_cmd_snapshot!(case.command(), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Passed tests: 1
    All checks passed!

    ----- stderr -----
    "#);

    Ok(())
}
