use karva_core::TestResultStats;
use karva_test::TestContext;
use rstest::rstest;

use crate::common::TestRunnerExt;

#[rstest]
fn test_temp_directory_fixture(#[values("tmp_path", "temp_path", "temp_dir")] fixture_name: &str) {
    let test_context = TestContext::with_file(
        "<test>/test.py",
        &format!(
            r"
                import pathlib

                def test_temp_directory_fixture({fixture_name}):
                    assert {fixture_name}.exists()
                    assert {fixture_name}.is_dir()
                    assert {fixture_name}.is_absolute()
                    assert isinstance({fixture_name}, pathlib.Path)
                "
        ),
    );

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    expected_stats.add_passed();

    assert_eq!(*result.stats(), expected_stats);

    assert!(result.diagnostics().is_empty());
}
