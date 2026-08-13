use std::collections::{HashMap, HashSet};
use std::time::Duration;

use karva_python_semantic::TestCacheKey;

use super::super::{Partition, TestOrdering, partition_collected_tests, scheduled_test_count};
use super::helpers::{collected_package, collected_package_with_files};

#[test]
fn stable_partitioning_preserves_module_order_after_grouping() {
    let (_temp_dir, test_paths, package) = collected_package_with_files([
        ("test_c.py", "def test_1(): pass\n"),
        ("test_a.py", "def test_1(): pass\n"),
        ("test_b.py", "def test_1(): pass\n"),
    ]);

    let partitions = partition_collected_tests(
        &package,
        2,
        &HashMap::new(),
        &HashSet::new(),
        None,
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [
            format!("{}::test_1", test_paths["test_a.py"]),
            format!("{}::test_1", test_paths["test_c.py"]),
        ]
    );
    assert_eq!(
        partitions[1].test_paths().collect::<Vec<_>>(),
        [format!("{}::test_1", test_paths["test_b.py"])]
    );
}

#[test]
fn literal_parametrize_cases_split_across_workers() {
    let (_temp_dir, _test_path, package) = collected_package(
        "@karva.tags.parametrize('value', [0, 1, 2, 3, 4, 5])\n\
         def test_value(value): pass\n",
    );

    let partitions = partition_collected_tests(
        &package,
        2,
        &HashMap::new(),
        &HashSet::new(),
        None,
        TestOrdering::Stable,
    );

    assert_eq!(scheduled_test_count(&package), 6);
    assert!(partitions.iter().all(|partition| !partition.is_empty()));
    assert_eq!(
        partitions.iter().map(Partition::test_count).sum::<usize>(),
        6
    );
}

#[test]
fn dynamic_parametrize_cases_remain_one_unit() {
    let (_temp_dir, test_path, package) = collected_package(
        "@karva.tags.parametrize('value', range(6))\n\
         def test_value(value): pass\n",
    );

    let partitions = partition_collected_tests(
        &package,
        2,
        &HashMap::new(),
        &HashSet::new(),
        None,
        TestOrdering::Stable,
    );

    assert_eq!(scheduled_test_count(&package), 1);
    assert_eq!(
        partitions
            .iter()
            .flat_map(Partition::test_paths)
            .collect::<Vec<_>>(),
        [format!("{test_path}::test_value")]
    );
}

#[test]
fn last_failed_case_selects_opaque_dynamic_parameter_function() {
    let (_temp_dir, test_path, package) = collected_package(
        "@karva.tags.parametrize('value', range(6))\n\
         def test_value(value): pass\n",
    );
    let last_failed = HashSet::from([TestCacheKey::function_name("test_sample::test_value[2]")]);

    let partitions = partition_collected_tests(
        &package,
        1,
        &HashMap::new(),
        &last_failed,
        None,
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions
            .iter()
            .flat_map(Partition::test_paths)
            .collect::<Vec<_>>(),
        [format!("{test_path}::test_value")]
    );
}

#[test]
fn one_literal_parametrize_case_uses_indexed_selector_and_legacy_duration() {
    let (_temp_dir, test_path, package) = collected_package(
        "@karva.tags.parametrize('value', [1])\n\
         def test_value(value): pass\n",
    );
    let durations = HashMap::from([(
        TestCacheKey::function_name("test_sample::test_value"),
        Duration::from_millis(10),
    )]);

    let partitions = partition_collected_tests(
        &package,
        1,
        &durations,
        &HashSet::new(),
        None,
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [format!("{test_path}::test_value[0]")]
    );
    assert_eq!(partitions[0].weight(), 10_000);
}

#[test]
fn literal_parametrize_cases_share_legacy_function_duration() {
    let (_temp_dir, _test_path, package) = collected_package(
        "@karva.tags.parametrize('value', [0, 1, 2, 3, 4, 5])\n\
         def test_value(value): pass\n",
    );
    let durations = HashMap::from([(
        TestCacheKey::function_name("test_sample::test_value"),
        Duration::from_millis(60),
    )]);

    let partitions = partition_collected_tests(
        &package,
        2,
        &durations,
        &HashSet::new(),
        None,
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions.iter().map(Partition::weight).sum::<u128>(),
        60_000
    );
}
