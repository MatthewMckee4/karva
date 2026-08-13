use std::collections::HashSet;

use karva_python_semantic::TestCacheKey;

use super::super::Partition;
use super::helpers::test_info;

#[test]
fn crash_recovery_keeps_only_unstarted_static_parameter_cases() {
    let mut partition = Partition::new();
    for name in [
        "test_module::test_case[0]",
        "test_module::test_case[1]",
        "test_module::test_case[2]",
    ] {
        partition.add_test(test_info(name), 1);
    }
    let completed = HashSet::from([TestCacheKey::function_name("test_module::test_case[0]")]);

    let pending = partition.pending_after_crash(
        &completed,
        Some(&TestCacheKey::function_name("test_module::test_case[1]")),
    );

    assert_eq!(
        pending.test_paths().collect::<Vec<_>>(),
        ["test_module::test_case[2]"]
    );
}

#[test]
fn crash_recovery_resumes_dynamic_parameter_functions() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_case"), 1);

    let pending = partition.pending_after_crash(
        &HashSet::new(),
        Some(&TestCacheKey::function_name("test_module::test_case[1]")),
    );

    assert_eq!(
        pending.test_paths().collect::<Vec<_>>(),
        ["test_module::test_case"]
    );
    assert_eq!(
        pending.resume_skip(),
        &[TestCacheKey::function_name("test_module::test_case[1]")]
    );
}

#[test]
fn crash_recovery_does_not_repeat_completed_dynamic_functions() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_dynamic"), 1);
    partition.add_test(test_info("test_module::test_crash"), 1);
    let completed = HashSet::from([
        TestCacheKey::function_name("test_module::test_dynamic[0]"),
        TestCacheKey::function_name("test_module::test_dynamic[1]"),
    ]);

    let pending = partition.pending_after_crash(
        &completed,
        Some(&TestCacheKey::function_name("test_module::test_crash")),
    );

    assert!(pending.is_empty());
    assert!(pending.resume_skip().is_empty());
}

#[test]
fn crash_recovery_does_not_spawn_a_replacement_for_plain_crashed_test() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_crash"), 1);

    let pending = partition.pending_after_crash(
        &HashSet::new(),
        Some(&TestCacheKey::function_name("test_module::test_crash")),
    );

    assert!(pending.is_empty());
}

#[test]
fn crash_recovery_reschedules_unstarted_tests_without_an_active_test() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_completed"), 1);
    partition.add_test(test_info("test_module::test_pending"), 1);
    let completed = HashSet::from([TestCacheKey::function_name("test_module::test_completed")]);

    let pending = partition.pending_after_crash(&completed, None);

    assert_eq!(
        pending.test_paths().collect::<Vec<_>>(),
        ["test_module::test_pending"]
    );
}

#[test]
fn crash_recovery_resumes_partial_dynamic_function_without_an_active_test() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_dynamic"), 1);
    let completed = HashSet::from([TestCacheKey::function_name("test_module::test_dynamic[0]")]);

    let pending = partition.pending_after_crash(&completed, None);

    assert_eq!(
        pending.test_paths().collect::<Vec<_>>(),
        ["test_module::test_dynamic"]
    );
    assert_eq!(
        pending.resume_skip(),
        &[TestCacheKey::function_name("test_module::test_dynamic[0]")]
    );
}

#[test]
fn crash_recovery_stops_retrying_without_progress() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_pending"), 1);

    let first_retry = partition.pending_after_crash(&HashSet::new(), None);
    let second_retry = first_retry.pending_after_crash(&HashSet::new(), None);

    assert!(second_retry.is_empty());
}
