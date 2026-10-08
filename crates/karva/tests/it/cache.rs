use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

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
