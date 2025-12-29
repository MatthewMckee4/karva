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

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_exists(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def my_fixture(request):
                    # request.node should exist
                    assert hasattr(request, 'node')
                    assert request.node is not None
                    return 'fixture_value'

                def test_with_node_access(my_fixture):
                    assert my_fixture == 'fixture_value'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_node_access(my_fixture=fixture_value) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_name(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def my_fixture(request):
                    # request.node.name should return the test name
                    assert hasattr(request.node, 'name')
                    assert request.node.name == 'test_with_node_name'
                    return 'fixture_value'

                def test_with_node_name(my_fixture):
                    assert my_fixture == 'fixture_value'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_node_name(my_fixture=fixture_value) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_autouse(#[values("pytest", "karva")] framework: &str) {
    // pytest uses 'autouse', karva uses 'auto_use'
    let autouse_param = if framework == "pytest" {
        "autouse"
    } else {
        "auto_use"
    };

    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture({autouse_param}=True)
                def autouse_fixture(request):
                    # Even autouse fixtures should have a node
                    assert hasattr(request, 'node')
                    assert request.node is not None
                    # autouse fixtures executed in test context should get the test name
                    assert request.node.name == 'test_with_autouse'

                def test_with_autouse():
                    pass
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_autouse ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_name_module_scope(#[values("pytest", "karva")] framework: &str) {
    let autouse_param = if framework == "pytest" {
        "autouse"
    } else {
        "auto_use"
    };

    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture(scope='module', {autouse_param}=True)
                def module_fixture(request):
                    # Module-scoped fixtures should have node.name = 'module'
                    assert hasattr(request.node, 'name')
                    assert request.node.name == 'module'

                def test_one():
                    pass

                def test_two():
                    pass
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command_no_parallel(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_one ... ok
        test test::test_two ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_name_with_parametrize(#[values("pytest")] framework: &str) {
    // Note: Only testing with pytest as karva uses different parametrize syntax
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def check_name(request):
                    # Should see test name (without parameters in current implementation)
                    name = request.node.name
                    assert name == 'test_params'
                    return name

                @{framework}.mark.parametrize('value', [1, 2])
                def test_params(value, check_name):
                    assert check_name == 'test_params'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command_no_parallel(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_params(check_name=test_params, value=1) ... ok
        test test::test_params(check_name=test_params, value=2) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_name_nested_fixtures(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def outer_fixture(request):
                    # Outer fixture should see the test name
                    assert request.node.name == 'test_nested'
                    return request.node.name

                @{framework}.fixture
                def inner_fixture(request, outer_fixture):
                    # Inner fixture should also see the test name
                    assert request.node.name == 'test_nested'
                    assert outer_fixture == 'test_nested'
                    return 'inner'

                def test_nested(inner_fixture):
                    assert inner_fixture == 'inner'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_nested(inner_fixture=inner) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_get_closest_marker_skip(#[values("pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def check_marker(request):
                    # Get the skip marker
                    marker = request.node.get_closest_marker('skip')
                    assert marker is not None
                    assert marker.kwargs.get('reason') == 'Testing skip marker'
                    return 'checked'

                @{framework}.mark.skip(reason='Testing skip marker')
                def test_with_skip_marker(check_marker):
                    assert check_marker == 'checked'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_skip_marker ... skipped: Testing skip marker

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_get_closest_marker_not_found(#[values("pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def check_no_marker(request):
                    # Try to get a marker that doesn't exist
                    marker = request.node.get_closest_marker('nonexistent')
                    assert marker is None
                    return 'checked'

                def test_without_marker(check_no_marker):
                    assert check_no_marker == 'checked'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_without_marker(check_no_marker=checked) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_get_closest_tag(#[values("pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def check_tag(request):
                    # get_closest_tag is an alias for get_closest_marker
                    marker = request.node.get_closest_tag('skip')
                    assert marker is not None
                    return 'checked'

                @{framework}.mark.skip(reason='Test reason')
                def test_with_tag(check_tag):
                    assert check_tag == 'checked'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_tag ... skipped: Test reason

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_with_parametrize(#[values("pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
                import {framework}

                @{framework}.fixture
                def check_parametrize(request):
                    # Get the parametrize marker
                    marker = request.node.get_closest_marker('parametrize')
                    assert marker is not None
                    # The marker should have the parametrize data
                    assert marker.args[0] == 'x'
                    return f'x={{request.param if hasattr(request, "param") and request.param else "none"}}'

                @{framework}.mark.parametrize('x', [1, 2, 3])
                def test_parametrized(check_parametrize, x):
                    assert x in [1, 2, 3]
"#
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_parametrized(check_parametrize=x=none, x=1) ... ok
        test test::test_parametrized(check_parametrize=x=none, x=2) ... ok
        test test::test_parametrized(check_parametrize=x=none, x=3) ... ok

        test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_with_custom_marker(#[values("pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def check_custom(request):
                    # Get a custom marker
                    marker = request.node.get_closest_marker('custom')
                    assert marker is not None
                    assert marker.args == ('arg1', 'arg2')
                    assert marker.kwargs == {{'key': 'value'}}
                    return 'checked'

                @{framework}.mark.custom('arg1', 'arg2', key='value')
                def test_with_custom_marker(check_custom):
                    assert check_custom == 'checked'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_custom_marker(check_custom=checked) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_multiple_markers(#[values("pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def check_multiple(request):
                    # Should find the first matching marker (closest)
                    skip_marker = request.node.get_closest_marker('skip')
                    custom_marker = request.node.get_closest_marker('custom')

                    assert skip_marker is not None
                    assert custom_marker is not None

                    return 'checked'

                @{framework}.mark.custom('value')
                @{framework}.mark.skip(reason='Multiple markers test')
                def test_with_multiple_markers(check_multiple):
                    assert check_multiple == 'checked'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_multiple_markers ... skipped: Multiple markers test

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_pytest_custom_marker(#[values("pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def check_custom_marker(request):
                    # Get a custom marker
                    marker = request.node.get_closest_marker('my_custom_marker')
                    assert marker is not None
                    assert marker.args == ('arg1', 'arg2')
                    assert marker.kwargs == {{'key1': 'value1', 'key2': 42}}
                    return 'checked'

                @{framework}.mark.my_custom_marker('arg1', 'arg2', key1='value1', key2=42)
                def test_with_custom_marker(check_custom_marker):
                    assert check_custom_marker == 'checked'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command_no_parallel(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_custom_marker(check_custom_marker=checked) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_karva_custom_tag(#[values("karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def check_custom_tag(request):
                    # Get a custom tag using get_closest_tag
                    tag = request.node.get_closest_tag('custom')
                    assert tag is not None
                    assert tag.name == 'custom'
                    assert tag.args == ('my_custom_tag', 'arg1', 'arg2')
                    assert tag.kwargs == {{'key': 'value'}}
                    return 'checked'

                @{framework}.tags.custom('my_custom_tag', 'arg1', 'arg2', key='value')
                def test_with_custom_tag(check_custom_tag):
                    assert check_custom_tag == 'checked'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command_no_parallel(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_custom_tag(check_custom_tag=checked) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_multiple_custom_markers(#[values("pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def check_multiple_custom(request):
                    # Get different custom markers
                    marker1 = request.node.get_closest_marker('marker1')
                    marker2 = request.node.get_closest_marker('marker2')

                    assert marker1 is not None
                    assert marker1.args == ('data1',)

                    assert marker2 is not None
                    assert marker2.args == ('data2',)

                    return 'checked'

                @{framework}.mark.marker1('data1')
                @{framework}.mark.marker2('data2')
                def test_with_multiple_custom(check_multiple_custom):
                    assert check_multiple_custom == 'checked'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command_no_parallel(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_multiple_custom(check_multiple_custom=checked) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request_node_no_args_custom_marker(#[values("pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def check_no_args_marker(request):
                    # Get a custom marker with no args
                    marker = request.node.get_closest_marker('simple_marker')
                    assert marker is not None
                    assert marker.args == ()
                    assert marker.kwargs == {{}}
                    return 'checked'

                @{framework}.mark.simple_marker
                def test_with_no_args_marker(check_no_args_marker):
                    assert check_no_args_marker == 'checked'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command_no_parallel(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_no_args_marker(check_no_args_marker=checked) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}
