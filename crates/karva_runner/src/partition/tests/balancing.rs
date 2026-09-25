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
        false,
        false,
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
        false,
        false,
        None,
        TestOrdering::Stable,
    );

    assert_eq!(scheduled_test_count(&package), 6);
    assert!(partitions.iter().all(|partition| !partition.is_empty()));
    assert_eq!(
        partitions.iter().map(Partition::test_count).sum::<usize>(),
        6
    );
    let mut cache_keys = partitions
        .iter()
        .flat_map(Partition::cache_keys)
        .collect::<Vec<_>>();
    cache_keys.sort();
    assert_eq!(
        cache_keys,
        (0..6)
            .map(|index| TestCacheKey::function_name(&format!("test_sample::test_value[{index}]")))
            .collect::<Vec<_>>()
    );
}

#[test]
fn failed_first_places_cached_parametrize_case_first() {
    let (_temp_dir, test_path, package) = collected_package(
        "@karva.tags.parametrize('value', [0, 1, 2])\n\
         def test_value(value): pass\n",
    );
    let failed = HashSet::from([TestCacheKey::function_name("test_sample::test_value[1]")]);

    let partitions = partition_collected_tests(
        &package,
        1,
        &HashMap::new(),
        &failed,
        false,
        true,
        None,
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [
            format!("{test_path}::test_value[1]"),
            format!("{test_path}::test_value[0]"),
            format!("{test_path}::test_value[2]"),
        ]
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
        false,
        false,
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
        true,
        false,
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
        false,
        false,
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
        false,
        false,
        None,
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions.iter().map(Partition::weight).sum::<u128>(),
        60_000
    );
}

#[test]
fn failed_first_prioritizes_cached_failures_without_filtering_tests() {
    let (_temp_dir, test_path, package) = collected_package(
        "def test_a(): pass\n\
         def test_b(): pass\n\
         def test_c(): pass\n",
    );
    let last_failed = HashSet::from([TestCacheKey::function_name("test_sample::test_b")]);
    let durations = HashMap::from([
        (
            TestCacheKey::function_name("test_sample::test_a"),
            Duration::from_millis(30),
        ),
        (
            TestCacheKey::function_name("test_sample::test_b"),
            Duration::from_millis(1),
        ),
        (
            TestCacheKey::function_name("test_sample::test_c"),
            Duration::from_millis(20),
        ),
    ]);

    let partitions = partition_collected_tests(
        &package,
        1,
        &durations,
        &last_failed,
        false,
        true,
        None,
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [
            format!("{test_path}::test_b"),
            format!("{test_path}::test_a"),
            format!("{test_path}::test_c"),
        ]
    );
}

#[test]
fn failed_first_keeps_partition_selection_before_priority_ordering() {
    let (_temp_dir, test_path, package) = collected_package(
        "def test_a(): pass\n\
         def test_b(): pass\n\
         def test_c(): pass\n\
         def test_d(): pass\n",
    );
    let selection = "slice:1/2".parse().expect("valid partition selection");
    let last_failed = HashSet::from([TestCacheKey::function_name("test_sample::test_b")]);

    let partitions = partition_collected_tests(
        &package,
        1,
        &HashMap::new(),
        &last_failed,
        false,
        true,
        Some(selection),
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [
            format!("{test_path}::test_a"),
            format!("{test_path}::test_c")
        ]
    );
}

#[test]
fn failed_first_reorders_across_module_groups() {
    let (_temp_dir, test_paths, package) = collected_package_with_files([
        ("test_pass.py", "def test_pass(): pass\n"),
        ("test_fail.py", "def test_fail(): pass\n"),
    ]);
    let last_failed = HashSet::from([TestCacheKey::function_name("test_fail::test_fail")]);
    let durations = HashMap::from([
        (
            TestCacheKey::function_name("test_pass::test_pass"),
            Duration::from_micros(1),
        ),
        (
            TestCacheKey::function_name("test_fail::test_fail"),
            Duration::from_millis(10),
        ),
    ]);

    let partitions = partition_collected_tests(
        &package,
        1,
        &durations,
        &last_failed,
        false,
        true,
        None,
        TestOrdering::Stable,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [
            format!("{}::test_fail", test_paths["test_fail.py"]),
            format!("{}::test_pass", test_paths["test_pass.py"]),
        ]
    );
}
