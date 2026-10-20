use std::collections::HashMap;
use std::time::Duration;

use insta_cmd::assert_cmd_snapshot;
use karva_cache::{read_recent_durations, write_durations};
use karva_python_semantic::TestCacheKey;
use rstest::rstest;

use crate::common::TestContext;

#[rstest]
fn focused_run_preserves_other_test_durations(#[values(false, true)] no_cache: bool) {
    let context = TestContext::with_file(
        "test_a.py",
        "def test_selected(): pass\ndef test_other(): pass\n",
    );
    let cache_dir = context.root().join(".karva_cache");
    let selected = TestCacheKey::function_name("test_a::test_selected");
    let other = TestCacheKey::function_name("test_a::test_other");
    let history = HashMap::from([
        (selected.clone(), Duration::from_secs(100)),
        (other.clone(), Duration::from_secs(200)),
    ]);
    write_durations(&cache_dir, &history).expect("seed duration history");

    let mut command = context.command_no_parallel();
    command.args(["test_a.py::test_selected", "--status-level=none"]);
    if no_cache {
        command.arg("--no-cache");
    }
    assert_cmd_snapshot!(command, @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let updated = read_recent_durations(&cache_dir).expect("read duration history");
    assert_eq!(updated.len(), history.len());
    assert_eq!(updated[&other], history[&other]);
    if no_cache {
        assert_eq!(updated, history);
    } else {
        assert_ne!(updated[&selected], history[&selected]);
    }
}

#[test]
fn filtered_out_tests_preserve_duration_history() {
    let context = TestContext::with_file("test_a.py", "def test_a(): pass\n");
    let cache_dir = context.root().join(".karva_cache");
    let history = HashMap::from([(
        TestCacheKey::function_name("test_a::test_a"),
        Duration::from_secs(100),
    )]);
    write_durations(&cache_dir, &history).expect("seed duration history");

    assert_cmd_snapshot!(context.command_no_parallel().args(["-E", "test(~missing)", "--status-level=none"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");

    assert_eq!(
        read_recent_durations(&cache_dir).expect("read duration history"),
        history
    );
}

#[test]
fn prune_removes_all_but_most_recent() {
    let context = TestContext::new();
    let cache_dir = context.root().join(".karva_cache");
    std::fs::create_dir(&cache_dir).unwrap();
    for run in ["run-100", "run-200", "run-300"] {
        std::fs::create_dir(cache_dir.join(run)).unwrap();
    }

    assert_cmd_snapshot!(context.cache("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Removed: run-[TIMESTAMP]
    Removed: run-[TIMESTAMP]

    2 run(s) pruned.

    ----- stderr -----
    ");

    assert!(cache_dir.join("run-300").is_dir());
    assert!(!cache_dir.join("run-100").exists());
    assert!(!cache_dir.join("run-200").exists());
}

#[test]
fn prune_with_single_run_removes_nothing() {
    let context = TestContext::new();
    let cache_dir = context.root().join(".karva_cache");
    std::fs::create_dir(&cache_dir).unwrap();
    std::fs::create_dir(cache_dir.join("run-100")).unwrap();

    assert_cmd_snapshot!(context.cache("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No cache runs to prune.

    ----- stderr -----
    ");
}

#[test]
fn test_run_persists_history_without_run_artifacts() {
    let context = TestContext::with_file("test_a.py", "def test_1(): pass");

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_a::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let cache_dir = context.root().join(".karva_cache");
    assert!(cache_dir.join("durations.json").exists());
    assert!(std::fs::read_dir(cache_dir).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("run-")
    }));
}

#[test]
fn prune_with_no_cache_dir() {
    let context = TestContext::new();

    assert_cmd_snapshot!(context.cache("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No cache runs to prune.

    ----- stderr -----
    ");
}

#[test]
fn clean_removes_cache_directory() {
    let context = TestContext::with_file("test_a.py", "def test_1(): pass");

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_a::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let cache_dir = context.root().join(".karva_cache");
    assert!(cache_dir.exists());

    assert_cmd_snapshot!(context.cache("clean"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Cache directory removed.

    ----- stderr -----
    ");

    assert!(!cache_dir.exists());
}

#[test]
fn clean_with_no_cache_dir() {
    let context = TestContext::new();

    assert_cmd_snapshot!(context.cache("clean"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No cache directory found.

    ----- stderr -----
    ");
}
