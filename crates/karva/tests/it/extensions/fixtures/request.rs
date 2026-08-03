use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn pytest_parametrized_fixture_receives_request() {
    let context = TestContext::with_file(
        "test_request.py",
        r#"
import pytest


@pytest.fixture(params=["python", "json"], ids=lambda value: f"mode-{value}")
def mode(request):
    assert isinstance(request, pytest.FixtureRequest)
    assert request.fixturename == "mode"
    assert request.scope == "function"
    assert request.param_index in (0, 1)
    assert request.node.name == f"test_mode[mode-{request.param}]"
    assert request.node.originalname == "test_mode"
    assert request.node.nodeid.endswith(f"test_request.py::test_mode[mode-{request.param}]")
    return request.param


def test_mode(mode):
    assert mode in {"python", "json"}
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_request::test_mode(mode-python)
            PASS [TIME] test_request::test_mode(mode-json)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn request_exposes_context_dynamic_lookup_markers_and_finalizers() {
    let context = TestContext::with_file(
        "test_request.py",
        r#"
from pathlib import Path

import pytest


events = []


@pytest.fixture
def base():
    return 42


@pytest.fixture
def resource(request):
    assert not hasattr(request, "param")
    assert request.param_index == 0
    request.addfinalizer(lambda: events.append("finalized"))
    request.applymarker(pytest.mark.requested)
    return request.getfixturevalue("base")


@pytest.mark.initial
def test_request(resource, request):
    assert resource == 42
    assert request.fixturename is None
    assert request.scope == "function"
    assert {"base", "request", "resource"} <= set(request.fixturenames)
    assert request.function is test_request
    assert request.cls is None
    assert request.instance is None
    assert request.module is __import__(__name__)
    assert isinstance(request.path, Path)
    assert request.path.name == "test_request.py"
    assert request.node.name == "test_request"
    assert request.node.originalname == "test_request"
    assert request.node.nodeid.endswith("test_request.py::test_request")
    assert request.node.get_closest_marker("requested").name == "requested"
    assert request.node.get_closest_marker("initial").name == "initial"
    assert next(request.node.iter_markers()).name == "initial"
    assert {marker.name for marker in request.node.iter_markers()} == {"initial", "requested"}
    assert "requested" in request.keywords
    assert "initial" in request.keywords
    assert request.config is request.session.config
    assert isinstance(request.config.rootpath, Path)
    assert request.getfixturevalue("resource") == 42
    with pytest.raises(pytest.FixtureLookupError):
        request.getfixturevalue("missing")
    with pytest.raises(pytest.FixtureLookupError, match="deliberate"):
        request.raiseerror("deliberate")


def test_finalizer_ran():
    assert events == ["finalized"]
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_request::test_request(resource=42, request)
            PASS [TIME] test_request::test_finalizer_ran
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn indirect_parametrization_sets_request_param() {
    let context = TestContext::with_file(
        "test_request.py",
        r#"
import pytest


@pytest.fixture
def doubled(request):
    return request.param * 2


@pytest.mark.parametrize("doubled", [2, 3], indirect=True)
def test_indirect(doubled):
    assert doubled in (4, 6)
"#,
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_request::test_indirect(doubled=4)
            PASS [TIME] test_request::test_indirect(doubled=6)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn request_respects_scope_and_finalizer_order() {
    let context = TestContext::with_file(
        "test_request.py",
        r#"
import pytest


events = []
module_requests = []


@pytest.fixture(scope="module", autouse=True)
def module_request(request):
    module_requests.append(request.module.__name__)


@pytest.fixture(scope="session")
def session_value(request):
    assert request.scope == "session"
    assert request.fixturename == "session_value"
    assert request.instance is None
    with pytest.raises(AttributeError):
        request.function
    with pytest.raises(AttributeError):
        request.cls
    with pytest.raises(AttributeError):
        request.module
    with pytest.raises(AttributeError):
        request.path
    return "session"


@pytest.fixture(scope="module")
def module_value(request):
    assert request.scope == "module"
    assert request.module is __import__(__name__)
    assert request.path.name == "test_request.py"
    with pytest.raises(AttributeError):
        request.function
    return "module"


@pytest.fixture
def child(request):
    request.addfinalizer(lambda: events.append("child callback"))
    yield "child"
    events.append("child yield")


@pytest.fixture
def resource(request):
    request.addfinalizer(lambda: events.append("callback"))
    session_value = request.getfixturevalue("session_value")
    module_value = request.getfixturevalue("module_value")
    child = request.getfixturevalue("child")
    yield (session_value, module_value, child)
    assert session_value == "session"
    events.append("yield")


def test_scopes(resource, request):
    assert resource == ("session", "module", "child")
    assert module_requests == [__name__]
    request.addfinalizer(lambda: events.append("test"))


def test_finalizers():
    assert events == ["test", "yield", "child yield", "child callback", "callback"]
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_request::test_scopes(resource=('session', 'module', 'child'), request)
            PASS [TIME] test_request::test_finalizers
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn fixture_parameters_combine_and_apply_parameter_marks() {
    let context = TestContext::with_file(
        "test_request.py",
        r#"
import karva
import pytest


@pytest.fixture(params=[pytest.param(1, id="one"), pytest.param(2, marks=pytest.mark.skip)])
def number(request):
    return request.param


@karva.fixture(params=["a", "b"])
def letter(request):
    return request.param


def test_product(number, letter):
    assert number == 1
    assert letter in ("a", "b")
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_request::test_product(one-'a')
            PASS [TIME] test_request::test_product(one-'b')
    ────────────
         Summary [TIME] 4 tests run: 2 passed, 2 skipped

    ----- stderr -----
    ");
}

#[test]
fn partial_indirect_parametrization_only_routes_named_fixtures() {
    let context = TestContext::with_file(
        "test_request.py",
        r#"
import pytest


@pytest.fixture
def value(request):
    return request.param.upper()


@pytest.mark.parametrize("value,expected", [("hello", "HELLO")], indirect=["value"])
def test_partial(value, expected):
    assert value == expected
"#,
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_request::test_partial(value='HELLO', expected='HELLO')
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn indirect_parametrization_overrides_fixture_parameters() {
    let context = TestContext::with_file(
        "test_request.py",
        r#"
import pytest


@pytest.fixture(params=["decorator"])
def source(request):
    return request.param


@pytest.mark.parametrize("source", ["indirect"], indirect=True)
def test_source(source):
    assert source == "indirect"
"#,
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_request::test_source(source='indirect')
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn scoped_parameters_replace_the_active_fixture_instance() {
    let context = TestContext::with_file(
        "test_request.py",
        r#"
import pytest


events = []


@pytest.fixture(scope="module", params=["one", "two"])
def scoped(request):
    value = request.param
    events.append(f"setup {value}")
    request.addfinalizer(lambda: events.append(f"teardown {value}"))
    return value


def test_parameter_lifetime(scoped):
    if scoped == "one":
        assert events == ["setup one"]
    else:
        assert events == ["setup one", "teardown one", "setup two"]


dependency_events = []


@pytest.fixture(scope="module", params=[1, 2])
def dependency(request):
    value = request.param
    dependency_events.append(f"dependency setup {value}")
    yield value
    dependency_events.append(f"dependency teardown {value}")


@pytest.fixture(scope="module")
def dependent(dependency):
    dependency_events.append(f"dependent setup {dependency}")
    yield dependency
    dependency_events.append(f"dependent teardown {dependency}")


def test_dependency_lifetime(dependency, dependent):
    assert dependency == dependent
    if dependency == 2:
        assert dependency_events == [
            "dependency setup 1",
            "dependent setup 1",
            "dependent teardown 1",
            "dependency teardown 1",
            "dependency setup 2",
            "dependent setup 2",
        ]


@pytest.fixture(scope="module")
def indirect_value(request):
    return request.param


@pytest.mark.parametrize("indirect_value", ["first"], indirect=True)
def test_first_indirect(indirect_value):
    assert indirect_value == "first"


@pytest.mark.parametrize("indirect_value", ["second"], indirect=True)
def test_second_indirect(indirect_value):
    assert indirect_value == "second"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 4 tests across 1 worker
            PASS [TIME] test_request::test_parameter_lifetime('one')
            PASS [TIME] test_request::test_parameter_lifetime('two')
            PASS [TIME] test_request::test_dependency_lifetime(1)
            PASS [TIME] test_request::test_dependency_lifetime(2)
            PASS [TIME] test_request::test_first_indirect(indirect_value='first')
            PASS [TIME] test_request::test_second_indirect(indirect_value='second')
    ────────────
         Summary [TIME] 6 tests run: 6 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn request_applied_xfail_and_marker_order_match_pytest() {
    let context = TestContext::with_file(
        "test_request.py",
        r#"
import pytest


@pytest.fixture
def expected_failure(request):
    request.applymarker(pytest.mark.xfail(reason="dynamic"))


@pytest.mark.label("outer")
@pytest.mark.label("inner")
def test_dynamic_marker(expected_failure, request):
    assert [marker.args[0] for marker in request.node.iter_markers("label")] == [
        "inner",
        "outer",
    ]
    assert request.node.get_closest_marker("label").args == ("inner",)
    assert False
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_request::test_dynamic_marker(expected_failure=None, request)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn request_registers_with_pytest_before_a_late_import() {
    let context = TestContext::with_file(
        "test_request.py",
        r"
def test_request_type(request):
    import pytest

    assert isinstance(request, pytest.FixtureRequest)
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_request::test_request_type(request)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn dynamic_lookup_uses_overridden_parent_and_deferred_autouse_fixture() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import pytest


events = []


@pytest.fixture(scope="session", autouse=True)
def initialize(request):
    events.append(request.scope)


@pytest.fixture
def value():
    return "parent"
"#,
        ),
        (
            "nested/conftest.py",
            r#"
import pytest


@pytest.fixture
def value(request):
    return request.getfixturevalue("value") + " nested"
"#,
        ),
        (
            "nested/test_request.py",
            r#"
import pytest

from conftest import events


@pytest.fixture
def value(request):
    return request.getfixturevalue("value") + " module"


def test_override(value):
    assert value == "parent nested module"
    assert events == ["session"]
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] nested.test_request::test_override(value='parent nested module')
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}
