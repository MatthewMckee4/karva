use camino::Utf8PathBuf;
use insta::assert_snapshot;
use karva_core::{StandardTestRunner, TestRunner, testing::setup_module};
use karva_project::{Project, project::ProjectOptions};
use karva_test::TestContext;

use crate::common::TestRunnerExt;

#[ctor::ctor]
pub fn setup() {
    setup_module();
}

#[test]
fn test_single_file() {
    let context = TestContext::with_files([
        (
            "<test>/test_file1.py",
            r"
def test_1(): pass
def test_2(): pass",
        ),
        (
            "<test>/test_file2.py",
            r"
def test_3(): pass
def test_4(): pass",
        ),
    ]);

    let mapped_path = context.mapped_path("<test>").unwrap().clone();
    let test_file1_path = mapped_path.join("test_file1.py");

    let project = Project::new(context.cwd(), vec![test_file1_path]);

    let test_runner = StandardTestRunner::new(&project);

    let result = test_runner.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_empty_file() {
    let context = TestContext::with_file("<test>/test_empty.py", "");

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_empty_directory() {
    let context = TestContext::with_file("<test>/tests/test_empty.py", "");

    let mapped_tests_dir = context.mapped_path("<test>").unwrap();

    let project = Project::new(context.cwd(), vec![mapped_tests_dir.clone()]);

    let test_runner = karva_core::runner::StandardTestRunner::new(&project);

    let result = test_runner.test();

    assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_single_function() {
    let context = TestContext::with_files([(
        "<test>/test_file.py",
        r"
            def test_1(): pass
            def test_2(): pass",
    )]);

    let mapped_path = context.mapped_path("<test>").unwrap().clone();

    let test_file1_path = mapped_path.join("test_file.py");

    let project = Project::new(
        context.cwd(),
        vec![Utf8PathBuf::from(format!("{test_file1_path}::test_1"))],
    );

    let test_runner = karva_core::runner::StandardTestRunner::new(&project);

    let result = test_runner.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_single_function_shadowed_by_file() {
    let context = TestContext::with_files([(
        "<test>/test_file.py",
        r"
def test_1(): pass
def test_2(): pass",
    )]);

    let mapped_path = context.mapped_path("<test>").unwrap().clone();

    let test_file1_path = mapped_path.join("test_file.py");

    let project = Project::new(
        context.cwd(),
        vec![
            Utf8PathBuf::from(format!("{test_file1_path}::test_1")),
            test_file1_path,
        ],
    );

    let test_runner = karva_core::runner::StandardTestRunner::new(&project);

    let result = test_runner.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_single_function_shadowed_by_directory() {
    let context = TestContext::with_files([(
        "<test>/test_file.py",
        r"
def test_1(): pass
def test_2(): pass",
    )]);

    let mapped_path = context.mapped_path("<test>").unwrap().clone();

    let test_file1_path = mapped_path.join("test_file.py");

    let project = Project::new(
        context.cwd(),
        vec![
            Utf8PathBuf::from(format!("{test_file1_path}::test_1")),
            mapped_path,
        ],
    );

    let test_runner = StandardTestRunner::new(&project);

    let result = test_runner.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_fail_fast() {
    let context = TestContext::with_files([(
        "<test>/test_file.py",
        r"
def test_1():
    assert False
def test_2():
    assert False
",
    )]);

    let project = Project::new(context.cwd(), vec![context.cwd()])
        .with_options(ProjectOptions::default().with_fail_fast(true));

    let test_runner = StandardTestRunner::new(&project);

    let result = test_runner.test();

    assert_snapshot!(result.display(), @r"
    test failures:

    test `<test>.test_file::test_1` at <temp_dir>/<test>/test_file.py:2 failed at <temp_dir>/<test>/test_file.py:3

    test failures:
        <test>.test_file::test_1 at <temp_dir>/<test>/test_file.py:2

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    ");
}
