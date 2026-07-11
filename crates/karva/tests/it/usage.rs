use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn usage_prints_agent_friendly_commands() {
    let context = TestContext::default();

    assert_cmd_snapshot!(context.usage(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Karva usage for agents

    Run tests:
      karva test
      karva test path/to/test_file.py
      karva test path/to/test_file.py::test_name

    Focus while editing:
      karva test path/to/test_file.py --status-level=fail
      karva test --last-failed
      karva test --filter 'test(~login)'

    Debug output:
      karva test --no-parallel
      karva test --no-capture
      karva show-config

    Exit codes:
      0  tests passed
      1  tests failed, timed out, or no tests were allowed to run
      2  configuration, discovery, or internal error

    Use `karva test --help` for all test options.

    ----- stderr -----
    ");
}
