use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn prune_removes_all_but_most_recent() {
    let context = TestContext::with_file("test_a.py", "def test_1(): pass");

    context.command_no_parallel().output().unwrap();
    context.command_no_parallel().output().unwrap();
    context.command_no_parallel().output().unwrap();

    let cache_dir = context.root().join(".karva_cache");
    let run_count = std::fs::read_dir(&cache_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_str()
                .unwrap()
                .starts_with("run-")
        })
        .count();
    assert_eq!(run_count, 3);

    assert_cmd_snapshot!(context.cache("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Removed: run-[TIMESTAMP]
    Removed: run-[TIMESTAMP]

    2 run(s) pruned.

    ----- stderr -----
    ");

    let remaining = std::fs::read_dir(&cache_dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_str()
                .unwrap()
                .starts_with("run-")
        })
        .count();
    assert_eq!(remaining, 1);
}

#[test]
fn prune_with_single_run_removes_nothing() {
    let context = TestContext::with_file("test_a.py", "def test_1(): pass");

    context.command_no_parallel().output().unwrap();

    assert_cmd_snapshot!(context.cache("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No cache runs to prune.

    ----- stderr -----
    ");
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

    context.command_no_parallel().output().unwrap();

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
