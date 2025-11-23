use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::TestRunnerExt;
#[rstest]
fn test_temp_directory_fixture(
    #[values("tmp_path", "temp_path", "temp_dir", "tmpdir")] fixture_name: &str,
) {
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

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}
