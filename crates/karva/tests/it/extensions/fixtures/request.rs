use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[rstest]
fn fixture_request_exposes_context_and_sync_lookup(#[values("karva", "pytest")] framework: &str) {
    let source = format!(
        r#"
            from pathlib import Path
            import sys
            import {framework} as framework

            events = []

            @framework.fixture
            def resource():
                events.append("setup")
                yield object()
                events.append("teardown")

            @framework.fixture
            def consumer(request, resource):
                assert request.fixturename == "consumer"
                assert request.scope == "function"
                assert not hasattr(request, "param")
                assert request.getfixturevalue("request") is request
                assert request.getfixturevalue("resource") is resource
                assert {{"consumer", "request", "resource"}} <= set(request.fixturenames)
                assert request.function is test_request
                assert request.module is sys.modules[__name__]
                assert request.cls is None
                assert request.instance is None
                assert request.path == Path(__file__).resolve()
                assert request.node.name == "test_request"
                assert request.node.nodeid == "test.py::test_request"
                assert request.node.config is request.config
                assert request.node.session is request.session
                assert request.node.keywords is request.keywords
                request.addfinalizer(lambda: events.append("first"))
                request.addfinalizer(lambda: events.append("last"))
                yield 42
                assert request.getfixturevalue("resource") is resource
                events.append("yield")

            def test_request(request, consumer):
                assert consumer == 42
                assert request.fixturename is None
                assert request.scope == "function"
                assert request.getfixturevalue("consumer") is consumer
                request.addfinalizer(lambda: events.append("test"))

            def test_order():
                assert events == ["setup", "test", "yield", "last", "first", "teardown"]
        "#
    );
    let context = TestContext::with_files([("test.py", source.as_str())]);
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_request(request, consumer=42)
                PASS [TIME] test::test_order
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn fixture_request_resolves_dynamic_override_and_rejects_missing_names() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r"
                import karva

                @karva.fixture
                def item():
                    return 40
            ",
        ),
        (
            "test.py",
            r#"
                import karva
                import pytest

                @karva.fixture
                def item(request):
                    return request.getfixturevalue("item") + 2

                @karva.fixture(scope="module")
                def invalid_scope(request):
                    return request.getfixturevalue("item")

                @karva.fixture
                def recursive(request):
                    return request.getfixturevalue("through")

                @karva.fixture
                def through(recursive):
                    return recursive

                @karva.fixture
                def setup_failure():
                    raise ValueError("setup failed")

                def test_lookup(request):
                    assert "item" not in request.fixturenames
                    assert request.getfixturevalue("item") == 42
                    assert "item" in request.fixturenames
                    with pytest.raises(pytest.FixtureLookupError, match="not found"):
                        request.getfixturevalue("missing")
                    with karva.raises(karva.FixtureLookupError, match="ScopeMismatch"):
                        request.getfixturevalue("invalid_scope")
                    with karva.raises(karva.FixtureLookupError, match="recursive dependency"):
                        request.getfixturevalue("recursive")
                    with karva.raises(karva.FixtureLookupError, match="explain"):
                        request.raiseerror("explain")
                    with karva.raises(ValueError, match="setup failed"):
                        request.getfixturevalue("setup_failure")
            "#,
        ),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_lookup
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_preserves_fixture_callbacks_after_failed_setup() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import karva

            events = []

            @karva.fixture
            def broken(request):
                request.addfinalizer(lambda: events.append("first"))
                request.addfinalizer(lambda: events.append("last"))
                raise ValueError("setup broke")

            def test_broken(broken):
                raise AssertionError("the body ran")

            def test_cleanup():
                assert events == ["last", "first"]
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
           ERROR [TIME] test::test_broken
            PASS [TIME] test::test_cleanup

    failures:

    test::test_broken (requires fixture `broken`):

    error[fixture-failure]: Fixture `broken` failed
     --> test.py:7:5
      |
    7 | def broken(request):
      |     ^^^^^^
    info: Fixture ran with arguments:
    info:   `request`: `<FixtureRequest 'broken'>`
    info: Fixture failed here
      --> test.py:10:5
       |
    10 |     raise ValueError("setup broke")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: setup broke

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 error, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn fixture_request_reports_all_finalizer_failures_and_continues_cleanup() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import karva

            events = []

            def first():
                events.append("first")
                raise ValueError("first cleanup broke")

            def last():
                events.append("last")
                raise RuntimeError("last cleanup broke")

            @karva.fixture
            def resource(request):
                request.addfinalizer(first)
                request.addfinalizer(last)
                return 42

            def test_cleanup(resource):
                assert resource == 42

            def test_order():
                assert events == ["last", "first"]
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
           ERROR [TIME] test::test_cleanup(resource=42)
            PASS [TIME] test::test_order

    failures:

    test::test_cleanup(resource=42):

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `resource`
      --> test.py:15:5
       |
    15 | def resource(request):
       |     ^^^^^^^^
    info: Fixture finalizer failed: last cleanup broke

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `resource`
      --> test.py:15:5
       |
    15 | def resource(request):
       |     ^^^^^^^^
    info: Fixture finalizer failed: first cleanup broke

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_uses_scope_nodes_and_applies_runtime_markers() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
                import karva
                import pytest

                @karva.fixture(scope="session", auto_use=True)
                def session_info(request):
                    assert request.scope == "session"
                    assert not hasattr(request, "path")
                    assert not hasattr(request, "module")
                    assert request.node is request.session
                    assert request.config.rootpath.is_absolute()
                    request.applymarker(pytest.mark.session_marker(value=42))
            "#,
        ),
        (
            "test.py",
            r#"
                import karva
                import pytest

                @karva.fixture(scope="module")
                def module_info(request):
                    assert request.scope == "module"
                    assert not hasattr(request, "function")
                    assert not hasattr(request, "cls")
                    assert request.instance is None
                    assert request.module.__name__ == __name__
                    assert request.node.path == request.path
                    return request.node

                @karva.fixture
                def skip(request):
                    request.applymarker(pytest.mark.skip(reason="skipped during setup"))

                @karva.fixture
                def expect_fail(request):
                    request.applymarker(pytest.mark.xfail(reason="marked during setup"))

                def test_scope(request, module_info):
                    assert request.node.parent is module_info
                    assert request.node.get_closest_marker("session_marker").kwargs == {"value": 42}

                def test_skip(request, skip):
                    assert request.node.get_closest_marker("skip").kwargs["reason"] == "skipped during setup"

                def test_expected_failure(expect_fail):
                    assert False

                def test_expected_failure_from_body(request):
                    request.applymarker(pytest.mark.xfail(reason="marked during call"))
                    assert False
            "#,
        ),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 4 tests across 1 worker
            PASS [TIME] test::test_scope(request, module_info=<karva._fixtures.request.Re...)
            PASS [TIME] test::test_skip(request, skip=None)
            PASS [TIME] test::test_expected_failure(expect_fail=None)
            PASS [TIME] test::test_expected_failure_from_body(request)
    ────────────
         Summary [TIME] 4 tests run: 4 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn fixture_request_expands_fixture_parameters_with_direct_cases(
    #[values("karva", "pytest")] framework: &str,
) {
    let parametrize = if framework == "pytest" {
        "pytest.mark.parametrize"
    } else {
        "karva.tags.parametrize"
    };
    let source = format!(
        r#"
            import {framework}

            @{framework}.fixture(params=[None, 7], ids=["empty", "seven"])
            def number(request):
                assert hasattr(request, "param")
                return request.param

            @{framework}.fixture
            def dependency(number):
                return number

            @{parametrize}("mode", ["manual", "automatic"])
            def test_parameters(request, mode, dependency):
                assert mode in {{"manual", "automatic"}}
                assert dependency in {{None, 7}}
                assert request.getfixturevalue("mode") == mode
                assert request.getfixturevalue("number") is dependency
                assert request.node.name.startswith("test_parameters[")
        "#
    );
    let context = TestContext::with_files([("test.py", source.as_str())]);
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_parameters('manual'-empty)
                PASS [TIME] test::test_parameters('manual'-seven)
                PASS [TIME] test::test_parameters('automatic'-empty)
                PASS [TIME] test::test_parameters('automatic'-seven)
        ────────────
             Summary [TIME] 4 tests run: 4 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn fixture_request_handles_indirect_parameters_and_scoped_invalidation(
    #[values("karva", "pytest")] framework: &str,
) {
    let parametrize = if framework == "pytest" {
        "pytest.mark.parametrize"
    } else {
        "karva.tags.parametrize"
    };
    let source = format!(
        r#"
            import {framework}

            events = []

            @{framework}.fixture(scope="module")
            def resource(request):
                assert request.scope == "module"
                value = request.param
                events.append(("open", value))
                yield value * 10
                events.append(("close", value))

            @{framework}.fixture(scope="module")
            def dependent(request):
                value = request.getfixturevalue("resource")
                request.addfinalizer(lambda: events.append(("dependent", value)))
                return value

            @{parametrize}("resource, expected", [(1, 10), (2, 20)], indirect=["resource"], scope="module")
            def test_indirect(resource, expected, dependent):
                assert resource == expected == dependent
                if expected == 20:
                    assert events == [("open", 1), ("dependent", 10), ("close", 1), ("open", 2)]
        "#
    );
    let context = TestContext::with_files([("test.py", source.as_str())]);
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_indirect(1-10)
                PASS [TIME] test::test_indirect(2-20)
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn fixture_request_rejects_new_lookups_in_teardown() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import karva

            @karva.fixture
            def never_called():
                raise AssertionError("new fixture started during teardown")

            def test_new_lookup(request):
                def finalize():
                    with karva.raises(karva.FixtureLookupError, match="unavailable during teardown"):
                        request.getfixturevalue("never_called")
                request.addfinalizer(finalize)
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_new_lookup
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_runs_async_fixtures_and_rebuilds_on_retry() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import asyncio

            import karva

            events = []

            @karva.fixture
            async def resource(request):
                await asyncio.sleep(0)
                events.append("setup")
                request.addfinalizer(lambda: events.append("callback"))
                yield len(events)
                await asyncio.sleep(0)
                events.append("teardown")

            async def test_retry(request, resource):
                assert request.getfixturevalue("resource") == resource
                assert resource == 4

            def test_order():
                assert events == ["setup", "teardown", "callback"] * 2
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
      TRY 1 FAIL [TIME] test::test_retry(request, resource=1)
      TRY 2 PASS [TIME] test::test_retry(request, resource=1)
            PASS [TIME] test::test_order
    ────────────
         Summary [TIME] 2 tests run: 2 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(request, resource=1)

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_skips_empty_fixture_parameters() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import karva

            @karva.fixture(params=[])
            def empty():
                raise AssertionError("empty fixture was called")

            def test_empty(empty):
                raise AssertionError("test with no cases was called")
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_reports_an_unselected_parameter_and_missing_indirect_fixture() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import karva

            @karva.fixture(params=[1])
            def selected_only(request):
                return request.param

            def test_dynamic(request):
                request.getfixturevalue("selected_only")

            @karva.tags.parametrize("undefined", [1], indirect=True)
            def test_missing(undefined):
                raise AssertionError("the body ran")
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test::test_dynamic
           ERROR [TIME] test::test_missing

    failures:

    test::test_dynamic:

    error[test-failure]: Test `test_dynamic` failed
     --> test.py:8:5
      |
    8 | def test_dynamic(request):
      |     ^^^^^^^^^^^^
    info: Test ran with arguments:
    info:   `request`: `<FixtureRequest None>`
    info: Test failed here
     --> test.py:9:5
      |
    9 |     request.getfixturevalue("selected_only")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: fixture 'selected_only' has no parameter defined for this test

    test::test_missing:

    error[missing-fixtures]: Test `test_missing` has missing fixtures
      --> test.py:12:5
       |
    12 | def test_missing(undefined):
       |     ^^^^^^^^^^^^
    info: Missing fixtures: `undefined`

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 1 failed, 1 error, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn fixture_request_validates_an_indirect_dependents_effective_scope() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import karva
            import pytest

            @karva.fixture
            def dependency():
                return 40

            @karva.fixture(scope="module")
            def narrower(request, dependency):
                assert request.scope == "function"
                return request.param + dependency

            @karva.fixture
            def wider(request, dependency):
                raise AssertionError("body ran despite scope mismatch")

            @pytest.mark.parametrize("narrower", [2], indirect=True, scope="function")
            def test_narrower(narrower):
                assert narrower == 42

            @pytest.mark.parametrize("wider", [2], indirect=True, scope="module")
            def test_wider(wider):
                raise AssertionError("the test ran")
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_narrower(2)
           ERROR [TIME] test::test_wider(2)

    failures:

    test::test_wider(2) (requires fixture `wider`):

    error[fixture-failure]: Fixture `wider` failed
      --> test.py:15:5
       |
    15 | def wider(request, dependency):
       |     ^^^^^
    info: ScopeMismatch: module-scoped fixture 'wider' cannot access function-scoped fixture 'dependency'

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_allows_late_lookup_until_its_scope_ends() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import karva

            @karva.fixture(scope="module")
            def target():
                return 42

            @karva.fixture(scope="module")
            def deferred(request):
                return lambda: request.getfixturevalue("target")

            def test_first(deferred):
                assert callable(deferred)

            def test_second(deferred):
                assert deferred() == 42
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_first(deferred=<function deferred.<locals>...)
            PASS [TIME] test::test_second(deferred=<function deferred.<locals>...)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_exposes_mark_payloads_from_param_cases() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import pytest

            @pytest.mark.parametrize("value", [
                pytest.param(1, marks=pytest.mark.label("first", payload=1)),
                pytest.param(2, marks=pytest.mark.label("second", payload=2)),
            ])
            def test_params(value, request):
                marker = request.node.get_closest_marker("label")
                assert marker.args == (("first" if value == 1 else "second"),)
                assert marker.kwargs == {"payload": value}
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_params(value=1)
            PASS [TIME] test::test_params(value=2)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_preserves_legacy_broad_fixture_setup_recovery() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import karva

            calls = []

            @karva.fixture(scope="module")
            def flaky():
                calls.append(1)
                if len(calls) == 1:
                    raise ValueError("first call failed")
                return 42

            def test_first(flaky):
                raise AssertionError("the body ran")

            def test_second(flaky):
                assert flaky == 42
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
           ERROR [TIME] test::test_first
            PASS [TIME] test::test_second(flaky=42)

    failures:

    test::test_first (requires fixture `flaky`):

    error[fixture-failure]: Fixture `flaky` failed
     --> test.py:7:5
      |
    7 | def flaky():
      |     ^^^^^
    info: Fixture failed here
      --> test.py:10:9
       |
    10 |         raise ValueError("first call failed")
       |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: first call failed

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 error, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn fixture_request_keeps_an_unrelated_autouse_scope_failure() {
    let context = TestContext::with_files([
        (
            "test_a.py",
            r#"
            import karva

            calls = []

            @karva.fixture(scope="module", auto_use=True)
            def resource():
                calls.append(1)
                if len(calls) == 1:
                    raise ValueError("module setup failed")

            def test_first():
                raise AssertionError("the first body ran")

            def test_second():
                raise AssertionError("the second body ran")
        "#,
        ),
        (
            "test_b.py",
            r#"
            import karva

            @karva.fixture
            def resource(request):
                return request.param

            @karva.tags.parametrize("resource", [42], indirect=True)
            def test_resource(resource):
                assert resource == 42
        "#,
        ),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
           ERROR [TIME] test_a::test_first
           ERROR [TIME] test_a::test_second
            PASS [TIME] test_b::test_resource(42)

    failures:

    test_a::test_first (uses auto-use fixture `resource`):
    test_a::test_second (uses auto-use fixture `resource`):

    error[fixture-failure]: Fixture `resource` failed
     --> test_a.py:7:5
      |
    7 | def resource():
      |     ^^^^^^^^
    info: Fixture failed here
      --> test_a.py:10:9
       |
    10 |         raise ValueError("module setup failed")
       |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: module setup failed

    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 errors, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn fixture_request_uses_the_current_parameter_for_a_retained_lookup() {
    let context = TestContext::with_file(
        "test.py",
        r#"
        import karva

        @karva.fixture(scope="module")
        def target(request):
            return request.param

        @karva.fixture(scope="module")
        def lookup(request):
            return lambda: request.getfixturevalue("target")

        @karva.tags.parametrize("target", [[1]], indirect=True)
        def test_first(target, lookup):
            assert lookup() is target

        @karva.fixture
        def read_first(lookup):
            return lookup()

        @karva.tags.parametrize("target", [[2]], indirect=True)
        def test_second(read_first, target, lookup):
            assert target == [2]
            assert read_first is target
            assert lookup() is target
    "#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_first([1])
            PASS [TIME] test::test_second([2])
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_uses_the_current_parameter_for_a_shared_session_fixture() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
                import karva

                @karva.fixture(scope="session")
                def target(request):
                    return request.param

                @karva.fixture(scope="session")
                def lookup(request):
                    return lambda: request.getfixturevalue("target")
            "#,
        ),
        (
            "test_a.py",
            r#"
                import karva

                @karva.tags.parametrize("target", [[1]], indirect=True)
                def test_first(target, lookup):
                    assert lookup() is target
            "#,
        ),
        (
            "test_b.py",
            r#"
                import karva

                @karva.fixture
                def read_first(lookup):
                    return lookup()

                @karva.tags.parametrize("target", [[2]], indirect=True)
                def test_second(read_first, target, lookup):
                    assert target == [2]
                    assert read_first is target
                    assert lookup() is target
            "#,
        ),
        (
            "test_c.py",
            r#"
                import karva

                @karva.fixture(scope="session")
                def target(request):
                    return request.param

                @karva.tags.parametrize("target", [[3]], indirect=True)
                def test_shadowed(target, lookup):
                    assert target == [3]
                    assert lookup() == [2]
            "#,
        ),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test_a::test_first([1])
            PASS [TIME] test_b::test_second([2])
            PASS [TIME] test_c::test_shadowed([3])
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_waits_for_the_autouse_guard_before_a_narrowed_dependency() {
    let context = TestContext::with_file(
        "test.py",
        r#"
            import karva

            events = []

            @karva.fixture(auto_use=True)
            def guard():
                events.append("guard")

            @karva.fixture(scope="module")
            def resource(request):
                assert events == ["guard"]
                assert request.scope == "function"
                return request.param

            @karva.fixture(scope="module")
            def outer(resource):
                return resource

            @karva.tags.parametrize("resource", [42], indirect=True, scope="function")
            def test_order(outer):
                assert outer == 42
        "#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_order(42)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_stops_the_scope_prepass_at_an_incompatible_dependency() {
    let context = TestContext::with_file(
        "test.py",
        r#"
            import karva

            events = []

            @karva.fixture(auto_use=True)
            def guard():
                pass

            @karva.fixture
            def narrow():
                raise AssertionError("incompatible dependency was called")

            @karva.fixture(scope="session")
            def later():
                events.append("later")

            @karva.fixture
            def wider(request, narrow, later):
                raise AssertionError("fixture with invalid dependency was called")

            @karva.tags.parametrize("wider", [42], indirect=True, scope="module")
            def test_incompatible(wider):
                raise AssertionError("the test ran")

            @karva.fixture(scope="module")
            def blocked(request, narrow):
                raise AssertionError("fixture with invalid dependency was called")

            @karva.fixture(scope="module")
            def outer(blocked, later):
                raise AssertionError("outer fixture was called")

            @karva.tags.parametrize("blocked", [42], indirect=True)
            def test_nested_incompatible(outer):
                raise AssertionError("the test ran")

            def test_side_effects():
                assert events == []
        "#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
           ERROR [TIME] test::test_incompatible(42)
           ERROR [TIME] test::test_nested_incompatible(42)
            PASS [TIME] test::test_side_effects

    failures:

    test::test_incompatible(42) (requires fixture `wider`):

    error[fixture-failure]: Fixture `wider` failed
      --> test.py:19:5
       |
    19 | def wider(request, narrow, later):
       |     ^^^^^
    info: ScopeMismatch: module-scoped fixture 'wider' cannot access function-scoped fixture 'narrow'

    test::test_nested_incompatible(42) (uses fixture `blocked` via `use_fixtures`; requires fixture `outer`):

    error[fixture-failure]: Fixture `blocked` failed
      --> test.py:27:5
       |
    27 | def blocked(request, narrow):
       |     ^^^^^^^
    info: ScopeMismatch: module-scoped fixture 'blocked' cannot access function-scoped fixture 'narrow'

    error[fixture-failure]: Fixture `blocked` failed
      --> test.py:27:5
       |
    27 | def blocked(request, narrow):
       |     ^^^^^^^
    info: Fixture `outer` requires `blocked`
      --> test.py:31:5
       |
    31 | def outer(blocked, later):
       |     ^^^^^
    info: ScopeMismatch: module-scoped fixture 'blocked' cannot access function-scoped fixture 'narrow'

    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 errors, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn fixture_request_combines_indirect_and_fixture_only_direct_parameters(
    #[values("karva", "pytest")] framework: &str,
) {
    let parametrize = if framework == "pytest" {
        "pytest.mark.parametrize"
    } else {
        "karva.tags.parametrize"
    };
    let source = format!(
        r#"
            import {framework}

            @{framework}.fixture
            def value():
                return 5

            @{framework}.fixture
            def lazy(value):
                return value * 2

            @{framework}.fixture(scope="module")
            def resource(request, value):
                assert request.scope == "function"
                assert request.getfixturevalue("value") is value
                return request.param + request.getfixturevalue("lazy")

            @{parametrize}("resource, value", [(1, 10), (2, 20)], indirect=["resource"], scope="function")
            def test_indirect(resource, request):
                assert resource in (21, 42)
                assert request.getfixturevalue("lazy") == (resource // 21) * 20

            def test_default(request):
                assert request.getfixturevalue("lazy") == 10
        "#
    );

    let context = TestContext::with_files([("test.py", source.as_str())]);
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_indirect(1-10)
                PASS [TIME] test::test_indirect(2-20)
                PASS [TIME] test::test_default
        ────────────
             Summary [TIME] 3 tests run: 3 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn fixture_request_validates_direct_scope_even_after_module_cache_is_primed() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import karva

            @karva.fixture(scope="module")
            def value():
                return "default"

            @karva.fixture(scope="module")
            def resource(value):
                return value

            def test_prime(resource):
                assert resource == "default"

            @karva.tags.parametrize("value", ["override"])
            def test_mismatch(resource):
                assert False, "the test should fail during fixture validation"

            @karva.tags.parametrize("resource", [None], indirect=True, scope="function")
            def test_narrowed(resource):
                assert resource == "default"
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_prime(resource='default')
           ERROR [TIME] test::test_mismatch
            PASS [TIME] test::test_narrowed(None)

    failures:

    test::test_mismatch:

    error[fixture-scope-mismatch]: Fixture `resource` with `module` scope cannot depend on parametrized argument `value` with `function` scope
     --> test.py:9:5
      |
    9 | def resource(value):
      |     ^^^^^^^^

    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_retained_lookup_respects_active_direct_parameter_scope() {
    let context = TestContext::with_files([(
        "test.py",
        r#"
            import karva
            import pytest

            @karva.fixture(scope="module")
            def value():
                return "default"

            @karva.fixture(scope="module")
            def resource(value):
                return value

            lookup = None

            @karva.fixture(scope="module", auto_use=True)
            def retain_lookup(request):
                global lookup
                lookup = request.getfixturevalue

            def test_prime():
                assert lookup("value") == "default"
                assert lookup("resource") == "default"

            @karva.tags.parametrize("value", ["override"])
            def test_reject_direct(value):
                assert value == "override"
                with pytest.raises(pytest.FixtureLookupError, match="function-scoped parameter 'value'"):
                    lookup("value")
                with pytest.raises(pytest.FixtureLookupError, match="parametrized argument `value`"):
                    lookup("resource")

            def test_restore():
                assert lookup("value") == "default"
                assert lookup("resource") == "default"
        "#,
    )]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_prime
            PASS [TIME] test::test_reject_direct(value='override')
            PASS [TIME] test::test_restore
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_request_retained_session_lookup_respects_direct_overrides_across_modules() {
    let context = TestContext::with_files([
        ("shared.py", "lookup = None\n"),
        (
            "conftest.py",
            r#"
                import karva
                import shared

                @karva.fixture(scope="session")
                def value():
                    return "default"

                @karva.fixture(scope="session")
                def resource(value):
                    return value

                @karva.fixture(scope="session")
                def lookup(request):
                    shared.lookup = request.getfixturevalue
            "#,
        ),
        (
            "test_a.py",
            r#"
                import karva
                import shared

                @karva.tags.use_fixtures("lookup")
                def test_prime():
                    assert shared.lookup("value") == "default"
                    assert shared.lookup("resource") == "default"
            "#,
        ),
        (
            "test_b.py",
            r#"
                import karva
                import pytest
                import shared

                @karva.tags.parametrize("value", ["override"])
                def test_override(value):
                    assert value == "override"
                    with pytest.raises(pytest.FixtureLookupError, match="function-scoped parameter 'value'"):
                        shared.lookup("value")
                    with pytest.raises(pytest.FixtureLookupError, match="parametrized argument `value`"):
                        shared.lookup("resource")
            "#,
        ),
        (
            "test_c.py",
            r#"
                import karva
                import shared

                @karva.fixture(scope="session")
                def value():
                    return "local"

                @karva.tags.parametrize("value", ["override"])
                def test_distinct(value):
                    assert value == "override"
                    assert shared.lookup("value") == "default"
            "#,
        ),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test_a::test_prime
            PASS [TIME] test_b::test_override(value='override')
            PASS [TIME] test_c::test_distinct(value='override')
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn fixture_request_keeps_imported_fixture_parameters(#[values("karva", "pytest")] framework: &str) {
    let fixtures = format!(
        r#"
            import {framework}

            @{framework}.fixture(params=[10, 20], ids=["ten", "twenty"])
            def original(request):
                assert request.fixturename == "renamed"
                return request.param
        "#
    );
    let context = TestContext::with_files([
        ("helpers.py", fixtures.as_str()),
        (
            "test_import.py",
            r#"
                from helpers import original as renamed

                def test_imported(renamed, request):
                    assert renamed in (10, 20)
                    assert request.getfixturevalue("renamed") == renamed
            "#,
        ),
    ]);
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test_import::test_imported(ten)
                PASS [TIME] test_import::test_imported(twenty)
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}
