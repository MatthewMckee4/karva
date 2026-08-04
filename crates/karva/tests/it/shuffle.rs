use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
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

fn recording_tests(names: &[&str]) -> String {
    let tests = names
        .iter()
        .map(|name| format!(r#"def test_{name}(): _record("{name}")"#))
        .collect::<Vec<_>>()
        .join("\n");
    format!("{RECORDER}\n{tests}\n")
}

#[test]
fn generated_seed_reproduces_single_worker_order() {
    let context = TestContext::with_file(
        "test_order.py",
        &recording_tests(&["a", "b", "c", "d", "e", "f"]),
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--shuffle", "--status-level=none"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 6 tests run: 6 passed, 0 skipped

    ----- stderr -----
    "
    );
    let generated_order = context.read_file("order-0.txt");

    context.write_file("order-0.txt", "");
    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--shuffle",
            "--random-seed=last",
            "--status-level=none",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 6 tests run: 6 passed, 0 skipped

    ----- stderr -----
    "
    );
    assert_eq!(context.read_file("order-0.txt"), generated_order);
}

#[test]
fn last_seed_requires_a_generated_seed() {
    let context = TestContext::with_file("test_pass.py", "def test_pass(): pass");

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--shuffle", "--random-seed=last"]),
        @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: No generated random seed found; run with `--shuffle` first
    "
    );
}

#[test]
fn last_seed_requires_cache() {
    let context = TestContext::with_file("test_pass.py", "def test_pass(): pass");

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--shuffle", "--random-seed=last", "--no-cache"]),
        @"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    karva failed
      Cause: `--random-seed=last` cannot be used with `--no-cache`
    "
    );
}

#[test]
fn parallel_worker_assignment_and_order_are_reproducible() {
    let context = TestContext::with_file(
        "test_order.py",
        &recording_tests(&["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]),
    );

    assert_cmd_snapshot!(
        context.command().args([
            "--num-workers=2",
            "--shuffle",
            "--random-seed=170938",
            "--status-level=none",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 10 tests run: 10 passed, 0 skipped

    ----- stderr -----
    "
    );
    let first_worker_0 = context.read_file("order-0.txt");
    let first_worker_1 = context.read_file("order-1.txt");
    assert_snapshot!(first_worker_0, @"
    d
    e
    h
    i
    ");
    assert_snapshot!(first_worker_1, @"
    a
    b
    c
    f
    g
    j
    ");

    context.write_file("order-0.txt", "");
    context.write_file("order-1.txt", "");
    assert_cmd_snapshot!(
        context.command().args([
            "--num-workers=2",
            "--shuffle",
            "--random-seed=170938",
            "--status-level=none",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 10 tests run: 10 passed, 0 skipped

    ----- stderr -----
    "
    );
    assert_eq!(context.read_file("order-0.txt"), first_worker_0);
    assert_eq!(context.read_file("order-1.txt"), first_worker_1);
}

#[test]
fn filtering_precedes_seeded_ordering() {
    let context = TestContext::with_file(
        "test_order.py",
        &recording_tests(&["drop_a", "keep_a", "drop_b", "keep_b", "drop_c", "keep_c"]),
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--shuffle",
            "--random-seed=170938",
            "--status-level=none",
            "--filter=test(~keep)",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 6 tests run: 3 passed, 3 skipped

    ----- stderr -----
    "
    );
    let filtered_order = context.read_file("order-0.txt");
    assert_snapshot!(filtered_order, @"
    keep_a
    keep_b
    keep_c
    ");

    context.write_file(
        "test_order.py",
        &recording_tests(&["keep_a", "keep_b", "keep_c"]),
    );
    context.write_file("order-0.txt", "");
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
    Random seed: [SEED]
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    "
    );
    assert_eq!(context.read_file("order-0.txt"), filtered_order);
}

#[test]
fn partition_selection_precedes_seeded_ordering() {
    let context = TestContext::with_file(
        "test_order.py",
        &recording_tests(&["a", "b", "c", "d", "e", "f"]),
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--shuffle",
            "--random-seed=170939",
            "--status-level=none",
            "--partition=slice:2/2",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    "
    );
    assert_snapshot!(context.read_file("order-0.txt"), @"
    b
    d
    f
    ");
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

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--shuffle",
            "--random-seed=170938",
            "--status-level=none",
            "--retry=1",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
    ────────────
         Summary [TIME] 3 tests run: 3 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test_order::test_flaky

    ----- stderr -----
    "
    );
    assert_snapshot!(context.read_file("order-0.txt"), @"
    a
    flaky:1
    flaky:2
    z
    ");
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
    Random seed: [SEED]
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );
    let json: Value =
        serde_json::from_str(&context.read_file("results.json")).expect("JSON report should parse");
    assert_eq!(json["random_seed"], SEED);

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--result-output=results.jsonl",
            "--result-format=jsonl",
        ]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    Random seed: [SEED]
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
    assert_eq!(finished["random_seed"], SEED);
}
