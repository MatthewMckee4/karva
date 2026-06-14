use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn override_retries_for_tagged_test() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[[profile.default.overrides]]
filter = "tag(network)"
retries = 2
"#,
        ),
        (
            "test.py",
            r"
import karva

counter = 0

@karva.tags.network
def test_flaky():
    global counter
    counter += 1
    assert counter >= 2
",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_flaky
      TRY 2 PASS [TIME] test::test_flaky
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/3 [TIME] test::test_flaky

    ----- stderr -----
    ");
}

/// A failing test that does not match any override should fall through to
/// the profile-level `retry` value.
#[test]
fn override_retries_does_not_apply_to_non_matching_test() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.test]
retry = 0

[[profile.default.overrides]]
filter = "tag(network)"
retries = 5
"#,
        ),
        (
            "test.py",
            r"
def test_flaky():
    assert False
",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_flaky

    diagnostics:

    error[test-failure]: Test `test_flaky` failed
     --> test.py:2:5
      |
    2 | def test_flaky():
      |     ^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:3:5
      |
    3 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// `retries = 0` on a matching override defeats a higher profile-level
/// `retry` value.
#[test]
fn override_retries_zero_disables_retries_for_matching_test() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.test]
retry = 5

[[profile.default.overrides]]
filter = "tag(unit)"
retries = 0
"#,
        ),
        (
            "test.py",
            r"
import karva

@karva.tags.unit
def test_unit():
    assert False
",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_unit

    diagnostics:

    error[test-failure]: Test `test_unit` failed
     --> test.py:5:5
      |
    5 | def test_unit():
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> test.py:6:5
      |
    6 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn override_retries_invalid_filter_errors_at_load() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[[profile.default.overrides]]
filter = "tag("
retries = 1
"#,
        ),
        ("test.py", "def test_a(): pass"),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: <temp_dir>/karva.toml is not a valid `karva.toml`: TOML parse error at line 3, column 10
      |
    3 | filter = "tag("
      |          ^^^^^^
    expected a matcher body in filter expression `tag(`

      Cause: TOML parse error at line 3, column 10
      |
    3 | filter = "tag("
      |          ^^^^^^
    expected a matcher body in filter expression `tag(`
    "#);
}

/// A matching override's `timeout` overrides the profile-level value.
#[test]
fn override_timeout_kills_matching_test() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[[profile.default.overrides]]
filter = "tag(slow)"
timeout = 0.1
"#,
        ),
        (
            "test.py",
            r"
import time
import karva

@karva.tags.slow
def test_slow():
    time.sleep(2)
",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_slow

    diagnostics:

    error[test-failure]: Test `test_slow` failed
     --> test.py:6:5
      |
    6 | def test_slow():
      |     ^^^^^^^^^
      |
    info: Test exceeded timeout of 0.1 seconds

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// `timeout = 0` on a matching override disables the hard limit even when
/// the profile sets one.
#[test]
fn override_timeout_zero_disables_profile_timeout() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.test]
timeout = 0.1

[[profile.default.overrides]]
filter = "tag(integration)"
timeout = 0
"#,
        ),
        (
            "test.py",
            r"
import time
import karva

@karva.tags.integration
def test_long_lived():
    time.sleep(0.3)
",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_long_lived
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

/// A matching override's `slow-timeout` flags only the matched test as
/// slow.
#[test]
fn override_slow_timeout_flags_matching_test() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[[profile.default.overrides]]
filter = "tag(integration)"
slow-timeout = 0.001
"#,
        ),
        (
            "test.py",
            r"
import time
import karva

@karva.tags.integration
def test_integration():
    time.sleep(0.05)

def test_unit():
    pass
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--status-level=slow"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            SLOW [TIME] test::test_integration
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped, 1 slow

    ----- stderr -----
    "
    );
}
