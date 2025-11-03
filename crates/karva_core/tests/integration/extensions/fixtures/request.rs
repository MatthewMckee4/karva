use karva_core::TestResultStats;
use karva_test::TestContext;
use rstest::rstest;

use crate::common::TestRunnerExt;

#[rstest]
fn test_fixture_request(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
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

    let mut expected_stats = TestResultStats::default();

    expected_stats.add_passed();

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}
