//! Logging attribution and shared caplog capture across worker boundaries.

use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;
use serde_json::Value;

use crate::common::TestContext;

#[rstest]
fn logs_preserve_lifecycle_and_metadata(#[values("json", "jsonl")] format: &str) {
    let context = TestContext::with_file(
        "test_logs.py",
        r#"
import logging
import karva

logger = logging.getLogger("payments")
logger.setLevel(logging.INFO)
original_level = logging.getLogger().level
user_handler = logging.StreamHandler()
logger.addHandler(user_handler)

@karva.fixture
def checkout():
    assert logging.getLogger().level == original_level
    assert logger.handlers == [user_handler]
    logger.warning("setup context")
    yield
    logger.error("teardown context")

def test_checkout(checkout):
    logger.info("call context")
    try:
        raise ValueError("provider unavailable")
    except ValueError:
        logger.exception("fallback response")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--result-output=results", &format!("--result-format={format}"),
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_logs::test_checkout(checkout=None)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
    let text = context.read_file("results");
    let case: Value = if format == "json" {
        serde_json::from_str::<Value>(&text).expect("JSON report")["tests"][0].clone()
    } else {
        text.lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record"))
            .find(|record| record["type"] == "test")
            .expect("test record")
            .clone()
    };
    let logs = case["captured_output"]["logs"]
        .as_array()
        .expect("structured logs");
    assert_eq!(logs.len(), 4);
    assert_eq!(
        logs.iter()
            .map(|log| log["phase"].as_str().expect("phase"))
            .collect::<Vec<_>>(),
        ["setup", "call", "call", "teardown"]
    );
    assert_eq!(logs[2]["logger"], "payments");
    assert_eq!(logs[2]["level"], 40);
    assert_eq!(logs[2]["level_name"], "ERROR");
    assert_eq!(logs[2]["message"], "fallback response");
    assert!(
        logs[2]["exception"]
            .as_str()
            .expect("exception")
            .contains("ValueError: provider unavailable")
    );
    assert!(
        logs[2]["timestamp"]
            .as_str()
            .expect("timestamp")
            .parse::<f64>()
            .expect("Unix seconds")
            > 0.0
    );
    assert!(
        logs[2]["pathname"]
            .as_str()
            .expect("path")
            .ends_with("test_logs.py")
    );
    assert!(logs[2]["lineno"].as_u64().expect("line") > 0);
}

#[test]
fn logs_are_shown_for_failures_without_caplog() {
    let context = TestContext::with_file(
        "test_logs.py",
        r#"
import logging

logger = logging.getLogger("payments")
logger.setLevel(logging.INFO)

def test_checkout():
    logger.info("hidden detail")
    logger.warning("fallback response")
    assert False
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_logs::test_checkout

    failures:

    test_logs::test_checkout:

    error[test-failure]: Test `test_checkout` failed
     --> test_logs.py:7:5
      |
    7 | def test_checkout():
      |     ^^^^^^^^^^^^^
    info: Test failed here
      --> test_logs.py:10:5
       |
    10 |     assert False
       |     ^^^^^^^^^^^^

    captured logs for test_logs::test_checkout:

    WARNING payments fallback response

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn logs_share_caplog_and_isolate_retries() {
    let context = TestContext::with_file(
        "test_logs.py",
        r#"
import logging
import os
from pathlib import Path
import karva

root = logging.getLogger()
baseline = list(root.handlers)

original_remove = root.removeHandler

def remove_handler(handler):
    original_remove(handler)
    Path("cleanup").write_text(str(root.handlers == baseline))

root.removeHandler = remove_handler

@karva.fixture
def resource():
    assert len(root.handlers) == len(baseline) + 1
    logging.warning("setup")
    yield
    logging.warning("teardown")

def test_retry(resource, caplog):
    assert caplog.messages == ["setup"]
    logging.warning("call")
    assert caplog.messages == ["setup", "call"]
    caplog.clear()
    assert caplog.messages == []
    assert int(os.environ["KARVA_ATTEMPT"]) > 1

def test_next(caplog):
    assert caplog.messages == []
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel().args([
        "--retry=1", "--result-output=results.json",
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
      TRY 1 FAIL [TIME] test_logs::test_retry(resource=None, caplog)
      TRY 2 PASS [TIME] test_logs::test_retry(resource=None, caplog)
            PASS [TIME] test_logs::test_next(caplog)
    ────────────
         Summary [TIME] 2 tests run: 2 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test_logs::test_retry(resource=None, caplog)

    ----- stderr -----
    ");
    let report: Value = serde_json::from_str(&context.read_file("results.json")).expect("report");
    let case = report["tests"]
        .as_array()
        .expect("tests")
        .iter()
        .find(|case| {
            case["name"]
                .as_str()
                .is_some_and(|name| name.starts_with("test_retry("))
        })
        .expect("retry");
    let attempts = case["attempts"].as_array().expect("attempts");
    assert_eq!(attempts.len(), 2);
    for attempt in attempts {
        assert_eq!(
            attempt["captured_output"]["logs"]
                .as_array()
                .expect("logs")
                .len(),
            3
        );
    }
    assert_eq!(context.read_file("cleanup"), "True");
}

#[rstest]
fn logs_are_structured_in_junit(#[values(true, false)] store: bool) {
    let context = TestContext::with_files([
        (
            "karva.toml",
            format!(
                r#"
[profile.ci.junit]
path = "results.xml"
store-success-output = {store}
"#
            ),
        ),
        (
            "test_logs.py",
            r#"
import logging

def test_pass():
    logging.warning("context <&>")
"#
            .to_owned(),
        ),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel().arg("--profile=ci"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_logs::test_pass
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
    let xml = context.read_file("results.xml");
    assert_eq!(xml.contains("karva.logs"), store);
    if store {
        assert!(xml.contains("context &lt;&amp;&gt;"));
        assert!(xml.contains("&quot;phase&quot;:&quot;call&quot;"));
        assert!(xml.contains("&quot;level&quot;:30"));
    }
}

#[test]
fn logs_isolate_parallel_parameter_cases() {
    let source = r#"
import logging
import karva

@karva.tags.parametrize("case", [1, 2])
def test_case(case):
    logging.warning("%s case %s", __name__, case)
"#;
    let context = TestContext::with_files([("test_a.py", source), ("test_b.py", source)]);
    assert_cmd_snapshot!(context.command().args([
        "--num-workers=2", "--status-level=none", "--result-output=results.json",
    ]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 4 tests run: 4 passed, 0 skipped

    ----- stderr -----
    ");
    let report: Value = serde_json::from_str(&context.read_file("results.json")).expect("report");
    let cases = report["tests"].as_array().expect("tests");
    assert_eq!(cases.len(), 4);
    for case in cases {
        let logs = case["captured_output"]["logs"].as_array().expect("logs");
        assert_eq!(logs.len(), 1);
        let message = logs[0]["message"].as_str().expect("message");
        assert!(message.starts_with(case["module"].as_str().expect("module")));
        assert!(
            case["name"]
                .as_str()
                .expect("name")
                .contains(message.chars().last().expect("parameter"))
        );
    }
}

#[rstest]
fn logs_survive_fixture_failures(#[values("setup", "teardown")] phase: &str) {
    let context = TestContext::with_file(
        "test_logs.py",
        format!(
            r#"
import logging
import karva

@karva.fixture
def resource():
    logging.warning("setup context")
    if "{phase}" == "setup":
        raise RuntimeError("setup failed")
    yield
    logging.warning("teardown context")
    raise RuntimeError("teardown failed")

def test_checkout(resource):
    logging.warning("call context")
"#
        ),
    );
    if phase == "setup" {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--result-output=results.json"), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_logs::test_checkout

        failures:

        test_logs::test_checkout (requires fixture `resource`):

        error[fixture-failure]: Fixture `resource` failed
         --> test_logs.py:6:5
          |
        6 | def resource():
          |     ^^^^^^^^
        info: Fixture failed here
         --> test_logs.py:9:9
          |
        9 |         raise RuntimeError("setup failed")
          |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: setup failed

        captured logs for test_logs::test_checkout:

        WARNING root setup context

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    } else {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--result-output=results.json"), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_logs::test_checkout(resource=None)

        failures:

        test_logs::test_checkout(resource=None):

        error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `resource`
         --> test_logs.py:6:5
          |
        6 | def resource():
          |     ^^^^^^^^
        info: Failed to reset fixture: teardown failed

        captured logs for test_logs::test_checkout(resource=None):

        WARNING root setup context
        WARNING root call context
        WARNING root teardown context

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        ");
    }
    let report: Value = serde_json::from_str(&context.read_file("results.json")).expect("report");
    let logs = report["tests"][0]["captured_output"]["logs"]
        .as_array()
        .expect("logs");
    assert_eq!(logs.last().expect("last log")["phase"], phase);
}
