use insta::assert_snapshot;
use serde_json::Value;

use crate::common::TestContext;

#[test]
fn writes_json_result_report() {
    let context = TestContext::with_file(
        "test_report.py",
        r#"
        import pathlib
        import sys

        import karva

        def test_pass():
            print("pass stdout")

        def test_fail():
            print("fail stderr", file=sys.stderr)
            assert False

        @karva.tags.skip("skip reason")
        def test_skip():
            assert False

        def test_flaky():
            marker = pathlib.Path("flaky-count.txt")
            count = int(marker.read_text()) if marker.exists() else 0
            marker.write_text(str(count + 1))
            print(f"flaky attempt {count + 1}")
            assert count >= 1
        "#,
    );

    let output = context
        .command_no_parallel()
        .args([
            "--retry=1",
            "--status-level=none",
            "--result-output=reports/results.json",
        ])
        .output()
        .expect("run karva");
    assert_eq!(output.status.code(), Some(1));

    assert_snapshot!(normalize_result_json(&context.read_file("reports/results.json")), @r#"
    {
      "diagnostics": "error[test-failure]: Test `test_fail` failed\n  --> test_report.py:10:5\n   |/n10 | def test_fail():\n   |     ^^^^^^^^^\n   |/ninfo: Test failed here\n  --> test_report.py:12:5\n   |/n12 |     assert False\n   |     ^^^^^^^^^^^^\n   |\n\n",
      "elapsed_seconds": "[TIME]",
      "schema_version": 1,
      "stats": {
        "failed": 1,
        "flaky": 1,
        "passed": 2,
        "skipped": 1,
        "slow": 0,
        "total": 4
      },
      "status": "failed",
      "tests": [
        {
          "captured_output": {
            "stderr": "fail stderr/nfail stderr\n"
          },
          "duration_seconds": "[TIME]",
          "full_name": "test_report::test_fail",
          "module": "test_report",
          "name": "test_fail",
          "retry": {
            "attempts": 2,
            "max_attempts": 2
          },
          "status": "failed"
        },
        {
          "captured_output": {
            "stdout": "flaky attempt 1/nflaky attempt 2\n"
          },
          "duration_seconds": "[TIME]",
          "flaky": true,
          "full_name": "test_report::test_flaky",
          "module": "test_report",
          "name": "test_flaky",
          "retry": {
            "attempts": 2,
            "max_attempts": 2
          },
          "status": "passed"
        },
        {
          "captured_output": {
            "stdout": "pass stdout\n"
          },
          "duration_seconds": "[TIME]",
          "full_name": "test_report::test_pass",
          "module": "test_report",
          "name": "test_pass",
          "status": "passed"
        },
        {
          "duration_seconds": "[TIME]",
          "full_name": "test_report::test_skip",
          "module": "test_report",
          "name": "test_skip",
          "skip_reason": "skip reason",
          "status": "skipped"
        }
      ]
    }
    "#);
}

#[test]
fn writes_jsonl_result_events() {
    let context = TestContext::with_file(
        "test_events.py",
        r"
        def test_pass():
            pass

        def test_fail():
            assert False
        ",
    );

    let output = context
        .command_no_parallel()
        .args([
            "--status-level=none",
            "--result-output=reports/events.jsonl",
            "--result-format=jsonl",
        ])
        .output()
        .expect("run karva");
    assert_eq!(output.status.code(), Some(1));

    assert_snapshot!(normalize_result_jsonl(&context.read_file("reports/events.jsonl")), @r#"
    {"duration_seconds":"[TIME]","full_name":"test_events::test_fail","module":"test_events","name":"test_fail","schema_version":1,"status":"failed","type":"test"}
    {"duration_seconds":"[TIME]","full_name":"test_events::test_pass","module":"test_events","name":"test_pass","schema_version":1,"status":"passed","type":"test"}
    {"diagnostics":"error[test-failure]: Test `test_fail` failed\n --> test_events.py:5:5\n  |/n5 | def test_fail():\n  |     ^^^^^^^^^\n  |/ninfo: Test failed here\n --> test_events.py:6:5\n  |/n6 |     assert False\n  |     ^^^^^^^^^^^^\n  |\n\n","schema_version":1,"type":"diagnostics"}
    {"elapsed_seconds":"[TIME]","schema_version":1,"stats":{"failed":1,"flaky":0,"passed":1,"skipped":0,"slow":0,"total":2},"status":"failed","type":"run_finished"}
    "#);
}

#[test]
fn result_report_status_matches_no_tests_failure() {
    let context = TestContext::new();

    let output = context
        .command_no_parallel()
        .args([
            "--status-level=none",
            "--result-output=reports/no-tests.json",
        ])
        .output()
        .expect("run karva");
    assert_eq!(output.status.code(), Some(1));

    assert_snapshot!(normalize_result_json(&context.read_file("reports/no-tests.json")), @r#"
    {
      "elapsed_seconds": "[TIME]",
      "schema_version": 1,
      "stats": {
        "failed": 0,
        "flaky": 0,
        "passed": 0,
        "skipped": 0,
        "slow": 0,
        "total": 0
      },
      "status": "failed",
      "tests": []
    }
    "#);
}

fn normalize_result_json(raw: &str) -> String {
    let mut value: Value = serde_json::from_str(raw).expect("parse result report");
    redact_times(&mut value);
    let mut output = serde_json::to_string_pretty(&value).expect("serialize result report");
    output.push('\n');
    output
}

fn normalize_result_jsonl(raw: &str) -> String {
    let mut output = raw
        .lines()
        .map(|line| {
            let mut value: Value = serde_json::from_str(line).expect("parse result event");
            redact_times(&mut value);
            serde_json::to_string(&value).expect("serialize result event")
        })
        .collect::<Vec<_>>()
        .join("\n");
    output.push('\n');
    output
}

fn redact_times(value: &mut Value) {
    match value {
        Value::Array(values) => {
            for value in values {
                redact_times(value);
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                if key == "duration_seconds" || key == "elapsed_seconds" {
                    *value = Value::String("[TIME]".to_string());
                } else {
                    redact_times(value);
                }
            }
        }
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}
