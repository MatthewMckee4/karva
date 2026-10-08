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
    info: Difference at ['active']:
      left: False
      right: True

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
    info: Evaluated values:
      side_effect = False

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_identity_reports_distinct_objects() {
    let context = TestContext::with_file(
        "test_assertion_identity.py",
        "def test_identity():\n    assert object() is object()\n",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_identity::test_identity

    failures:

    test_assertion_identity::test_identity:

    error[test-failure]: Test `test_identity` failed
     --> test_assertion_identity.py:1:5
      |
    1 | def test_identity():
      |     ^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_identity.py:2:5
      |
    2 |     assert object() is object()
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Distinct objects (both rendered as <builtins.object object>)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_safe_repr_avoids_user_repr() {
    let context = TestContext::with_file(
        "test_assertion_safe_repr.py",
        r#"
class Unprintable:
    def __repr__(self):
        raise RuntimeError("repr must not run")

def test_safe_repr():
    assert Unprintable() == Unprintable()
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_safe_repr::test_safe_repr

    failures:

    test_assertion_safe_repr::test_safe_repr:

    error[test-failure]: Test `test_safe_repr` failed
     --> test_assertion_safe_repr.py:6:5
      |
    6 | def test_safe_repr():
      |     ^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_safe_repr.py:7:5
      |
    7 |     assert Unprintable() == Unprintable()
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      left (Unprintable()) = <test_assertion_safe_repr.Unprintable object>
      right (Unprintable()) = <test_assertion_safe_repr.Unprintable object>

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_membership_preserves_native_failure() {
    let context = TestContext::with_file(
        "test_assertion_membership.py",
        "def test_membership():\n    assert \"needle\" in (\"haystack\",)\n",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_membership::test_membership

    failures:

    test_assertion_membership::test_membership:

    error[test-failure]: Test `test_membership` failed
     --> test_assertion_membership.py:1:5
      |
    1 | def test_membership():
      |     ^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_membership.py:2:5
      |
    2 |     assert "needle" in ("haystack",)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn assertion_truthiness_preserves_native_failure() {
    let context = TestContext::with_file(
        "test_assertion_truthiness.py",
        "def test_truthiness():\n    assert []\n",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_truthiness::test_truthiness

    failures:

    test_assertion_truthiness::test_truthiness:

    error[test-failure]: Test `test_truthiness` failed
     --> test_assertion_truthiness.py:1:5
      |
    1 | def test_truthiness():
      |     ^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_truthiness.py:2:5
      |
    2 |     assert []
      |     ^^^^^^^^^

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_repeated_boolean_operand_reports_failing_call() {
    let context = TestContext::with_file(
        "test_assertion_boolean.py",
        r"
calls = []

def next_value():
    calls.append(len(calls))
    return bool(calls[-1])

def test_repeated_boolean_operand():
    assert next_value() and next_value()
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_boolean::test_repeated_boolean_operand

    failures:

    test_assertion_boolean::test_repeated_boolean_operand:

    error[test-failure]: Test `test_repeated_boolean_operand` failed
     --> test_assertion_boolean.py:8:5
      |
    8 | def test_repeated_boolean_operand():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_boolean.py:9:5
      |
    9 |     assert next_value() and next_value()
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      next_value() = False

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_custom_message_preserves_native_args() {
    let context = TestContext::with_file(
        "test_assertion_message.py",
        r#"
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
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_assertion_message::test_custom_message_args
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_loader_preserves_resources_and_imports() {
    let context = TestContext::with_files([
        ("helpers/__init__.py", "_karva_message_ = object()\n"),
        ("helpers/data.txt", "payload\n"),
        (
            "helpers/import_assertion.py",
            "value = 1\nassert value == 2\n",
        ),
        (
            "test_assertion_loader.py",
            r#"
from importlib.resources import files

def test_package_resources():
    assert files("helpers").joinpath("data.txt").read_text() == "payload\n"

def test_module_import_assertion():
    import helpers.import_assertion
"#,
        ),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_assertion_loader::test_package_resources
            FAIL [TIME] test_assertion_loader::test_module_import_assertion

    failures:

    test_assertion_loader::test_module_import_assertion:

    error[test-failure]: Test `test_module_import_assertion` failed
     --> test_assertion_loader.py:7:5
      |
    7 | def test_module_import_assertion():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> helpers/import_assertion.py:2:1
      |
    2 | assert value == 2
      | ^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      value = 1

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_native_args_remain_empty() {
    let context = TestContext::with_file(
        "test_assertion_native_args.py",
        r#"
def test_native_args():
    try:
        assert False
    except AssertionError as error:
        assert error.args == ()
    else:
        raise AssertionError("the assertion should fail")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_assertion_native_args::test_native_args
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_passing_releases_operands() {
    let context = TestContext::with_file(
        "test_assertion_lifetime.py",
        r"
released = []

class Temporary:
    def __del__(self):
        released.append(True)

    def __eq__(self, other):
        return True

def test_passing_assert_releases_operands():
    assert Temporary() == None
    assert released == [True]
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_assertion_lifetime::test_passing_assert_releases_operands
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_annotations_do_not_inherit_karva_future_imports() {
    let context = TestContext::with_file(
        "test_assertion_annotations.py",
        r#"
def annotated(value: int) -> int:
    return value

def test_compiler_does_not_inherit_karva_annotations():
    assert annotated.__annotations__ == {"value": int, "return": int}
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_assertion_annotations::test_compiler_does_not_inherit_karva_annotations
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_one_line_suite_cleanup_preserves_scope() {
    let context = TestContext::with_file(
        "test_assertion_one_line.py",
        r"
def annotated(value: int) -> int:
    return value

def test_one_line_suite_cleanup():
    if True: assert annotated(1) == 1
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_assertion_one_line::test_one_line_suite_cleanup
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_same_line_second_assertion_selects_failure() {
    let context = TestContext::with_file(
        "test_assertion_same_line.py",
        r"
def test_same_line_second_assertion():
    first = 1
    second = 2
    if True: assert first == 1; assert second == 3
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_same_line::test_same_line_second_assertion

    failures:

    test_assertion_same_line::test_same_line_second_assertion:

    error[test-failure]: Test `test_same_line_second_assertion` failed
     --> test_assertion_same_line.py:2:5
      |
    2 | def test_same_line_second_assertion():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_same_line.py:5:5
      |
    5 |     if True: assert first == 1; assert second == 3
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      second = 2

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_string_difference_is_focused() {
    let context = TestContext::with_file(
        "test_assertion_string.py",
        r#"
def test_string_diff():
    assert "alpha\nbravo" == "alpha\nbrava"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_string::test_string_diff

    failures:

    test_assertion_string::test_string_diff:

    error[test-failure]: Test `test_string_diff` failed
     --> test_assertion_string.py:2:5
      |
    2 | def test_string_diff():
      |     ^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_string.py:3:5
      |
    3 |     assert "alpha/nbravo" == "alpha/nbrava"
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: String difference (- left, + right):
    1 │  alpha
    2 │ -bravo
    2 │ +brava

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn assertion_bytes_difference_is_focused() {
    let context = TestContext::with_file(
        "test_assertion_bytes.py",
        r#"
def test_bytes_diff():
    assert b"alpha" == b"alphi"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_bytes::test_bytes_diff

    failures:

    test_assertion_bytes::test_bytes_diff:

    error[test-failure]: Test `test_bytes_diff` failed
     --> test_assertion_bytes.py:2:5
      |
    2 | def test_bytes_diff():
      |     ^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_bytes.py:3:5
      |
    3 |     assert b"alpha" == b"alphi"
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Difference at [4]:
      left: 97
      right: 105

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn assertion_set_difference_is_focused() {
    let context = TestContext::with_file(
        "test_assertion_set.py",
        r#"
def test_set_diff():
    assert {"alpha"} == {"beta"}
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_set::test_set_diff

    failures:

    test_assertion_set::test_set_diff:

    error[test-failure]: Test `test_set_diff` failed
     --> test_assertion_set.py:2:5
      |
    2 | def test_set_diff():
      |     ^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_set.py:3:5
      |
    3 |     assert {"alpha"} == {"beta"}
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Set difference:
      left only: 'alpha'
      right only: 'beta'

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn assertion_mapping_difference_identifies_missing_key() {
    let context = TestContext::with_file(
        "test_assertion_mapping.py",
        r#"
def test_mapping_missing_key():
    assert {"active": True} == {"enabled": True}
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_mapping::test_mapping_missing_key

    failures:

    test_assertion_mapping::test_mapping_missing_key:

    error[test-failure]: Test `test_mapping_missing_key` failed
     --> test_assertion_mapping.py:2:5
      |
    2 | def test_mapping_missing_key():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_mapping.py:3:5
      |
    3 |     assert {"active": True} == {"enabled": True}
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Difference at ['active']:
      left: True
      right: <missing>

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn assertion_sequence_difference_reports_length() {
    let context = TestContext::with_file(
        "test_assertion_sequence.py",
        r"
def test_sequence_length_diff():
    assert [1, 2] == [1, 2, 3]
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_sequence::test_sequence_length_diff

    failures:

    test_assertion_sequence::test_sequence_length_diff:

    error[test-failure]: Test `test_sequence_length_diff` failed
     --> test_assertion_sequence.py:2:5
      |
    2 | def test_sequence_length_diff():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_sequence.py:3:5
      |
    3 |     assert [1, 2] == [1, 2, 3]
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Different lengths:
      left: 2
      right: 3

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_nested_difference_reports_path() {
    let context = TestContext::with_file(
        "test_assertion_nested.py",
        r#"
def test_nested_diff():
    assert {"outer": {"items": [1, 2]}} == {"outer": {"items": [1, 3]}}
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_nested::test_nested_diff

    failures:

    test_assertion_nested::test_nested_diff:

    error[test-failure]: Test `test_nested_diff` failed
     --> test_assertion_nested.py:2:5
      |
    2 | def test_nested_diff():
      |     ^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_nested.py:3:5
      |
    3 |     assert {"outer": {"items": [1, 2]}} == {"outer": {"items": [1, 3]}}
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Difference at ['outer']['items'][1]:
      left: 2
      right: 3

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn assertion_nested_difference_skips_equal_prefix() {
    let context = TestContext::with_file(
        "test_assertion_nested_prefix.py",
        r#"
def test_nested_diff_skips_equal_prefix():
    assert {"outer": {"same": {"x": 1}, "changed": [1, 2]}} == {"outer": {"same": {"x": 1}, "changed": [1, 3]}}
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_nested_prefix::test_nested_diff_skips_equal_prefix

    failures:

    test_assertion_nested_prefix::test_nested_diff_skips_equal_prefix:

    error[test-failure]: Test `test_nested_diff_skips_equal_prefix` failed
     --> test_assertion_nested_prefix.py:2:5
      |
    2 | def test_nested_diff_skips_equal_prefix():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_nested_prefix.py:3:5
      |
    3 |     assert {"outer": {"same": {"x": 1}, "changed": [1, 2]}} == {"outer": {"same": {"x": 1}, "changed": [1, 3]}}
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Difference at ['outer']['changed'][1]:
      left: 2
      right: 3

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn assertion_nested_string_difference_reports_path() {
    let context = TestContext::with_file(
        "test_assertion_nested_string.py",
        r#"
def test_nested_string_diff():
    assert {"message": "left"} == {"message": "right"}
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_nested_string::test_nested_string_diff

    failures:

    test_assertion_nested_string::test_nested_string_diff:

    error[test-failure]: Test `test_nested_string_diff` failed
     --> test_assertion_nested_string.py:2:5
      |
    2 | def test_nested_string_diff():
      |     ^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_nested_string.py:3:5
      |
    3 |     assert {"message": "left"} == {"message": "right"}
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Difference at ['message']:
      left: 'left'
      right: 'right'

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn assertion_whitespace_difference_is_visible() {
    let context = TestContext::with_file(
        "test_assertion_whitespace.py",
        r#"
def test_whitespace_string_diff():
    assert "hello world" == "helloworld"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_whitespace::test_whitespace_string_diff

    failures:

    test_assertion_whitespace::test_whitespace_string_diff:

    error[test-failure]: Test `test_whitespace_string_diff` failed
     --> test_assertion_whitespace.py:2:5
      |
    2 | def test_whitespace_string_diff():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_whitespace.py:3:5
      |
    3 |     assert "hello world" == "helloworld"
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: String difference:
      left: 'hello world'
      right: 'helloworld'

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn assertion_long_string_difference_keeps_middle_visible() {
    let context = TestContext::with_file(
        "test_assertion_long_string.py",
        r#"
def test_long_middle_string_diff():
    left = "a" * 140 + "LEFT" + "z" * 140
    right = "a" * 140 + "RIGHT" + "z" * 140
    assert left == right
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_long_string::test_long_middle_string_diff

    failures:

    test_assertion_long_string::test_long_middle_string_diff:

    error[test-failure]: Test `test_long_middle_string_diff` failed
     --> test_assertion_long_string.py:2:5
      |
    2 | def test_long_middle_string_diff():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_long_string.py:5:5
      |
    5 |     assert left == right
      |     ^^^^^^^^^^^^^^^^^^^^
    info: String difference:
      left: '…aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaLEFTzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz…'
      right: '…aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaRIGHTzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz…'

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_unicode_repr_is_bounded() {
    let context = TestContext::with_file(
        "test_assertion_unicode.py",
        r#"
def test_unicode_repr():
    assert "🙂" * 300 == None
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_unicode::test_unicode_repr

    failures:

    test_assertion_unicode::test_unicode_repr:

    error[test-failure]: Test `test_unicode_repr` failed
     --> test_assertion_unicode.py:2:5
      |
    2 | def test_unicode_repr():
      |     ^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_unicode.py:3:5
      |
    3 |     assert "🙂" * 300 == None
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      "🙂" * 300 = '🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂🙂…'

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn assertion_traceback_columns_keep_original_source() {
    let context = TestContext::with_file(
        "test_assertion_traceback.py",
        r#"
import traceback

def test_traceback_columns_keep_original_source():
    def check_columns(frame, expected):
        if hasattr(frame, "colno"):
            assert (frame.colno, frame.end_colno) == expected

    a = 1
    try:
        assert a / 0 == 2
    except ZeroDivisionError as error:
        frame = traceback.extract_tb(error.__traceback__)[-1]
        check_columns(frame, (15, 20))
    try:
        assert (
            a / 0 == 2
        )
    except ZeroDivisionError as error:
        frame = traceback.extract_tb(error.__traceback__)[-1]
        check_columns(frame, (12, 17))
    try:
        if True: assert True; assert a / 0 == 2
    except ZeroDivisionError as error:
        frame = traceback.extract_tb(error.__traceback__)[-1]
        check_columns(frame, (37, 42))
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_assertion_traceback::test_traceback_columns_keep_original_source
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_deep_value_repr_is_bounded() {
    let context = TestContext::with_file(
        "test_assertion_deep.py",
        r"
def test_deep_value():
    value = 0
    for _ in range(1500):
        value = [value]
    assert value == None
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_deep::test_deep_value

    failures:

    test_assertion_deep::test_deep_value:

    error[test-failure]: Test `test_deep_value` failed
     --> test_assertion_deep.py:2:5
      |
    2 | def test_deep_value():
      |     ^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_deep.py:6:5
      |
    6 |     assert value == None
      |     ^^^^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      value = [[[[[[[[[<builtins.list object>]]]]]]]]]

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_huge_integer_repr_is_bounded() {
    let context = TestContext::with_file(
        "test_assertion_huge_integer.py",
        r"
def test_huge_integer():
    assert 10**10000 == None
",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_huge_integer::test_huge_integer

    failures:

    test_assertion_huge_integer::test_huge_integer:

    error[test-failure]: Test `test_huge_integer` failed
     --> test_assertion_huge_integer.py:2:5
      |
    2 | def test_huge_integer():
      |     ^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_assertion_huge_integer.py:3:5
      |
    3 |     assert 10**10000 == None
      |     ^^^^^^^^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      10**10000 = <int 33220 bits>

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_hostile_metadata_does_not_run_user_code() {
    let context = TestContext::with_file(
        "test_assertion_hostile_metadata.py",
        r#"
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
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_assertion_hostile_metadata::test_hostile_metadata

    failures:

    test_assertion_hostile_metadata::test_hostile_metadata:

    error[test-failure]: Test `test_hostile_metadata` failed
      --> test_assertion_hostile_metadata.py:11:5
       |
    11 | def test_hostile_metadata():
       |     ^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
      --> test_assertion_hostile_metadata.py:12:5
       |
    12 |     assert Hostile() == Hostile()
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      left (Hostile()) = <test_assertion_hostile_metadata.Hostile object>
      right (Hostile()) = <test_assertion_hostile_metadata.Hostile object>

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

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

#[test]
fn assertion_cache_reuses_code_and_invalidates_safely() {
    let context = TestContext::with_file(
        "test_cached_assertion.py",
        "def test_cached_assertion():\n    value = 1\n    assert value == 2\n",
    );
    context.write_file(
        "sitecustomize.py",
        r#"
import ast
import os

if os.environ.get("KARVA_CACHE_PARSE_GUARD"):
    _parse = ast.parse

    def parse(source, filename="<unknown>", *args, **kwargs):
        if filename.endswith("test_cached_assertion.py"):
            raise RuntimeError("cache miss unexpectedly parsed the assertion module")
        return _parse(source, filename, *args, **kwargs)

    ast.parse = parse
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_cached_assertion::test_cached_assertion

    failures:

    test_cached_assertion::test_cached_assertion:

    error[test-failure]: Test `test_cached_assertion` failed
     --> test_cached_assertion.py:1:5
      |
    1 | def test_cached_assertion():
      |     ^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_cached_assertion.py:3:5
      |
    3 |     assert value == 2
      |     ^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      value = 1

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");

    let cache_dir = context.root().join("__pycache__");
    let cache_file = std::fs::read_dir(&cache_dir)
        .expect("assertion cache directory was not created")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("karvaV1O"))
        })
        .expect("assertion cache file was not created");

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .env("KARVA_CACHE_PARSE_GUARD", "1")
            .env("PYTHONPATH", context.root()),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_cached_assertion::test_cached_assertion

    failures:

    test_cached_assertion::test_cached_assertion:

    error[test-failure]: Test `test_cached_assertion` failed
     --> test_cached_assertion.py:1:5
      |
    1 | def test_cached_assertion():
      |     ^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_cached_assertion.py:3:5
      |
    3 |     assert value == 2
      |     ^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      value = 1

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    std::fs::write(&cache_file, b"corrupt cache").expect("failed to corrupt assertion cache");
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_cached_assertion::test_cached_assertion

    failures:

    test_cached_assertion::test_cached_assertion:

    error[test-failure]: Test `test_cached_assertion` failed
     --> test_cached_assertion.py:1:5
      |
    1 | def test_cached_assertion():
      |     ^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_cached_assertion.py:3:5
      |
    3 |     assert value == 2
      |     ^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      value = 1

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");

    context.write_file(
        "test_cached_assertion.py",
        "def test_cached_assertion():\n    value = 3\n    assert value == 2\n",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_cached_assertion::test_cached_assertion

    failures:

    test_cached_assertion::test_cached_assertion:

    error[test-failure]: Test `test_cached_assertion` failed
     --> test_cached_assertion.py:1:5
      |
    1 | def test_cached_assertion():
      |     ^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_cached_assertion.py:3:5
      |
    3 |     assert value == 2
      |     ^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      value = 3

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn assertion_cache_respects_dont_write_bytecode() {
    let context = TestContext::with_file(
        "test_no_assertion_cache.py",
        "def test_no_assertion_cache():\n    value = 1\n    assert value == 2\n",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .env("PYTHONDONTWRITEBYTECODE", "1"),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_no_assertion_cache::test_no_assertion_cache

    failures:

    test_no_assertion_cache::test_no_assertion_cache:

    error[test-failure]: Test `test_no_assertion_cache` failed
     --> test_no_assertion_cache.py:1:5
      |
    1 | def test_no_assertion_cache():
      |     ^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_no_assertion_cache.py:3:5
      |
    3 |     assert value == 2
      |     ^^^^^^^^^^^^^^^^^
    info: Evaluated values:
      value = 1

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    let has_cache = std::fs::read_dir(context.root().join("__pycache__"))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.contains("karvaV1O"))
        });
    assert!(
        !has_cache,
        "PYTHONDONTWRITEBYTECODE wrote an assertion cache"
    );
}

#[test]
fn multiline_assertion_keeps_focused_difference() {
    let context = TestContext::with_file(
        "test_multiline_assertion.py",
        "def test_multiline_assertion():\n    value = [1, 3]\n    assert (\n        value == [\n            1,\n            2,\n        ]\n    )\n",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_multiline_assertion::test_multiline_assertion

    failures:

    test_multiline_assertion::test_multiline_assertion:

    error[test-failure]: Test `test_multiline_assertion` failed
     --> test_multiline_assertion.py:1:5
      |
    1 | def test_multiline_assertion():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_multiline_assertion.py:4:9
      |
    4 |         value == [
      |         ^^^^^^^^^^
    info: Difference at [1]:
      left: 3
      right: 2

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");

    // The second import loads bytecode and reconstructs metadata on failure.
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_multiline_assertion::test_multiline_assertion

    failures:

    test_multiline_assertion::test_multiline_assertion:

    error[test-failure]: Test `test_multiline_assertion` failed
     --> test_multiline_assertion.py:1:5
      |
    1 | def test_multiline_assertion():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_multiline_assertion.py:4:9
      |
    4 |         value == [
      |         ^^^^^^^^^^
    info: Difference at [1]:
      left: 3
      right: 2

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn literal_assertions_preserve_native_module_globals() {
    let context = TestContext::with_file(
        "test_native_assertion.py",
        r#"
def test_native_assertion():
    assert False, [name for name in globals() if name.startswith("_karva_")]
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_native_assertion::test_native_assertion

    failures:

    test_native_assertion::test_native_assertion:

    error[test-failure]: Test `test_native_assertion` failed
     --> test_native_assertion.py:2:5
      |
    2 | def test_native_assertion():
      |     ^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_native_assertion.py:3:5
      |
    3 |     assert False, [name for name in globals() if name.startswith("_karva_")]
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: []

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}
