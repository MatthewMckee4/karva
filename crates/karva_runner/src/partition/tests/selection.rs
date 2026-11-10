use std::collections::{HashMap, HashSet};
use std::time::Duration;

use indoc::indoc;
use rstest::rstest;

use karva_cli::PartitionSelection;
use karva_python_semantic::TestCacheKey;

use super::super::{Partition, TestOrdering, partition_collected_tests};
use super::helpers::{collected_package, collected_package_with_files};
use crate::orchestration::{FailurePriority, LastFailedSelection};

#[test]
fn benchmark_durations_do_not_skew_ordinary_balancing() {
    let (_directory, _paths, package) = collected_package_with_files([
        ("test_bench.py", "def test_micro(benchmark): pass\n"),
        (
            "test_large.py",
            "def test_a(): pass\ndef test_b(): pass\ndef test_c(): pass\n",
        ),
        ("test_small.py", "def test_small(): pass\n"),
    ]);
    let durations = HashMap::from([
        (
            TestCacheKey::function_name("test_bench::test_micro"),
            Duration::from_secs(6),
        ),
        (
            TestCacheKey::function_name("test_large::test_a"),
            Duration::from_secs(1),
        ),
        (
            TestCacheKey::function_name("test_large::test_b"),
            Duration::from_secs(1),
        ),
        (
            TestCacheKey::function_name("test_large::test_c"),
            Duration::from_secs(1),
        ),
        (
            TestCacheKey::function_name("test_small::test_small"),
            Duration::from_secs(1),
        ),
    ]);
    let (partitions, benchmarks) = partition_collected_tests(
        &package,
        2,
        &durations,
        &HashSet::new(),
        LastFailedSelection::All,
        FailurePriority::Normal,
        None,
        TestOrdering::Stable,
        true,
    );
    let benchmarks = benchmarks.expect("benchmark module");
    assert_eq!(benchmarks.test_count(), 1);
    assert!(
        partitions
            .iter()
            .all(|partition| partition.test_count() == 2)
    );
}

#[rstest]
fn benchmark_deferral_preserves_selected_cases(
    #[values(TestOrdering::Stable, TestOrdering::SeededShuffle(42))] ordering: TestOrdering,
) {
    let (_directory, _paths, package) = collected_package_with_files([
        (
            "test_ordinary.py",
            "def test_a(): pass\ndef test_b(): pass\n",
        ),
        (
            "test_measure.py",
            "@karva.tags.parametrize('value', [0, 1])\ndef test_micro(benchmark, value): pass\n",
        ),
    ]);
    let partition = |defer| {
        partition_collected_tests(
            &package,
            2,
            &HashMap::new(),
            &HashSet::new(),
            LastFailedSelection::All,
            FailurePriority::Normal,
            None,
            ordering,
            defer,
        )
    };
    let (original, _) = partition(false);
    let selected = original
        .iter()
        .flat_map(Partition::cache_keys)
        .collect::<HashSet<_>>();
    let (partitions, benchmarks) = partition(true);
    let benchmarks = benchmarks.expect("benchmark module");
    assert_eq!(benchmarks.test_count(), 2);
    assert!(
        benchmarks
            .cache_keys()
            .all(|key| key.to_string().starts_with("test_measure::"))
    );
    let ordinary = partitions
        .iter()
        .flat_map(Partition::cache_keys)
        .collect::<HashSet<_>>();
    assert_eq!(ordinary.len(), 2);
    assert!(
        ordinary
            .iter()
            .all(|key| key.to_string().starts_with("test_ordinary::"))
    );
    let combined = ordinary
        .into_iter()
        .chain(benchmarks.cache_keys())
        .collect::<HashSet<_>>();
    assert_eq!(selected, combined);
}

#[test]
fn partition_selection_filters_after_sorting_by_qualified_name() {
    let (_temp_dir, test_path, package) = collected_package(indoc! {r"
        def test_c(): pass
        def test_a(): pass
        def test_b(): pass
    "});
    let selection = "slice:2/3"
        .parse::<PartitionSelection>()
        .expect("valid partition selection");

    let (partitions, _) = partition_collected_tests(
        &package,
        1,
        &HashMap::new(),
        &HashSet::new(),
        LastFailedSelection::All,
        FailurePriority::Normal,
        Some(selection),
        TestOrdering::Stable,
        false,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [format!("{test_path}::test_b")]
    );
}

#[test]
fn last_failed_filters_before_explicit_partition_selection() {
    let (_temp_dir, test_path, package) = collected_package(indoc! {r"
        def test_c(): pass
        def test_a(): pass
        def test_b(): pass
        def test_d(): pass
    "});
    let selection = "slice:2/2"
        .parse::<PartitionSelection>()
        .expect("valid partition selection");
    let last_failed = HashSet::from([
        TestCacheKey::function_name("test_sample::test_b"),
        TestCacheKey::function_name("test_sample::test_c"),
    ]);

    let (partitions, _) = partition_collected_tests(
        &package,
        1,
        &HashMap::new(),
        &last_failed,
        LastFailedSelection::LastFailed,
        FailurePriority::Normal,
        Some(selection),
        TestOrdering::Stable,
        false,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [format!("{test_path}::test_c")]
    );
}

#[test]
fn cached_case_roots_select_opaque_functions_without_selecting_siblings() {
    let (_temp_dir, test_path, package) = collected_package(indoc! {r"
        @karva.tags.parametrize('value', [0, 1])
        def test_value(value): pass
        @karva.tags.parametrize('value', range(3))
        def test_dynamic(value): pass
        def test_value_extra(): pass
        def test_pass(): pass
    "});
    let failed = HashSet::from([
        TestCacheKey::parameter_case_name("test_sample::test_value", 1),
        TestCacheKey::parameter_case_name("test_sample::test_dynamic", 2),
        TestCacheKey::parameter_case_name("test_other::test_value", 0),
        TestCacheKey::parameter_case_name("test_other::test_pass", 0),
    ]);

    let (partitions, _) = partition_collected_tests(
        &package,
        1,
        &HashMap::new(),
        &failed,
        LastFailedSelection::LastFailed,
        FailurePriority::Normal,
        None,
        TestOrdering::Stable,
        false,
    );

    assert_eq!(
        partitions[0].test_paths().collect::<Vec<_>>(),
        [
            format!("{test_path}::test_dynamic"),
            format!("{test_path}::test_value[1]"),
        ]
    );
}
