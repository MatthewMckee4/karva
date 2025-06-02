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
     --> test_fail.py:3:12
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
     --> test_fail2.py:3:12
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
     --> test_fail.py:3:12
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
     --> test_fail_with_msg.py:3:12
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
     --> test_equality.py:5:12
      |
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
            items = [1, 2, 3]
            assert len(items) > 5
    ",
    )?;

    assert_cmd_snapshot!(case.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    error[assertion-failed]: Assertion failed
     --> test_complex.py:4:12
      |
    3 |     items = [1, 2, 3]
    4 |     assert len(items) > 5
      |            ^^^^^^^^^^^^^^ assertion failed
      |

    ─────────────
    Failed tests: 1

    ----- stderr -----
    ");

    Ok(())
}
