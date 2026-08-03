use std::process::{Command, Output};
use std::sync::LazyLock;

use insta::assert_json_snapshot;
use regex::Regex;
use rstest::rstest;
use serde_json::{Value, json};

use crate::common::TestContext;

const SEED: u64 = 170_938;
static DURATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\s*\d+\.\d+s\]").expect("valid duration regex"));
const RECORDER: &str = r#"
import os

def _record(name):
    worker = os.environ["KARVA_WORKER_ID"]
    with open(f"order-{worker}.txt", "a", encoding="utf-8") as output:
        output.write(f"{name}\n")
"#;

struct ShuffledRun {
    output: Output,
    seed: u64,
    orders: Vec<Vec<String>>,
}

fn snapshot(output: &Output, worker_orders: &[Vec<String>]) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            line.strip_prefix("Random seed: ")
                .map_or(line, |_| "Random seed: [SEED]")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let stdout = DURATION.replace_all(&stdout, "[TIME]");
    let mut snapshot = json!({
        "success": output.status.success(),
        "exit_code": output.status.code(),
        "stdout": stdout,
        "stderr": String::from_utf8_lossy(&output.stderr),
    });
    if !worker_orders.is_empty() {
        snapshot["worker_orders"] = json!(worker_orders);
    }
    snapshot
}

fn recording_tests(names: &[&str]) -> String {
    let tests = names
        .iter()
        .map(|name| format!(r#"def test_{name}(): _record("{name}")"#))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{RECORDER}\n{tests}\n")
}

fn clear_orders(context: &TestContext, workers: usize) {
    for worker in 0..workers {
        context.write_file(format!("order-{worker}.txt"), "");
    }
}

fn read_orders(context: &TestContext, workers: usize) -> Vec<Vec<String>> {
    (0..workers)
        .map(|worker| {
            context
                .read_file(format!("order-{worker}.txt"))
                .lines()
                .map(str::to_string)
                .collect()
        })
        .collect()
}

fn run(mut command: Command) -> Output {
    command.output().expect("run Karva")
}

fn run_shuffled(
    context: &TestContext,
    workers: usize,
    seed: Option<u64>,
    args: &[&str],
) -> ShuffledRun {
    clear_orders(context, workers);
    let mut command = if workers == 1 {
        context.command_no_parallel()
    } else {
        let mut command = context.command();
        command.arg(format!("--num-workers={workers}"));
        command
    };
    command.args(["--shuffle", "--status-level=none"]);
    if let Some(seed) = seed {
        command.arg(format!("--random-seed={seed}"));
    }
    command.args(args);

    let output = run(command);
    let actual_seed = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Random seed: "))
        .expect("seed should be printed")
        .parse::<u64>()
        .expect("seed should be a u64");
    if let Some(seed) = seed {
        assert_eq!(actual_seed, seed);
    }

    ShuffledRun {
        output,
        seed: actual_seed,
        orders: read_orders(context, workers),
    }
}

#[test]
fn generated_seed_reproduces_single_worker_order() {
    let context = TestContext::with_file(
        "test_order.py",
        &recording_tests(&["a", "b", "c", "d", "e", "f"]),
    );

    let first = run_shuffled(&context, 1, None, &[]);
    let repeated = run_shuffled(&context, 1, Some(first.seed), &[]);

    let first_output = snapshot(&first.output, &[]);
    assert_eq!(first_output, snapshot(&repeated.output, &[]));
    assert_json_snapshot!(first_output, @r#"
    {
      "exit_code": 0,
      "stderr": "",
      "stdout": "Random seed: [SEED]\n────────────\n     Summary [TIME] 6 tests run: 6 passed, 0 skipped",
      "success": true
    }
    "#);
    assert_eq!(first.orders, repeated.orders);
}

#[test]
fn parallel_worker_assignment_and_order_are_reproducible() {
    let context = TestContext::with_file(
        "test_order.py",
        &recording_tests(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]),
    );

    let first = run_shuffled(&context, 2, Some(SEED), &[]);
    let repeated = run_shuffled(&context, 2, Some(SEED), &[]);

    let first_snapshot = snapshot(&first.output, &first.orders);
    assert_eq!(first_snapshot, snapshot(&repeated.output, &repeated.orders));
    assert_json_snapshot!(first_snapshot, @r#"
    {
      "exit_code": 0,
      "stderr": "",
      "stdout": "Random seed: [SEED]\n────────────\n     Summary [TIME] 10 tests run: 10 passed, 0 skipped",
      "success": true,
      "worker_orders": [
        [
          "d",
          "e",
          "h",
          "i"
        ],
        [
          "a",
          "b",
          "c",
          "f",
          "g",
          "j"
        ]
      ]
    }
    "#);
}

#[test]
fn filtering_precedes_seeded_ordering() {
    let context = TestContext::with_file(
        "test_order.py",
        &recording_tests(&["drop_a", "keep_a", "drop_b", "keep_b", "drop_c", "keep_c"]),
    );

    let filtered = run_shuffled(&context, 1, Some(SEED), &["--filter=test(~keep)"]);
    context.write_file(
        "test_order.py",
        &recording_tests(&["keep_a", "keep_b", "keep_c"]),
    );
    let selected = run_shuffled(&context, 1, Some(SEED), &[]);

    assert_eq!(filtered.orders, selected.orders);
    assert_json_snapshot!(
        json!({
            "filtered": snapshot(&filtered.output, &[]),
            "selected": snapshot(&selected.output, &[]),
            "worker_orders": filtered.orders,
        }),
        @r#"
    {
      "filtered": {
        "exit_code": 0,
        "stderr": "",
        "stdout": "Random seed: [SEED]\n────────────\n     Summary [TIME] 6 tests run: 3 passed, 3 skipped",
        "success": true
      },
      "selected": {
        "exit_code": 0,
        "stderr": "",
        "stdout": "Random seed: [SEED]\n────────────\n     Summary [TIME] 3 tests run: 3 passed, 0 skipped",
        "success": true
      },
      "worker_orders": [
        [
          "keep_a",
          "keep_b",
          "keep_c"
        ]
      ]
    }
    "#
    );
}

#[test]
fn partition_selection_precedes_seeded_ordering() {
    let context = TestContext::with_file(
        "test_order.py",
        &recording_tests(&["a", "b", "c", "d", "e", "f"]),
    );

    let run = run_shuffled(&context, 1, Some(170_939), &["--partition=slice:2/2"]);

    assert_json_snapshot!(snapshot(&run.output, &run.orders), @r#"
    {
      "exit_code": 0,
      "stderr": "",
      "stdout": "Random seed: [SEED]\n────────────\n     Summary [TIME] 3 tests run: 3 passed, 0 skipped",
      "success": true,
      "worker_orders": [
        [
          "b",
          "d",
          "f"
        ]
      ]
    }
    "#);
}

#[test]
fn retries_stay_with_shuffled_test() {
    let context = TestContext::with_file(
        "test_order.py",
        &format!(
            r#"
{RECORDER}
def test_a(): _record("a")

def test_flaky():
    attempt = os.environ["KARVA_ATTEMPT"]
    _record(f"flaky:{{attempt}}")
    assert attempt == "2"

def test_z(): _record("z")
"#
        ),
    );

    let run = run_shuffled(&context, 1, Some(SEED), &["--retry=1"]);

    assert_json_snapshot!(snapshot(&run.output, &run.orders), @r#"
    {
      "exit_code": 0,
      "stderr": "",
      "stdout": "Random seed: [SEED]\n────────────\n     Summary [TIME] 3 tests run: 3 passed (1 flaky), 0 skipped\n   FLAKY 2/2 [TIME] test_order::test_flaky",
      "success": true,
      "worker_orders": [
        [
          "a",
          "flaky:1",
          "flaky:2",
          "z"
        ]
      ]
    }
    "#);
}

#[rstest]
fn configuration_seed_is_written_to_report(#[values("json", "jsonl")] format: &str) {
    let context = TestContext::with_files([
        ("test_order.py", "def test_a(): pass"),
        (
            "karva.toml",
            r#"
[profile.default.test]
shuffle = true
random-seed = 170938

[profile.default.terminal]
status-level = "none"
"#,
        ),
    ]);

    let result_path = format!("results.{format}");
    let mut command = context.command_no_parallel();
    command.arg(format!("--result-output={result_path}"));
    if format == "jsonl" {
        command.arg("--result-format=jsonl");
    }
    let output = run(command);
    let report = context.read_file(result_path);
    let random_seed = if format == "json" {
        serde_json::from_str::<Value>(&report).expect("JSON report should parse")["random_seed"]
            .clone()
    } else {
        report
            .lines()
            .map(|line| serde_json::from_str::<Value>(line).expect("JSONL record should parse"))
            .find(|record| record["type"] == "run_finished")
            .expect("run_finished record should exist")["random_seed"]
            .clone()
    };

    insta::allow_duplicates! {
        assert_json_snapshot!(
            json!({
                "output": snapshot(&output, &[]),
                "random_seed": random_seed,
            }),
            @r#"
        {
          "output": {
            "exit_code": 0,
            "stderr": "",
            "stdout": "Random seed: [SEED]\n────────────\n     Summary [TIME] 1 test run: 1 passed, 0 skipped",
            "success": true
          },
          "random_seed": 170938
        }
        "#
        );
    }
}
