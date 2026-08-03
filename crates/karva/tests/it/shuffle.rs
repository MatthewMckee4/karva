use std::process::Output;

use insta::assert_snapshot;
use serde_json::Value;

use crate::common::TestContext;

const SEED: u64 = 170_938;
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

fn snapshot(output: &Output, worker_orders: &[Vec<String>]) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| {
            line.strip_prefix("Random seed: ")
                .map_or(line, |_| "Random seed: [SEED]")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let output = format!(
        "success: {}\nexit_code: {}\n----- stdout -----\n{stdout}\n----- stderr -----\n{}",
        output.status.success(),
        output.status.code().map_or(-1, |code| code),
        String::from_utf8_lossy(&output.stderr),
    );
    let orders = worker_orders
        .iter()
        .enumerate()
        .map(|(worker, orders)| format!("worker {worker}: {orders:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    if orders.is_empty() {
        output
    } else {
        format!("{output}\n----- worker orders -----\n{orders}")
    }
}

fn snapshots(runs: &[(&str, &Output, &[Vec<String>])]) -> String {
    runs.iter()
        .map(|(label, output, orders)| format!("{label}:\n{}", snapshot(output, orders)))
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn recording_tests(names: &[&str]) -> String {
    let tests = names
        .iter()
        .map(|name| format!(r#"def test_{name}(): _record("{name}")"#))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{RECORDER}\n{tests}\n")
}

fn run_shuffled(
    context: &TestContext,
    workers: usize,
    seed: Option<u64>,
    args: &[&str],
) -> ShuffledRun {
    for worker in 0..workers {
        context.write_file(format!("order-{worker}.txt"), "");
    }
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

    let output = command.output().expect("run shuffled tests");
    let actual_seed = String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| line.strip_prefix("Random seed: "))
        .expect("seed should be printed")
        .parse::<u64>()
        .expect("seed should be a u64");
    if let Some(seed) = seed {
        assert_eq!(actual_seed, seed);
    }

    let orders = (0..workers)
        .map(|worker| {
            context
                .read_file(format!("order-{worker}.txt"))
                .lines()
                .map(str::to_string)
                .collect()
        })
        .collect();
    ShuffledRun {
        output,
        seed: actual_seed,
        orders,
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

    assert_snapshot!(
        snapshots(&[
            ("generated", &first.output, &[]),
            ("replayed", &repeated.output, &[]),
        ]),
        @"
    generated:
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 6 tests run: 6 passed, 0 skipped
    ----- stderr -----


    replayed:
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 6 tests run: 6 passed, 0 skipped
    ----- stderr -----
    "
    );
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

    assert_snapshot!(
        snapshots(&[
            ("first", &first.output, &first.orders),
            ("replayed", &repeated.output, &repeated.orders),
        ]),
        @r#"
    first:
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 10 tests run: 10 passed, 0 skipped
    ----- stderr -----

    ----- worker orders -----
    worker 0: ["d", "e", "h", "i"]
    worker 1: ["a", "b", "c", "f", "g", "j"]

    replayed:
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 10 tests run: 10 passed, 0 skipped
    ----- stderr -----

    ----- worker orders -----
    worker 0: ["d", "e", "h", "i"]
    worker 1: ["a", "b", "c", "f", "g", "j"]
    "#
    );
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

    assert_snapshot!(
        snapshots(&[
            ("filtered", &filtered.output, &filtered.orders),
            ("selected", &selected.output, &selected.orders),
        ]),
        @r#"
    filtered:
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 6 tests run: 3 passed, 3 skipped
    ----- stderr -----

    ----- worker orders -----
    worker 0: ["keep_a", "keep_b", "keep_c"]

    selected:
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped
    ----- stderr -----

    ----- worker orders -----
    worker 0: ["keep_a", "keep_b", "keep_c"]
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

    assert_snapshot!(snapshot(&run.output, &run.orders), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped
    ----- stderr -----

    ----- worker orders -----
    worker 0: ["b", "d", "f"]
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

    assert_snapshot!(snapshot(&run.output, &run.orders), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 3 tests run: 3 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test_order::test_flaky
    ----- stderr -----

    ----- worker orders -----
    worker 0: ["a", "flaky:1", "flaky:2", "z"]
    "#);
}

#[test]
fn configuration_seed_is_written_to_json_and_jsonl_reports() {
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

    let mut json_command = context.command_no_parallel();
    json_command.arg("--result-output=results.json");
    let json_output = json_command.output().expect("write JSON report");
    let json: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report should parse");

    let mut jsonl_command = context.command_no_parallel();
    jsonl_command.args(["--result-output=results.jsonl", "--result-format=jsonl"]);
    let jsonl_output = jsonl_command.output().expect("write JSONL report");
    let finished: Value = context
        .read_file("results.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSONL record should parse"))
        .find(|record: &Value| record["type"] == "run_finished")
        .expect("run_finished record should exist");

    assert_snapshot!(
        snapshots(&[
            ("json", &json_output, &[]),
            ("jsonl", &jsonl_output, &[]),
        ]),
        @"
    json:
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped
    ----- stderr -----


    jsonl:
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped
    ----- stderr -----
    "
    );
    assert_eq!(json["random_seed"], SEED);
    assert_eq!(finished["random_seed"], SEED);
}
