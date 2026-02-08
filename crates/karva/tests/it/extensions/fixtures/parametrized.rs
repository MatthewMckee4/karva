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
fn test_fixture_basic(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def my_fixture():
                    return 'value'

                def test_with_fixture(my_fixture):
                    assert my_fixture == 'value'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.test(), @"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_fixture(my_fixture=value) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_in_conftest(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r"
                    import {framework}

                    @{framework}.fixture
                    def number_fixture():
                        return 42
                "
            )
            .as_str(),
        ),
        (
            "test.py",
            r"
                    def test_with_number(number_fixture):
                        assert number_fixture == 42
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.test(), @"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_number(number_fixture=42) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_module_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r"
                    import {framework}

                    call_count = []

                    @{framework}.fixture(scope='module')
                    def module_fixture():
                        call_count.append(1)
                        return 'MODULE'
                "
            )
            .as_str(),
        ),
        (
            "test.py",
            r"
                    from conftest import call_count

                    def test_first(module_fixture):
                        assert module_fixture == 'MODULE'

                    def test_second(module_fixture):
                        assert module_fixture == 'MODULE'
                        # Module scope means fixture is called once
                        assert len(call_count) == 1
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.test_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_first(module_fixture=MODULE) ... ok
        test test::test_second(module_fixture=MODULE) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_with_generator(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                results = []

                @{framework}.fixture
                def setup_fixture():
                    value = 'resource'
                    results.append('start')
                    yield value
                    results.append('end')

                def test_with_setup(setup_fixture):
                    assert setup_fixture == 'resource'
                    assert results == ['start']

                def test_verify_finalizer_ran():
                    # Fixture teardown runs after test_with_setup completes
                    assert results == ['start', 'end']
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.test_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_with_setup(setup_fixture=resource) ... ok
        test test::test_verify_finalizer_ran ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_session_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r"
                    import {framework}

                    call_count = []

                    @{framework}.fixture(scope='session')
                    def session_fixture():
                        call_count.append(1)
                        return 'session_value'
                "
            )
            .as_str(),
        ),
        (
            "test_1.py",
            r"
                    def test_a1(session_fixture):
                        assert session_fixture == 'session_value'

                    def test_a2(session_fixture):
                        assert session_fixture == 'session_value'
                ",
        ),
        (
            "test_2.py",
            r"
                    def test_b1(session_fixture):
                        assert session_fixture == 'session_value'
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.test().arg("-q"), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_with_multiple_fixtures(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def number():
                    return 100

                @{framework}.fixture
                def letter():
                    return 'X'

                def test_combination(number, letter):
                    assert number == 100
                    assert letter == 'X'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.test(), @"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_combination(letter=X, number=100) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_with_test_parametrize(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def fixture_value():
                    return 'fixture_value'

                @{parametrize}('test_param', [10, 20])
                def test_both(fixture_value, test_param):
                    assert fixture_value == 'fixture_value'
                    assert test_param in [10, 20]
",
            parametrize = get_parametrize_function(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.test(), @"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_both(fixture_value=fixture_value, test_param=10) ... ok
        test test::test_both(fixture_value=fixture_value, test_param=20) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_generator_finalizer_order(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                execution_log = []

                @{framework}.fixture
                def ordered_fixture():
                    execution_log.append('setup')
                    yield 'value'
                    execution_log.append('teardown')

                def test_one(ordered_fixture):
                    execution_log.append('test_one')
                    assert ordered_fixture == 'value'

                def test_check_order():
                    # After test_one completes, fixture is torn down
                    assert execution_log == [
                        'setup',
                        'test_one',
                        'teardown',
                    ], execution_log
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.test_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_one(ordered_fixture=value) ... ok
        test test::test_check_order ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_package_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "package/conftest.py",
            format!(
                r"
                    import {framework}

                    @{framework}.fixture(scope='package')
                    def package_fixture():
                        return 'package_value'
                "
            )
            .as_str(),
        ),
        (
            "package/test_one.py",
            r"
                    def test_in_one(package_fixture):
                        assert package_fixture == 'package_value'
                ",
        ),
        (
            "package/test_two.py",
            r"
                    def test_in_two(package_fixture):
                        assert package_fixture == 'package_value'
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.test().arg("-q"), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_with_dependency(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def base_fixture():
                    return 10

                @{framework}.fixture
                def dependent_fixture(base_fixture):
                    return base_fixture * 100

                def test_dependent(dependent_fixture):
                    assert dependent_fixture == 1000
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.test(), @"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_dependent(dependent_fixture=1000) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_finalizer_with_state(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                arr = []

                @{framework}.fixture
                def resource():
                    resource_name = 'resource'
                    yield resource_name
                    arr.append(resource_name)

                def test_uses_resource(resource):
                    assert resource == 'resource'

                def test_all_cleaned_up():
                    # Only one test used the fixture
                    assert arr == ['resource']
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.test_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_uses_resource(resource=resource) ... ok
        test test::test_all_cleaned_up ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_complex_fixture_generator_finalizer_order(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"

            import {framework}

            execution_log: list[str] = []


            @{framework}.fixture
            def ordered_fixture():
                execution_log.append("1_setup")
                yield "value1"
                execution_log.append("1_teardown")


            @{framework}.fixture
            def ordered_fixture2():
                execution_log.append("2_setup")
                yield "value2"
                execution_log.append("2_teardown")


            def test_one(ordered_fixture, ordered_fixture2):
                execution_log.append("test")


            def test_check_order():
                # After test_one, both fixtures are torn down in reverse order
                assert execution_log == [
                    "1_setup",
                    "2_setup",
                    "test",
                    "2_teardown",
                    "1_teardown",
                ], execution_log

"#
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.test_no_parallel(), @r#"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_one(ordered_fixture=value1, ordered_fixture2=value2) ... ok
        test test::test_check_order ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        "#);
    }
}
