use super::super::collection::TestInfo;
use super::super::{Partition, TestOrdering, order_tests_for_partitioning};
use super::helpers::{test_info, test_info_with_duration};

#[test]
fn deterministic_partitioning_sorts_by_qualified_name() {
    let mut tests = vec![
        test_info("test_module::test_c"),
        test_info("test_module::test_a"),
        test_info("test_module::test_b"),
    ];

    order_tests_for_partitioning(&mut tests, TestOrdering::Stable);

    let ordered_names: Vec<_> = tests
        .iter()
        .map(|test| test.qualified_name.as_str())
        .collect();
    assert_eq!(
        ordered_names,
        [
            "test_module::test_a",
            "test_module::test_b",
            "test_module::test_c"
        ]
    );
}

#[test]
fn duration_backed_partitioning_starts_from_qualified_name_order() {
    let duration = Some(std::time::Duration::from_millis(1));
    let mut tests = vec![
        test_info_with_duration("test_module::test_c", duration),
        test_info_with_duration("test_module::test_a", duration),
        test_info_with_duration("test_module::test_b", duration),
    ];

    order_tests_for_partitioning(&mut tests, TestOrdering::RandomizeUnmeasured);

    let ordered_names: Vec<_> = tests
        .iter()
        .map(|test| test.qualified_name.as_str())
        .collect();
    assert_eq!(
        ordered_names,
        [
            "test_module::test_a",
            "test_module::test_b",
            "test_module::test_c"
        ]
    );
}

#[test]
fn seeded_ordering_is_reproducible() {
    let tests = vec![
        test_info("test_module::test_d"),
        test_info("test_module::test_a"),
        test_info("test_module::test_c"),
        test_info("test_module::test_b"),
    ];
    let mut first = tests.clone();
    let mut repeated = tests.clone();
    let mut different_seed = tests;

    order_tests_for_partitioning(&mut first, TestOrdering::SeededShuffle(170_938));
    order_tests_for_partitioning(&mut repeated, TestOrdering::SeededShuffle(170_938));
    order_tests_for_partitioning(&mut different_seed, TestOrdering::SeededShuffle(170_939));

    let names = |tests: &[TestInfo]| {
        tests
            .iter()
            .map(|test| test.qualified_name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&first), names(&repeated));
    assert_ne!(names(&first), names(&different_seed));
}

#[test]
fn seeded_partitioning_reproduces_worker_assignment_and_order() {
    let mut tests = vec![
        test_info("test_module::test_f"),
        test_info("test_module::test_a"),
        test_info("test_module::test_e"),
        test_info("test_module::test_b"),
        test_info("test_module::test_d"),
        test_info("test_module::test_c"),
    ];
    order_tests_for_partitioning(&mut tests, TestOrdering::SeededShuffle(170_938));

    let first = super::super::partition_shuffled_tests(tests.clone(), 2, 170_938);
    let repeated = super::super::partition_shuffled_tests(tests, 2, 170_938);

    assert!(first[0].test_paths().eq(repeated[0].test_paths()));
    assert!(first[1].test_paths().eq(repeated[1].test_paths()));
    assert_eq!(first.iter().map(Partition::test_count).sum::<usize>(), 6);
}
