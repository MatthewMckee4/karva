use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::TestRunnerExt;

#[rstest]
fn test_fixture_request(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def my_fixture(request):
                    # request should be a FixtureRequest instance with a param property
                    assert hasattr(request, 'param')
                    # For non-parametrized fixtures, param should be None
                    assert request.param is None
                    return 'fixture_value'

                def test_with_request_fixture(my_fixture):
                    assert my_fixture == 'fixture_value'
"
        ),
    );

    let result = test_context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}
