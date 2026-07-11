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

    let report: Value =
        serde_json::from_str(&context.read_file("reports/results.json")).expect("parse report");

    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["stats"]["total"], 4);
    assert_eq!(report["stats"]["passed"], 2);
    assert_eq!(report["stats"]["failed"], 1);
    assert_eq!(report["stats"]["skipped"], 1);
    assert_eq!(report["stats"]["flaky"], 1);
    assert!(
        report["diagnostics"]
            .as_str()
            .is_some_and(|diagnostics| { diagnostics.contains("Test `test_fail` failed") })
    );

    let pass = test_by_name(&report, "test_report::test_pass");
    assert_eq!(pass["status"], "passed");
    assert_eq!(pass["captured_output"]["stdout"], "pass stdout\n");

    let fail = test_by_name(&report, "test_report::test_fail");
    assert_eq!(fail["status"], "failed");
    assert_eq!(fail["retry"]["attempts"], 2);
    assert_eq!(fail["retry"]["max_attempts"], 2);
    assert!(
        fail["captured_output"]["stderr"]
            .as_str()
            .is_some_and(|stderr| { stderr.contains("fail stderr") })
    );

    let skip = test_by_name(&report, "test_report::test_skip");
    assert_eq!(skip["status"], "skipped");
    assert_eq!(skip["skip_reason"], "skip reason");

    let flaky = test_by_name(&report, "test_report::test_flaky");
    assert_eq!(flaky["status"], "passed");
    assert_eq!(flaky["flaky"], true);
    assert_eq!(flaky["retry"]["attempts"], 2);
    assert_eq!(flaky["retry"]["max_attempts"], 2);
    assert!(
        flaky["captured_output"]["stdout"]
            .as_str()
            .is_some_and(|stdout| stdout.contains("flaky attempt 1"))
    );
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

    let events = context.read_file("reports/events.jsonl");
    let events: Vec<Value> = events
        .lines()
        .map(|line| serde_json::from_str(line).expect("parse event"))
        .collect();

    assert_eq!(events.len(), 4);
    assert_eq!(events[0]["schema_version"], 1);
    assert_eq!(events[0]["type"], "test");
    assert_eq!(events[0]["full_name"], "test_events::test_fail");
    assert_eq!(events[0]["status"], "failed");
    assert_eq!(events[1]["type"], "test");
    assert_eq!(events[1]["full_name"], "test_events::test_pass");
    assert_eq!(events[1]["status"], "passed");
    assert_eq!(events[2]["type"], "diagnostics");
    assert!(
        events[2]["diagnostics"]
            .as_str()
            .is_some_and(|diagnostics| { diagnostics.contains("Test `test_fail` failed") })
    );
    assert_eq!(events[3]["type"], "run_finished");
    assert_eq!(events[3]["status"], "failed");
    assert_eq!(events[3]["stats"]["total"], 2);
}

fn test_by_name<'a>(report: &'a Value, full_name: &str) -> &'a Value {
    report["tests"]
        .as_array()
        .expect("tests array")
        .iter()
        .find(|test| test["full_name"] == full_name)
        .unwrap_or_else(|| panic!("missing test `{full_name}`"))
}
