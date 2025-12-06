use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_request_fixture(my_fixture=fixture_value) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}
