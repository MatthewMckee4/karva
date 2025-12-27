use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

fn get_parametrize_function(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.parametrize",
        "karva" => "karva.tags.parametrize",
        _ => panic!("Invalid framework"),
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_fixture(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_parametrized_fixture(my_fixture=a) ... ok
        test test::test_with_parametrized_fixture(my_fixture=b) ... ok
        test test::test_with_parametrized_fixture(my_fixture=c) ... ok

        test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_fixture_in_conftest(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
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
            "test.py",
            r"
                    def test_with_number(number_fixture):
                        assert number_fixture in [10, 20, 30]
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_number(number_fixture=10) ... ok
        test test::test_with_number(number_fixture=20) ... ok
        test test::test_with_number(number_fixture=30) ... ok

        test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_fixture_module_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
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
            "test.py",
            r"
                    def test_first(module_fixture):
                        assert module_fixture in ['X', 'Y']

                    def test_second(module_fixture):
                        assert module_fixture in ['X', 'Y']
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_first(module_fixture=X) ... ok
        test test::test_first(module_fixture=X) ... ok
        test test::test_second(module_fixture=X) ... ok
        test test::test_second(module_fixture=X) ... ok

        test result: ok. 4 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_fixture_with_generator(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_setup(setup_fixture=setup_a) ... ok
        test test::test_with_setup(setup_fixture=setup_b) ... ok
        test test::test_verify_finalizers_ran ... ok

        test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_fixture_session_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
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
            "test_1.py",
            r"
                    def test_a1(session_fixture):
                        assert session_fixture in ['session_1', 'session_2']

                    def test_a2(session_fixture):
                        assert session_fixture in ['session_1', 'session_2']
                ",
        ),
        (
            "test_2.py",
            r"
                    def test_b1(session_fixture):
                        assert session_fixture in ['session_1', 'session_2']
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command().arg("-q"), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test result: ok. 6 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_fixture_with_multiple_params(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_combination(letter=a, number=10) ... ok
        test test::test_combination(letter=b, number=10) ... ok
        test test::test_combination(letter=a, number=20) ... ok
        test test::test_combination(letter=b, number=20) ... ok

        test result: ok. 4 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_fixture_with_regular_parametrize(
    #[values("pytest", "karva")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_both(fixture_param=1, test_param=10) ... ok
        test test::test_both(fixture_param=1, test_param=20) ... ok
        test test::test_both(fixture_param=2, test_param=10) ... ok
        test test::test_both(fixture_param=2, test_param=20) ... ok

        test result: ok. 4 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_generator_fixture_finalizer_order(
    #[values("pytest", "karva")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
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
                    assert execution_log == [
                        'first_setup',
                        'test_one_first',
                        'first_teardown',
                        'second_setup',
                        'test_one_second',
                        'second_teardown',
                    ], execution_log
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_one(ordered_fixture=first) ... ok
        test test::test_one(ordered_fixture=second) ... ok
        test test::test_check_order ... ok

        test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_fixture_package_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "package/conftest.py",
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
            "package/test_one.py",
            r"
                    def test_in_one(package_fixture):
                        assert package_fixture in ['pkg_a', 'pkg_b']
                ",
        ),
        (
            "package/test_two.py",
            r"
                    def test_in_two(package_fixture):
                        assert package_fixture in ['pkg_a', 'pkg_b']
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command().arg("-q"), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test result: ok. 4 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_fixture_with_dependency(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_dependent(dependent_fixture=100) ... ok
        test test::test_dependent(dependent_fixture=200) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_parametrized_fixture_finalizer_with_state(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_uses_resource(resource=resource_1) ... ok
        test test::test_uses_resource(resource=resource_2) ... ok
        test test::test_uses_resource(resource=resource_3) ... ok
        test test::test_all_cleaned_up ... ok

        test result: ok. 4 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_pytest_param() {
    let context = TestContext::with_file(
        "test.py",
        r"
            import pytest

            @pytest.fixture(params=[
                'resource_1',
                pytest.param('resource_2'),
                pytest.param('resource_3'),
                pytest.param('resource_4', marks=pytest.mark.skip),
                pytest.param('resource_5', marks=pytest.mark.xfail)
            ])
            def resource(request):
               return request.param

            def test_resource(resource):
                assert resource in ['resource_1', 'resource_2', 'resource_3']
   ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_resource(resource=resource_1) ... ok
    test test::test_resource(resource=resource_2) ... ok
    test test::test_resource(resource=resource_3) ... ok
    test test::test_resource ... skipped
    test test::test_resource(resource=resource_5) ... ok

    test result: ok. 4 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_karva_param() {
    let context = TestContext::with_file(
        "test.py",
        r"
            import karva

            @karva.fixture(params=[
                'resource_1',
                karva.param('resource_2'),
                karva.param('resource_3'),
                karva.param('resource_4', tags=[karva.tags.skip]),
                karva.param('resource_5', tags=[karva.tags.expect_fail]),
            ])
            def resource(request):
               return request.param

            def test_resource(resource):
                assert resource in ['resource_1', 'resource_2', 'resource_3']
   ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_resource(resource=resource_1) ... ok
    test test::test_resource(resource=resource_2) ... ok
    test test::test_resource(resource=resource_3) ... ok
    test test::test_resource ... skipped
    test test::test_resource(resource=resource_5) ... ok

    test result: ok. 4 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_complex_parametrized_generator_fixture_finalizer_order(
    #[values("pytest", "karva")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"

            import {framework}

            execution_log: list[str] = []


            @{framework}.fixture(params=["1_1", "1_2"])
            def ordered_fixture(request):
                execution_log.append(f"{{request.param}}_setup")
                yield request.param
                execution_log.append(f"{{request.param}}_teardown")


            @{framework}.fixture(params=["2_1", "2_2"])
            def ordered_fixture2(request):
                execution_log.append(f"{{request.param}}_setup")
                yield request.param
                execution_log.append(f"{{request.param}}_teardown")


            def test_one(ordered_fixture, ordered_fixture2):
                execution_log.append(f"{{ordered_fixture}}-{{ordered_fixture2}}_test")


            def test_check_order():
                assert execution_log == [
                    "1_1_setup",
                    "2_1_setup",
                    "1_1-2_1_test",
                    "2_1_teardown",
                    "1_1_teardown",
                    "1_1_setup",
                    "2_2_setup",
                    "1_1-2_2_test",
                    "2_2_teardown",
                    "1_1_teardown",
                    "1_2_setup",
                    "2_1_setup",
                    "1_2-2_1_test",
                    "2_1_teardown",
                    "1_2_teardown",
                    "1_2_setup",
                    "2_2_setup",
                    "1_2-2_2_test",
                    "2_2_teardown",
                    "1_2_teardown",
                ], execution_log

"#
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_one(ordered_fixture=1_1, ordered_fixture2=2_1) ... ok
        test test::test_one(ordered_fixture=1_1, ordered_fixture2=2_2) ... ok
        test test::test_one(ordered_fixture=1_2, ordered_fixture2=2_1) ... ok
        test test::test_one(ordered_fixture=1_2, ordered_fixture2=2_2) ... ok
        test test::test_check_order ... ok

        test result: ok. 5 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}
