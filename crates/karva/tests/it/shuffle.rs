use std::process::{Command, Output};

use insta::assert_json_snapshot;
use serde::Serialize;
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

#[derive(Serialize)]
struct RunSnapshot<'a> {
    success: bool,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_orders: Option<&'a [Vec<String>]>,
}

impl RunSnapshot<'static> {
    fn output(output: &Output) -> Self {
        Self::new(output, None)
    }
}

impl<'a> RunSnapshot<'a> {
    fn shuffled(run: &'a ShuffledRun) -> Self {
        Self::new(&run.output, Some(&run.orders))
    }

    fn new(output: &Output, worker_orders: Option<&'a [Vec<String>]>) -> Self {
        let stdout = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| {
                line.strip_prefix("Random seed: ")
                    .map_or(line, |_| "Random seed: [SEED]")
            })
            .collect::<Vec<_>>()
            .join("\n");
        Self {
            success: output.status.success(),
            exit_code: output.status.code(),
            stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            worker_orders,
        }
    }
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

    assert_json_snapshot!(
        "generated_seed_replay",
        [
            ("generated", RunSnapshot::output(&first.output)),
            ("replayed", RunSnapshot::output(&repeated.output)),
        ]
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

    assert_json_snapshot!(
        "parallel_seed_replay",
        [
            ("first", RunSnapshot::shuffled(&first)),
            ("replayed", RunSnapshot::shuffled(&repeated)),
        ]
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

    assert_json_snapshot!(
        "filter_before_shuffle",
        [
            ("filtered", RunSnapshot::shuffled(&filtered)),
            ("selected", RunSnapshot::shuffled(&selected)),
        ]
    );
}

#[test]
fn partition_selection_precedes_seeded_ordering() {
    let context = TestContext::with_file(
        "test_order.py",
        &recording_tests(&["a", "b", "c", "d", "e", "f"]),
    );

    let run = run_shuffled(&context, 1, Some(170_939), &["--partition=slice:2/2"]);

    assert_json_snapshot!("partition_before_shuffle", RunSnapshot::shuffled(&run));
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

    assert_json_snapshot!("retry_after_shuffle", RunSnapshot::shuffled(&run));
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
    let json_output = run(json_command);
    let json: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report should parse");

    let mut jsonl_command = context.command_no_parallel();
    jsonl_command.args(["--result-output=results.jsonl", "--result-format=jsonl"]);
    let jsonl_output = run(jsonl_command);
    let finished: Value = context
        .read_file("results.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSONL record should parse"))
        .find(|record: &Value| record["type"] == "run_finished")
        .expect("run_finished record should exist");

    assert_json_snapshot!(
        "configured_seed_reports",
        [
            ("json", RunSnapshot::output(&json_output)),
            ("jsonl", RunSnapshot::output(&jsonl_output)),
        ]
    );
    assert_eq!(json["random_seed"], SEED);
    assert_eq!(finished["random_seed"], SEED);
}
