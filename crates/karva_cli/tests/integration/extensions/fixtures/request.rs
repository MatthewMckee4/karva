use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use karva_test::IntegrationTestContext;
use rstest::rstest;

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_request(#[values("pytest", "karva")] framework: &str) {
    let test_context = IntegrationTestContext::with_file(
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
fn test_fixture_request_node_in_fixture(#[values("pytest", "karva")] framework: &str) {
    let test_context = IntegrationTestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def my_fixture(request):
                    # request should have a node property
                    assert hasattr(request, 'node')
                    # For fixtures, node should be the fixture function
                    assert request.node is not None
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

// Note: Test functions requesting 'request' directly is not yet supported
// This test is commented out until that feature is implemented
// #[rstest]
// #[ignore = "Will fail unless `maturin build` is ran"]
// fn test_request_node_in_test_function(#[values("pytest", "karva")] framework: &str) { ... }

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_request_node_function_scope(#[values("pytest", "karva")] framework: &str) {
    let test_context = IntegrationTestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def my_func_fixture(request):
                    # For function scope, node should have .name attribute
                    assert hasattr(request.node, 'name'), 'request.node should have .name attribute'
                    # The name should be the test function name
                    name = request.node.name
                    assert 'test_function_scope' in name, f'Expected test function name, got {{name}}'
                    return 'value'

                def test_function_scope_fixture(my_func_fixture):
                    assert my_func_fixture == 'value'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_function_scope_fixture(my_func_fixture=value) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_request_node_module_scope(#[values("pytest", "karva")] framework: &str) {
    let test_context = IntegrationTestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}
                import types

                @{framework}.fixture(scope='module')
                def module_fixture(request):
                    # For module scope, node should exist
                    assert hasattr(request, 'node')
                    assert request.node is not None
                    # Verify it has a name attribute (pytest-compatible interface)
                    assert hasattr(request.node, 'name'), 'node should have .name attribute'
                    assert request.node.name in ['test', '__main__'], f'Module name: {{request.node.name}}'
                    # Also verify it has the underlying module
                    assert hasattr(request.node, 'module')
                    return 'module_value'

                def test_module_scope_1(module_fixture):
                    assert module_fixture == 'module_value'

                def test_module_scope_2(module_fixture):
                    assert module_fixture == 'module_value'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_module_scope_1(module_fixture=module_value) ... ok
        test test::test_module_scope_2(module_fixture=module_value) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_request_node_with_parametrized_fixture(#[values("pytest", "karva")] framework: &str) {
    let test_context = IntegrationTestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture(params=[1, 2])
                def param_fixture(request):
                    # request.param should have the parameter value
                    assert request.param in [1, 2]
                    # request.node should exist
                    assert hasattr(request, 'node')
                    assert request.node is not None
                    return request.param * 10

                def test_with_param_fixture(param_fixture):
                    assert param_fixture in [10, 20]
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_param_fixture(param_fixture=10) ... ok
        test test::test_with_param_fixture(param_fixture=20) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_request_node_module_scope_has_test_functions(#[values("pytest", "karva")] framework: &str) {
    let test_context = IntegrationTestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}
                import types

                @{framework}.fixture(scope='module')
                def module_fixture(request):
                    # For module scope, node should have a .name attribute
                    assert hasattr(request.node, 'name')
                    assert request.node.name in ['test', '__main__']
                    # The underlying module should have our test functions as attributes
                    assert hasattr(request.node.module, 'test_one')
                    assert hasattr(request.node.module, 'test_two')
                    # The test functions should be callable
                    assert callable(request.node.module.test_one)
                    assert callable(request.node.module.test_two)
                    # Also verify we can access them via __getattr__ on the node
                    assert hasattr(request.node, 'test_one')
                    assert callable(request.node.test_one)
                    return 'fixture_value'

                def test_one(module_fixture):
                    assert module_fixture == 'fixture_value'

                def test_two(module_fixture):
                    assert module_fixture == 'fixture_value'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_one(module_fixture=fixture_value) ... ok
        test test::test_two(module_fixture=fixture_value) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}
