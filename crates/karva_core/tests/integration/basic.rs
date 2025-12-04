use camino::Utf8PathBuf;
use insta::assert_snapshot;
use karva_core::{StandardTestRunner, TestRunResult, TestRunner, testing::setup_module};
use karva_project::ProjectDatabase;
use karva_test::TestContext;

use crate::common::TestRunnerExt;

#[allow(clippy::needless_pass_by_value)]
fn run_context(context: &TestContext, paths: Vec<Utf8PathBuf>) -> TestRunResult {
    let db = ProjectDatabase::test_db(context.cwd(), &paths);
    let test_runner = StandardTestRunner::new(&db);
    test_runner.test()
}

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

    let result = run_context(&context, vec![test_file1_path]);

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_empty_file() {
    let context = TestContext::with_file("<test>/test.py", "");

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_empty_directory() {
    let context = TestContext::with_file("<test>/test.py", "");

    let mapped_tests_dir = context.mapped_path("<test>").unwrap();

    let result = run_context(&context, vec![mapped_tests_dir.clone()]);

    assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_single_function() {
    let context = TestContext::with_files([(
        "<test>/test.py",
        r"
            def test_1(): pass
            def test_2(): pass",
    )]);

    let mapped_path = context.mapped_path("<test>").unwrap().clone();

    let test_file1_path = mapped_path.join("test.py");

    let result = run_context(
        &context,
        vec![Utf8PathBuf::from(format!("{test_file1_path}::test_1"))],
    );

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_single_function_shadowed_by_file() {
    let context = TestContext::with_files([(
        "<test>/test.py",
        r"
def test_1(): pass
def test_2(): pass",
    )]);

    let mapped_path = context.mapped_path("<test>").unwrap().clone();

    let test_file1_path = mapped_path.join("test.py");

    let result = run_context(
        &context,
        vec![
            Utf8PathBuf::from(format!("{test_file1_path}::test_1")),
            test_file1_path,
        ],
    );

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_single_function_shadowed_by_directory() {
    let context = TestContext::with_files([(
        "<test>/test.py",
        r"
def test_1(): pass
def test_2(): pass",
    )]);

    let mapped_path = context.mapped_path("<test>").unwrap().clone();

    let test_file1_path = mapped_path.join("test.py");

    let result = run_context(
        &context,
        vec![
            Utf8PathBuf::from(format!("{test_file1_path}::test_1")),
            mapped_path,
        ],
    );

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}
