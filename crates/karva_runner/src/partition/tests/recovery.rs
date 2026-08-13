use std::collections::HashSet;

use karva_python_semantic::TestCacheKey;

use super::super::{CompletedTestIndex, Partition, UnattributedCrashRecovery};
use super::helpers::test_info;

fn pending_after_test_crash(
    partition: &Partition,
    completed: &HashSet<TestCacheKey>,
    crashed: &TestCacheKey,
) -> Partition {
    partition.pending_after_test_crash(&CompletedTestIndex::new(completed), crashed)
}

fn recover_unattributed_crash(
    partition: &Partition,
    completed: &HashSet<TestCacheKey>,
) -> UnattributedCrashRecovery {
    partition.recover_unattributed_crash(&CompletedTestIndex::new(completed))
}

fn retry(recovery: UnattributedCrashRecovery) -> (Partition, usize) {
    if let UnattributedCrashRecovery::Retry {
        pending,
        completed_results,
    } = recovery
    {
        Some((pending, completed_results))
    } else {
        None
    }
    .expect("recovery should retry pending work")
}

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

    let pending = pending_after_test_crash(
        &partition,
        &completed,
        &TestCacheKey::function_name("test_module::test_case[1]"),
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

    let pending = pending_after_test_crash(
        &partition,
        &HashSet::new(),
        &TestCacheKey::function_name("test_module::test_case[1]"),
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

    let pending = pending_after_test_crash(
        &partition,
        &completed,
        &TestCacheKey::function_name("test_module::test_crash"),
    );

    assert!(pending.is_empty());
    assert!(pending.resume_skip().is_empty());
}

#[test]
fn crash_recovery_does_not_spawn_a_replacement_for_plain_crashed_test() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_crash"), 1);

    let pending = pending_after_test_crash(
        &partition,
        &HashSet::new(),
        &TestCacheKey::function_name("test_module::test_crash"),
    );

    assert!(pending.is_empty());
}

#[test]
fn crash_recovery_reschedules_unstarted_tests_without_an_active_test() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_completed"), 1);
    partition.add_test(test_info("test_module::test_pending"), 1);
    let completed = HashSet::from([TestCacheKey::function_name("test_module::test_completed")]);

    let (pending, completed_results) = retry(recover_unattributed_crash(&partition, &completed));

    assert_eq!(completed_results, 1);
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

    let (pending, completed_results) = retry(recover_unattributed_crash(&partition, &completed));

    assert_eq!(completed_results, 1);
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

    let (first_retry, completed_results) =
        retry(recover_unattributed_crash(&partition, &HashSet::new()));
    let second_retry = recover_unattributed_crash(&first_retry, &HashSet::new());

    assert_eq!(completed_results, 0);
    assert!(matches!(
        second_retry,
        UnattributedCrashRecovery::Stalled {
            completed_results: 0
        }
    ));
}

#[test]
fn crash_recovery_reports_when_every_result_is_committed() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_completed"), 1);
    let completed = HashSet::from([TestCacheKey::function_name("test_module::test_completed")]);

    let recovery = recover_unattributed_crash(&partition, &completed);

    assert!(matches!(
        recovery,
        UnattributedCrashRecovery::Complete {
            completed_results: 1
        }
    ));
}

#[test]
fn crash_recovery_compares_progress_with_the_filtered_assignment() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_completed"), 1);
    partition.add_test(test_info("test_module::test_pending"), 1);
    let completed = HashSet::from([TestCacheKey::function_name("test_module::test_completed")]);

    let (first_retry, _) = retry(recover_unattributed_crash(&partition, &completed));
    let second_recovery = recover_unattributed_crash(&first_retry, &completed);

    assert!(matches!(
        second_recovery,
        UnattributedCrashRecovery::Stalled {
            completed_results: 0
        }
    ));
}

#[test]
fn crash_recovery_retries_again_after_assignment_local_progress() {
    let mut partition = Partition::new();
    for name in [
        "test_module::test_a",
        "test_module::test_b",
        "test_module::test_c",
    ] {
        partition.add_test(test_info(name), 1);
    }
    let mut completed = HashSet::from([TestCacheKey::function_name("test_module::test_a")]);
    let (first_retry, _) = retry(recover_unattributed_crash(&partition, &completed));
    completed.insert(TestCacheKey::function_name("test_module::test_b"));

    let (second_retry, completed_results) =
        retry(recover_unattributed_crash(&first_retry, &completed));
    let third_recovery = recover_unattributed_crash(&second_retry, &completed);

    assert_eq!(completed_results, 1);
    assert_eq!(
        second_retry.test_paths().collect::<Vec<_>>(),
        ["test_module::test_c"]
    );
    assert!(matches!(
        third_recovery,
        UnattributedCrashRecovery::Stalled {
            completed_results: 0
        }
    ));
}

#[test]
fn active_crash_replacement_stops_after_unattributed_stall() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_crash"), 1);
    partition.add_test(test_info("test_module::test_pending"), 1);

    let pending = pending_after_test_crash(
        &partition,
        &HashSet::new(),
        &TestCacheKey::function_name("test_module::test_crash"),
    );
    let recovery = recover_unattributed_crash(&pending, &HashSet::new());

    assert!(matches!(
        recovery,
        UnattributedCrashRecovery::Stalled {
            completed_results: 0
        }
    ));
}

#[test]
fn recovery_progress_excludes_parameter_cases_owned_by_other_workers() {
    let mut partition = Partition::new();
    partition.add_test(test_info("test_module::test_case[0]"), 1);
    let mut completed = HashSet::from([TestCacheKey::parameter_case_name(
        "test_module::test_case",
        1,
    )]);

    let (retry, completed_results) = retry(recover_unattributed_crash(&partition, &completed));
    completed.insert(TestCacheKey::parameter_case_name(
        "test_module::test_case",
        2,
    ));
    let recovery = recover_unattributed_crash(&retry, &completed);

    assert_eq!(completed_results, 0);
    assert!(matches!(
        recovery,
        UnattributedCrashRecovery::Stalled {
            completed_results: 0
        }
    ));
}
