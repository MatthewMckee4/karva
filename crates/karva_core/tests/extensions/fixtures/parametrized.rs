use karva_core::TestResultStats;
use karva_test::TestContext;
use rstest::rstest;

use crate::{TestRunnerExt, get_parametrize_function};

#[rstest]
fn test_parametrized_fixture(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture(params=['a', 'b', 'c'])
                def my_fixture(request):
                    assert hasattr(request, 'param')
                    assert request.param in ['a', 'b', 'c']
                    return request.param

                def test_with_parametrized_fixture(my_fixture):
                    assert my_fixture in ['a', 'b', 'c']
"
        ),
    );

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    for _ in 0..3 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_parametrized_fixture_in_conftest(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_files([
        (
            "<test>/conftest.py",
            format!(
                r"
                    import {framework}

                    @{framework}.fixture(params=[1, 2, 3])
                    def number_fixture(request):
                        return request.param * 10
                "
            )
            .as_str(),
        ),
        (
            "<test>/test_file.py",
            r"
                    def test_with_number(number_fixture):
                        assert number_fixture in [10, 20, 30]
                ",
        ),
    ]);

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    for _ in 0..3 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_parametrized_fixture_module_scope(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_files([
        (
            "<test>/conftest.py",
            format!(
                r"
                    import {framework}

                    @{framework}.fixture(scope='module', params=['x', 'y'])
                    def module_fixture(request):
                        return request.param.upper()
                "
            )
            .as_str(),
        ),
        (
            "<test>/test_file.py",
            r"
                    def test_first(module_fixture):
                        assert module_fixture in ['X', 'Y']

                    def test_second(module_fixture):
                        assert module_fixture in ['X', 'Y']
                ",
        ),
    ]);

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    // 2 tests * 2 params = 4 test runs
    for _ in 0..4 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_parametrized_fixture_with_generator(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        &format!(
            r"
                import {framework}

                results = []

                @{framework}.fixture(params=['setup_a', 'setup_b'])
                def setup_fixture(request):
                    value = request.param
                    results.append(f'{{value}}_start')
                    yield value
                    results.append(f'{{value}}_end')

                def test_with_setup(setup_fixture):
                    assert setup_fixture in ['setup_a', 'setup_b']
                    assert len(results) >= 1

                def test_verify_finalizers_ran():
                    # This test runs after the parametrized tests
                    # Both finalizers should have run
                    assert 'setup_a_start' in results
                    assert 'setup_a_end' in results
                    assert 'setup_b_start' in results
                    assert 'setup_b_end' in results
"
        ),
    );

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    for _ in 0..3 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_parametrized_fixture_session_scope(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_files([
        (
            "<test>/conftest.py",
            format!(
                r"
                    import {framework}

                    call_count = []

                    @{framework}.fixture(scope='session', params=['session_1', 'session_2'])
                    def session_fixture(request):
                        call_count.append(request.param)
                        return request.param
                "
            )
            .as_str(),
        ),
        (
            "<test>/test_a.py",
            r"
                    def test_a1(session_fixture):
                        assert session_fixture in ['session_1', 'session_2']

                    def test_a2(session_fixture):
                        assert session_fixture in ['session_1', 'session_2']
                ",
        ),
        (
            "<test>/test_b.py",
            r"
                    def test_b1(session_fixture):
                        assert session_fixture in ['session_1', 'session_2']
                ",
        ),
    ]);

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    for _ in 0..6 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_parametrized_fixture_with_multiple_params(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture(params=[10, 20])
                def number(request):
                    return request.param

                @{framework}.fixture(params=['a', 'b'])
                def letter(request):
                    return request.param

                def test_combination(number, letter):
                    assert number in [10, 20]
                    assert letter in ['a', 'b']
"
        ),
    );

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    for _ in 0..4 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_parametrized_fixture_with_regular_parametrize(
    #[values("pytest", "karva")] framework: &str,
) {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture(params=[1, 2])
                def fixture_param(request):
                    return request.param

                @{parametrize}('test_param', [10, 20])
                def test_both(fixture_param, test_param):
                    assert fixture_param in [1, 2]
                    assert test_param in [10, 20]
",
            parametrize = get_parametrize_function(framework)
        ),
    );

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    for _ in 0..4 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_parametrized_generator_fixture_finalizer_order(
    #[values("pytest", "karva")] framework: &str,
) {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        &format!(
            r"
                import {framework}

                execution_log = []

                @{framework}.fixture(params=['first', 'second'])
                def ordered_fixture(request):
                    execution_log.append(f'{{request.param}}_setup')
                    yield request.param
                    execution_log.append(f'{{request.param}}_teardown')

                def test_one(ordered_fixture):
                    execution_log.append(f'test_one_{{ordered_fixture}}')
                    assert ordered_fixture in ['first', 'second']

                def test_check_order():
                    assert 'first_setup' in execution_log
                    assert 'test_one_first' in execution_log
                    assert 'first_teardown' in execution_log
                    assert 'second_setup' in execution_log
                    assert 'test_one_second' in execution_log
                    assert 'second_teardown' in execution_log

                    first_test_idx = execution_log.index('test_one_first')
                    first_teardown_idx = execution_log.index('first_teardown')
                    assert first_teardown_idx > first_test_idx
"
        ),
    );

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    for _ in 0..3 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_parametrized_fixture_package_scope(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_files([
        (
            "<test>/package/conftest.py",
            format!(
                r"
                    import {framework}

                    @{framework}.fixture(scope='package', params=['pkg_a', 'pkg_b'])
                    def package_fixture(request):
                        return request.param
                "
            )
            .as_str(),
        ),
        (
            "<test>/package/test_one.py",
            r"
                    def test_in_one(package_fixture):
                        assert package_fixture in ['pkg_a', 'pkg_b']
                ",
        ),
        (
            "<test>/package/test_two.py",
            r"
                    def test_in_two(package_fixture):
                        assert package_fixture in ['pkg_a', 'pkg_b']
                ",
        ),
    ]);

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    for _ in 0..4 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_parametrized_fixture_with_dependency(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture(params=[1, 2])
                def base_fixture(request):
                    return request.param

                @{framework}.fixture
                def dependent_fixture(base_fixture):
                    return base_fixture * 100

                def test_dependent(dependent_fixture):
                    assert dependent_fixture in [100, 200]
"
        ),
    );

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    for _ in 0..2 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}

#[rstest]
fn test_parametrized_fixture_finalizer_with_state(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
        "<test>/test_file.py",
        &format!(
            r"
                import {framework}

                arr = []

                @{framework}.fixture(params=['resource_1', 'resource_2', 'resource_3'])
                def resource(request):
                    resource_name = request.param
                    yield resource_name
                    arr.append(resource_name)

                def test_uses_resource(resource):
                    assert resource in ['resource_1', 'resource_2', 'resource_3']

                def test_all_cleaned_up():
                    assert arr == ['resource_1', 'resource_2', 'resource_3']
"
        ),
    );

    let result = test_context.test();

    let mut expected_stats = TestResultStats::default();

    for _ in 0..4 {
        expected_stats.add_passed();
    }

    assert_eq!(*result.stats(), expected_stats, "{result:?}");
}
