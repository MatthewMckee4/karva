use std::collections::BTreeSet;
use std::process::Output;

use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
use serde_json::Value;

use crate::common::TestContext;

const SIX_TESTS: &str = r#"
import os

def _record(name):
    worker = os.environ["KARVA_WORKER_ID"]
    with open(f"order-{worker}.txt", "a", encoding="utf-8") as output:
        output.write(f"{name}\n")

def test_a(): _record("a")
def test_b(): _record("b")
def test_c(): _record("c")
def test_d(): _record("d")
def test_e(): _record("e")
def test_f(): _record("f")
"#;

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

fn render_output(output: &Output) -> String {
    format!(
        "success: {}\nexit_code: {}\n----- stdout -----\n{}\n----- stderr -----\n{}",
        output.status.success(),
        output.status.code().map_or(-1, |code| code),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn generated_seed_reproduces_single_worker_order() {
    let context = TestContext::with_file("test_order.py", SIX_TESTS);
    clear_orders(&context, 1);

    let first_output = context
        .command_no_parallel()
        .args(["--shuffle", "--status-level=none"])
        .output()
        .expect("run shuffled tests");
    let stdout = String::from_utf8(first_output.stdout.clone()).expect("stdout should be UTF-8");
    let seed = stdout
        .lines()
        .find_map(|line| line.strip_prefix("Random seed: "))
        .expect("generated seed should be printed")
        .parse::<u64>()
        .expect("generated seed should be a u64");
    let first_order = read_orders(&context, 1);

    clear_orders(&context, 1);
    let repeated_output = context
        .command_no_parallel()
        .args([
            "--shuffle",
            &format!("--random-seed={seed}"),
            "--status-level=none",
        ])
        .output()
        .expect("repeat shuffled tests");
    let repeated_order = read_orders(&context, 1);

    let first_rendered = render_output(&first_output)
        .replace(&format!("Random seed: {seed}"), "Random seed: [SEED]");
    let repeated_rendered = render_output(&repeated_output)
        .replace(&format!("Random seed: {seed}"), "Random seed: [SEED]");
    insta::allow_duplicates! {
        assert_snapshot!(first_rendered, @"
        success: true
        exit_code: 0
        ----- stdout -----
        Random seed: [SEED]
        ────────────
             Summary [TIME] 6 tests run: 6 passed, 0 skipped

        ----- stderr -----
        ");
        assert_snapshot!(repeated_rendered, @"
        success: true
        exit_code: 0
        ----- stdout -----
        Random seed: [SEED]
        ────────────
             Summary [TIME] 6 tests run: 6 passed, 0 skipped

        ----- stderr -----
        ");
    }
    assert_eq!(first_order, repeated_order);
}

#[test]
fn parallel_worker_assignment_and_order_are_reproducible() {
    let source = format!(
        "{SIX_TESTS}\ndef test_g(): _record(\"g\")\ndef test_h(): _record(\"h\")\ndef test_i(): _record(\"i\")\ndef test_j(): _record(\"j\")\n"
    );
    let context = TestContext::with_file("test_order.py", &source);
    clear_orders(&context, 2);

    assert_cmd_snapshot!(
        context.command().args([
            "--shuffle",
            "--random-seed=170938",
            "--num-workers=2",
            "--status-level=none",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: 170938
    ────────────
         Summary [TIME] 10 tests run: 10 passed, 0 skipped

    ----- stderr -----
    "
    );
    let first = read_orders(&context, 2);

    clear_orders(&context, 2);
    assert_cmd_snapshot!(
        context.command().args([
            "--shuffle",
            "--random-seed=170938",
            "--num-workers=2",
            "--status-level=none",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: 170938
    ────────────
         Summary [TIME] 10 tests run: 10 passed, 0 skipped

    ----- stderr -----
    "
    );
    let repeated = read_orders(&context, 2);

    assert_eq!(first, repeated);
    assert!(first.iter().all(|worker| !worker.is_empty()));
    assert_eq!(first.iter().map(Vec::len).sum::<usize>(), 10);
}

#[test]
fn filtering_precedes_seeded_ordering() {
    let context = TestContext::with_file(
        "test_order.py",
        r#"
import os

def _record(name):
    worker = os.environ["KARVA_WORKER_ID"]
    with open(f"order-{worker}.txt", "a", encoding="utf-8") as output:
        output.write(f"{name}\n")

def test_drop_a(): _record("drop-a")
def test_keep_a(): _record("keep-a")
def test_drop_b(): _record("drop-b")
def test_keep_b(): _record("keep-b")
def test_drop_c(): _record("drop-c")
def test_keep_c(): _record("keep-c")
"#,
    );
    clear_orders(&context, 1);

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--shuffle",
            "--random-seed=170938",
            "--filter=test(~keep)",
            "--status-level=none",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: 170938
    ────────────
         Summary [TIME] 6 tests run: 3 passed, 3 skipped

    ----- stderr -----
    "
    );
    let filtered_order = read_orders(&context, 1);

    context.write_file(
        "test_order.py",
        r#"
import os

def _record(name):
    worker = os.environ["KARVA_WORKER_ID"]
    with open(f"order-{worker}.txt", "a", encoding="utf-8") as output:
        output.write(f"{name}\n")

def test_keep_a(): _record("keep-a")
def test_keep_b(): _record("keep-b")
def test_keep_c(): _record("keep-c")
"#,
    );
    clear_orders(&context, 1);
    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--shuffle",
            "--random-seed=170938",
            "--status-level=none",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: 170938
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    "
    );

    assert_eq!(filtered_order, read_orders(&context, 1));
}

#[test]
fn partition_selection_precedes_seeded_ordering() {
    let context = TestContext::with_file("test_order.py", SIX_TESTS);
    clear_orders(&context, 1);

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--shuffle",
            "--random-seed=170939",
            "--partition=slice:2/2",
            "--status-level=none",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: 170939
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    "
    );

    let order = &read_orders(&context, 1)[0];
    assert_eq!(
        order.iter().map(String::as_str).collect::<BTreeSet<_>>(),
        BTreeSet::from(["b", "d", "f"]),
    );
}

#[test]
fn retries_stay_with_shuffled_test() {
    let context = TestContext::with_file(
        "test_order.py",
        r#"
import os

def _record(name):
    worker = os.environ["KARVA_WORKER_ID"]
    with open(f"order-{worker}.txt", "a", encoding="utf-8") as output:
        output.write(f"{name}\n")

def test_a(): _record("a")

def test_flaky():
    attempt = os.environ["KARVA_ATTEMPT"]
    _record(f"flaky:{attempt}")
    assert attempt == "2"

def test_z(): _record("z")
"#,
    );
    clear_orders(&context, 1);

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--shuffle",
            "--random-seed=170938",
            "--retry=1",
            "--status-level=none",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: 170938
    ────────────
         Summary [TIME] 3 tests run: 3 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test_order::test_flaky

    ----- stderr -----
    "
    );

    let order = &read_orders(&context, 1)[0];
    let first_attempt = order
        .iter()
        .position(|entry| entry == "flaky:1")
        .expect("first retry attempt should run");
    assert_eq!(
        &order[first_attempt..=first_attempt + 1],
        ["flaky:1", "flaky:2"]
    );
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

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("--result-output=results.json"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: 170938
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
    let json: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report should parse");
    assert_eq!(json["random_seed"], 170_938);

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--result-output=results.jsonl",
            "--result-format=jsonl",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: 170938
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
    let finished: Value = context
        .read_file("results.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).expect("JSONL record should parse"))
        .find(|record: &Value| record["type"] == "run_finished")
        .expect("run_finished record should exist");
    assert_eq!(finished["random_seed"], 170_938);
}
