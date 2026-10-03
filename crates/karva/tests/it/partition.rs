use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

const SIX_TESTS: &str = "
def test_a(): pass
def test_b(): pass
def test_c(): pass
def test_d(): pass
def test_e(): pass
def test_f(): pass
";

#[test]
fn slice_first_of_three() {
    let context = TestContext::with_file("test_mod.py", SIX_TESTS);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--partition=slice:1/3"),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_mod::test_a
            PASS [TIME] test_mod::test_d
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn slice_second_of_three() {
    let context = TestContext::with_file("test_mod.py", SIX_TESTS);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--partition=slice:2/3"),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_mod::test_b
            PASS [TIME] test_mod::test_e
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn slice_third_of_three() {
    let context = TestContext::with_file("test_mod.py", SIX_TESTS);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--partition=slice:3/3"),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_mod::test_c
            PASS [TIME] test_mod::test_f
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn slice_one_of_one_runs_everything() {
    let context = TestContext::with_file("test_mod.py", SIX_TESTS);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--partition=slice:1/1"),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 6 tests across 1 worker
            PASS [TIME] test_mod::test_a
            PASS [TIME] test_mod::test_b
            PASS [TIME] test_mod::test_c
            PASS [TIME] test_mod::test_d
            PASS [TIME] test_mod::test_e
            PASS [TIME] test_mod::test_f
    ────────────
         Summary [TIME] 6 tests run: 6 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn hash_first_of_three() {
    let context = TestContext::with_file("test_mod.py", SIX_TESTS);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--partition=hash:1/3"),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_mod::test_c
            PASS [TIME] test_mod::test_e
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn hash_second_of_three() {
    let context = TestContext::with_file("test_mod.py", SIX_TESTS);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--partition=hash:2/3"),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_mod::test_b
            PASS [TIME] test_mod::test_d
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn hash_third_of_three() {
    let context = TestContext::with_file("test_mod.py", SIX_TESTS);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--partition=hash:3/3"),
        @r"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_mod::test_a
            PASS [TIME] test_mod::test_f
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn invalid_partition_index_above_total_errors() {
    let context = TestContext::with_file("test_mod.py", SIX_TESTS);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--partition=slice:4/3"),
        @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    error: invalid value 'slice:4/3' for '--partition <STRATEGY:M/N>': partition index `M` (4) must not exceed partition count `N` (3)

    For more information, try '--help'.
    "
    );
}

#[test]
fn invalid_partition_strategy_errors() {
    let context = TestContext::with_file("test_mod.py", SIX_TESTS);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--partition=random:1/3"),
        @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    error: invalid value 'random:1/3' for '--partition <STRATEGY:M/N>': unknown partition strategy `random`; supported strategies: `slice`, `hash`

    For more information, try '--help'.
    "
    );
}

#[test]
fn partitioned_parametrized_variants_retry_independently() {
    let context = TestContext::with_file(
        "test_mod.py",
        r#"
import os
import karva

@karva.tags.parametrize("value", [1, 2])
def test_a(value):
    assert os.environ["KARVA_ATTEMPT"] == "2"

@karva.tags.parametrize("value", [3, 4])
def test_b(value):
    assert os.environ["KARVA_ATTEMPT"] == "2"
"#,
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--partition=slice:1/2", "--retry=1"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
      TRY 1 FAIL [TIME] test_mod::test_a(value=1)
      TRY 2 PASS [TIME] test_mod::test_a(value=1)
      TRY 1 FAIL [TIME] test_mod::test_b(value=3)
      TRY 2 PASS [TIME] test_mod::test_b(value=3)
    ────────────
         Summary [TIME] 2 tests run: 2 passed (2 flaky), 0 skipped
       FLAKY 2/2 [TIME] test_mod::test_a(value=1)
       FLAKY 2/2 [TIME] test_mod::test_b(value=3)

    ----- stderr -----
    "
    );
}

#[test]
fn cached_parametrize_cases_partition_across_workers() {
    let context = TestContext::with_file(
        "test_mod.py",
        r#"
import os
import time
import karva

@karva.tags.parametrize("value", [0, 1, 2, 3, 4, 5, 6, 7, 8, 9])
def test_value(value):
    time.sleep(value / 1000)
    if os.environ.get("RECORD_WORKERS") == "1":
        worker = os.environ["KARVA_WORKER_ID"]
        with open(f"worker-{worker}.txt", "a", encoding="utf-8") as output:
            output.write(f"{value}\n")
"#,
    );

    let first = context
        .command_no_parallel()
        .arg("--status-level=none")
        .output()
        .expect("seed duration cache");
    assert!(first.status.success(), "{first:?}");

    let second = context
        .command()
        .args(["--num-workers=2", "--status-level=none"])
        .env("RECORD_WORKERS", "1")
        .output()
        .expect("partition cached cases");
    assert!(second.status.success(), "{second:?}");

    let mut first_worker = context
        .read_file("worker-0.txt")
        .lines()
        .map(str::parse::<usize>)
        .collect::<Result<Vec<_>, _>>()
        .expect("worker 0 case indices");
    let second_worker = context
        .read_file("worker-1.txt")
        .lines()
        .map(str::parse::<usize>)
        .collect::<Result<Vec<_>, _>>()
        .expect("worker 1 case indices");
    assert!(!first_worker.is_empty());
    assert!(!second_worker.is_empty());
    first_worker.extend(second_worker);
    first_worker.sort_unstable();
    assert_eq!(first_worker, (0..10).collect::<Vec<_>>());
}

#[test]
fn dynamic_parametrize_cases_remain_atomic() {
    let context = TestContext::with_file(
        "test_mod.py",
        r#"
import karva

@karva.tags.parametrize("value", range(10))
def test_value(value):
    pass
"#,
    );

    assert_cmd_snapshot!(
        context
            .command()
            .args(["--num-workers=2", "--status-level=none"]),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 10 tests run: 10 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn last_failed_selects_one_parametrize_case() {
    let context = TestContext::with_file(
        "test_mod.py",
        r#"
from pathlib import Path
import karva

@karva.tags.parametrize("value", [0, 1, 2])
def test_value(value):
    if Path("fixed").exists():
        Path("executed").write_text(str(value), encoding="utf-8")
    assert value != 1 or Path("fixed").exists()
"#,
    );

    let first = context
        .command_no_parallel()
        .arg("--status-level=none")
        .output()
        .expect("record failed case");
    assert!(!first.status.success());

    context.write_file("fixed", "");
    let second = context
        .command_no_parallel()
        .args(["--last-failed", "--status-level=none"])
        .output()
        .expect("rerun failed case");
    assert!(second.status.success(), "{second:?}");
    assert_eq!(context.read_file("executed"), "1");
}
