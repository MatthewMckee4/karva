use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn reports_captured_operands_without_repeating_side_effects() {
    let context = TestContext::with_file(
        "test_assertions.py",
        r#"
calls = 0

def actual():
    global calls
    calls += 1
    return {"active": False}

def test_failure():
    assert actual() == {"active": True}, "state mismatch"

def test_call_count():
    assert calls == 1
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test_assertions::test_failure
            PASS [TIME] test_assertions::test_call_count

    failures:

    test_assertions::test_failure:

    error[test-failure]: Test `test_failure` failed
     --> test_assertions.py:9:5
      |
    9 | def test_failure():
      |     ^^^^^^^^^^^^
    info: Test failed here
      --> test_assertions.py:10:5
       |
    10 |     assert actual() == {"active": True}, "state mismatch"
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: state mismatch
    info: assert actual() == {"active": True}
          
          Differing values:
            actual['active']: False
            expected['active']: True

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn preserves_async_fixture_and_short_circuit_semantics() {
    let context = TestContext::with_file(
        "test_assertions_async.py",
        r"
import asyncio
import karva

@karva.fixture
async def expected():
    await asyncio.sleep(0)
    return [1, 2, 4]

async def test_failure(expected):
    side_effect = False
    assert side_effect and expected == [1, 2, 3]
",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertions_async::test_failure(expected=[1, 2, 4])

    failures:

    test_assertions_async::test_failure(expected=[1, 2, 4]):

    error[test-failure]: Test `test_failure` failed
      --> test_assertions_async.py:10:11
       |
    10 | async def test_failure(expected):
       |           ^^^^^^^^^^^^
    info: Test ran with arguments:
    info:   `expected`: `[1, 2, 4]`
    info: Test failed here
      --> test_assertions_async.py:12:5
       |
    12 |     assert side_effect and expected == [1, 2, 3]
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: assert side_effect and expected == [1, 2, 3]
          
          Differing values:
            side_effect: False

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn explains_identity_membership_truthiness_and_preserves_custom_args() {
    let context = TestContext::with_file(
        "test_assertion_forms.py",
        r#"
def test_identity():
    assert object() is object()

class Unprintable:
    def __repr__(self):
        raise RuntimeError("repr must not run")

def test_safe_repr():
    assert Unprintable() == Unprintable()

def test_membership():
    assert "needle" in ("haystack",)

def test_truthiness():
    assert []

def test_custom_message_args():
    sentinel = object()
    try:
        assert False, sentinel
    except AssertionError as error:
        assert error.args == (sentinel,)
    else:
        raise AssertionError("the assertion should fail")
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 5 tests across 1 worker
            FAIL [TIME] test_assertion_forms::test_identity
            FAIL [TIME] test_assertion_forms::test_safe_repr
            FAIL [TIME] test_assertion_forms::test_membership
            FAIL [TIME] test_assertion_forms::test_truthiness
            PASS [TIME] test_assertion_forms::test_custom_message_args

    failures:

    test_assertion_forms::test_identity:

    error[test-failure]: Test `test_identity` failed
     --> test_assertion_forms.py:2:5
      |
    2 | def test_identity():
      |     ^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_forms.py:3:5
      |
    3 |     assert object() is object()
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: assert object() is object()
          
          Differing values:
            object(): <builtins.object object> (distinct objects)
            object(): <builtins.object object> (distinct objects)

    test_assertion_forms::test_membership:

    error[test-failure]: Test `test_membership` failed
      --> test_assertion_forms.py:12:5
       |
    12 | def test_membership():
       |     ^^^^^^^^^^^^^^^
    info: Test failed here
      --> test_assertion_forms.py:13:5
       |
    13 |     assert "needle" in ("haystack",)
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: assert "needle" in ("haystack",)

    test_assertion_forms::test_safe_repr:

    error[test-failure]: Test `test_safe_repr` failed
     --> test_assertion_forms.py:9:5
      |
    9 | def test_safe_repr():
      |     ^^^^^^^^^^^^^^
    info: Test failed here
      --> test_assertion_forms.py:10:5
       |
    10 |     assert Unprintable() == Unprintable()
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: assert Unprintable() == Unprintable()
          
          Differing values:
            Unprintable(): <test_assertion_forms.Unprintable object>

    test_assertion_forms::test_truthiness:

    error[test-failure]: Test `test_truthiness` failed
      --> test_assertion_forms.py:15:5
       |
    15 | def test_truthiness():
       |     ^^^^^^^^^^^^^^^
    info: Test failed here
      --> test_assertion_forms.py:16:5
       |
    16 |     assert []
       |     ^^^^^^^^^
    info: assert []

    ────────────
         Summary [TIME] 5 tests run: 1 passed, 4 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn formatter_failures_preserve_native_assertions() {
    let context = TestContext::with_files([
        ("helpers/__init__.py", "_karva_message_ = object()\n"),
        ("helpers/data.txt", "payload\n"),
        (
            "test_assertion_formatter.py",
            r#"
from importlib.resources import files

from helpers import _karva_message_

def test_package_resources():
    assert files("helpers").joinpath("data.txt").read_text() == "payload\n"

def test_native_args():
    try:
        assert False
    except AssertionError as error:
        assert error.args == ()

def test_passing_assert_releases_operands():
    released = []

    class Temporary:
        def __del__(self):
            released.append(True)

        def __eq__(self, other):
            return True

    assert Temporary() == None
    assert released == [True]

def test_deep_value():
    value = 0
    for _ in range(1500):
        value = [value]
    assert value == None

def test_huge_integer():
    assert 10**10000 == None

class HostileMeta(type):
    def __getattribute__(cls, name):
        if name in ("__module__", "__qualname__"):
            raise RuntimeError("metadata must not run user code")
        return super().__getattribute__(name)

class Hostile(metaclass=HostileMeta):
    pass

def test_hostile_metadata():
    assert Hostile() == Hostile()
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 6 tests across 1 worker
            PASS [TIME] test_assertion_formatter::test_package_resources
            PASS [TIME] test_assertion_formatter::test_native_args
            PASS [TIME] test_assertion_formatter::test_passing_assert_releases_operands
            FAIL [TIME] test_assertion_formatter::test_deep_value
            FAIL [TIME] test_assertion_formatter::test_huge_integer
            FAIL [TIME] test_assertion_formatter::test_hostile_metadata

    failures:

    test_assertion_formatter::test_deep_value:

    error[test-failure]: Test `test_deep_value` failed
      --> test_assertion_formatter.py:28:5
       |
    28 | def test_deep_value():
       |     ^^^^^^^^^^^^^^^
    info: Test failed here
      --> test_assertion_formatter.py:32:5
       |
    32 |     assert value == None
       |     ^^^^^^^^^^^^^^^^^^^^
    info: assert value == None
          
          Differing values:
            value: [[[[[[[[[<builtins.list object>]]]]]]]]]

    test_assertion_formatter::test_hostile_metadata:

    error[test-failure]: Test `test_hostile_metadata` failed
      --> test_assertion_formatter.py:46:5
       |
    46 | def test_hostile_metadata():
       |     ^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
      --> test_assertion_formatter.py:47:5
       |
    47 |     assert Hostile() == Hostile()
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: assert Hostile() == Hostile()
          
          Differing values:
            Hostile(): <test_assertion_formatter.Hostile object>

    test_assertion_formatter::test_huge_integer:

    error[test-failure]: Test `test_huge_integer` failed
      --> test_assertion_formatter.py:34:5
       |
    34 | def test_huge_integer():
       |     ^^^^^^^^^^^^^^^^^
    info: Test failed here
      --> test_assertion_formatter.py:35:5
       |
    35 |     assert 10**10000 == None
       |     ^^^^^^^^^^^^^^^^^^^^^^^^
    info: assert 10**10000 == None
          
          Differing values:
            10**10000: <int 33220 bits>

    ────────────
         Summary [TIME] 6 tests run: 3 passed, 3 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn optimized_assertions_remove_cleanup_with_assertions() {
    let context = TestContext::with_file(
        "test_optimized_assertions.py",
        "def test_optimized_assertion():\n    assert False\n",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .env("PYTHONOPTIMIZE", "1"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_optimized_assertions::test_optimized_assertion
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
}
