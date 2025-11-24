use insta::{allow_duplicates, assert_snapshot};
use karva_core::{StandardTestRunner, TestRunner};
use karva_project::{Project, project::ProjectOptions};
use karva_test::TestContext;
use rstest::rstest;

#[rstest]
fn test_fixture_imported_from_other_file(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "<test>/foo.py",
            format!(
                r"
import {framework}

@{framework}.fixture
def x():
    return 1

@{framework}.fixture()
def y():
    return 1
            ",
            )
            .as_str(),
        ),
        (
            "<test>/test_file.py",
            "
            from <test>.foo import x, y

            def test_1(x): pass

            def test_2(y): pass

            ",
        ),
    ]);

    let mapped_path = context.mapped_path("<test>").unwrap().clone();
    let test_file1_path = mapped_path.join("test_file.py");

    let project = Project::new(context.cwd(), vec![test_file1_path])
        .with_options(ProjectOptions::default().with_try_import_fixtures(true));

    let test_runner = StandardTestRunner::new(&project);

    let result = test_runner.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}
