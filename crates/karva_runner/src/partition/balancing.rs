//! Module-aware worker load balancing.

use std::cell::OnceCell;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use karva_cli::PartitionSelection;
use karva_python_semantic::TestCacheKey;

use super::collection::{TestInfo, collect_test_paths_recursive};
use super::ordering::{order_tests_for_partitioning, seeded_order_key};
use super::{Partition, TestOrdering};
use crate::orchestration::{FailurePriority, LastFailedSelection};

/// Tests sharing one module import, weighted as a unit before large groups split.
#[derive(Debug)]
struct ModuleGroup {
    /// Tests collected from this module.
    tests: Vec<TestInfo>,

    /// Sum of measured or estimated test durations in microseconds.
    total_weight: u128,
}

impl ModuleGroup {
    fn new(tests: Vec<TestInfo>, total_weight: u128) -> Self {
        Self {
            tests,
            total_weight,
        }
    }

    fn add_test(&mut self, test: TestInfo, test_weight: u128) {
        self.tests.push(test);
        self.total_weight += test_weight;
    }

    fn weight(&self) -> u128 {
        self.total_weight
    }
}

/// Partitions collected tests with module-aware greedy load balancing.
///
/// Filtering happens before ordering so explicit slices remain stable over the
/// selected set. Unseeded runs group tests by importable module: light modules
/// stay intact to avoid repeated imports and fixture setup, while heavy modules
/// split test-by-test to limit worker skew. Seeded runs instead derive both
/// order and worker assignment from stable test identities, independent of
/// duration history and sibling tests.
///
/// Unmeasured tests use the mean duration of measured tests in the selected
/// workload, with a one-microsecond minimum to balance runs with no history.
///
/// With `defer_benchmarks`, the second partition contains selected benchmark
/// modules for serial execution; their durations do not affect ordinary balancing.
#[expect(
    clippy::too_many_arguments,
    reason = "partitioning combines independent scheduling inputs"
)]
pub fn partition_collected_tests(
    package: &karva_collector::CollectedPackage,
    num_workers: usize,
    previous_durations: &HashMap<TestCacheKey, Duration>,
    last_failed_cache: &HashSet<TestCacheKey>,
    last_failed: LastFailedSelection,
    failed_first: FailurePriority,
    partition_selection: Option<PartitionSelection>,
    test_ordering: TestOrdering,
    defer_benchmarks: bool,
) -> (Vec<Partition>, Option<Partition>) {
    let mut test_infos = Vec::new();
    collect_test_paths_recursive(package, &mut test_infos, previous_durations);

    let failed_function_roots = OnceCell::new();
    if last_failed.is_last_failed() && !last_failed_cache.is_empty() {
        test_infos
            .retain(|info| is_cached_failure(info, last_failed_cache, &failed_function_roots));
    }

    // Explicit partitioning uses deterministic ordering of post-filter tests.
    if let Some(selection) = partition_selection {
        test_infos.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        let mut position = 0usize;
        test_infos.retain(|info| {
            let keep = selection.contains_test(position, &info.qualified_name);
            position += 1;
            keep
        });
    }

    let prioritize_failures = failed_first.is_failed_first()
        && !last_failed_cache.is_empty()
        && test_infos
            .iter()
            .any(|test| is_cached_failure(test, last_failed_cache, &failed_function_roots));

    order_tests_for_partitioning(&mut test_infos, test_ordering);

    // Select and order the complete workload first, then keep measurement
    // durations out of ordinary module grouping and worker weights.
    let mut benchmarks = defer_benchmarks
        .then(|| {
            let mut partition = Partition::new();
            for test in test_infos.extract_if(.., |test| test.identity.benchmark) {
                let weight = test.duration.map_or(1, |duration| duration.as_micros());
                partition.add_test(test, weight);
            }
            partition
        })
        .filter(|partition| !partition.is_empty());
    if prioritize_failures && let Some(partition) = &mut benchmarks {
        partition.prioritize_failures(last_failed_cache, previous_durations);
    }

    if let TestOrdering::SeededShuffle(seed) = test_ordering {
        let mut partitions = partition_shuffled_tests(test_infos, num_workers, seed);
        if prioritize_failures {
            for partition in &mut partitions {
                partition.prioritize_failures(last_failed_cache, previous_durations);
            }
        }
        return (partitions, benchmarks);
    }

    let unmeasured_weight = estimate_unmeasured_weight(&test_infos);
    let test_weight = |test: &TestInfo| {
        test.duration
            .map_or(unmeasured_weight, |duration| duration.as_micros())
    };
    let mut module_groups: Vec<ModuleGroup> = Vec::new();
    let mut module_indices: HashMap<Arc<str>, usize> = HashMap::new();
    for test_info in test_infos {
        let weight = test_weight(&test_info);
        if let Some(&index) = module_indices.get(&test_info.identity.module_name) {
            module_groups[index].add_test(test_info, weight);
        } else {
            module_indices.insert(
                Arc::clone(&test_info.identity.module_name),
                module_groups.len(),
            );
            module_groups.push(ModuleGroup::new(vec![test_info], weight));
        }
    }

    let total_weight: u128 = module_groups.iter().map(ModuleGroup::weight).sum();
    let target_partition_weight = total_weight / num_workers.max(1) as u128;
    let split_threshold = target_partition_weight / 2;
    let (mut small_modules, mut large_modules): (Vec<_>, Vec<_>) = module_groups
        .into_iter()
        .partition(|module| module.weight() < split_threshold);

    small_modules.sort_by_key(|module| Reverse(module.weight()));
    let mut partitions: Vec<Partition> = (0..num_workers).map(|_| Partition::new()).collect();

    for module_group in small_modules {
        let min_partition_idx = find_lightest_partition(&partitions);
        for test_info in module_group.tests {
            let weight = test_weight(&test_info);
            partitions[min_partition_idx].add_test(test_info, weight);
        }
    }

    for module_group in &mut large_modules {
        module_group
            .tests
            .sort_by_key(|test| Reverse((test_weight(test), test.duration)));
        for test_info in module_group.tests.drain(..) {
            let weight = test_weight(&test_info);
            let min_partition_idx = find_lightest_partition(&partitions);
            partitions[min_partition_idx].add_test(test_info, weight);
        }
    }

    if prioritize_failures {
        for partition in &mut partitions {
            partition.prioritize_failures(last_failed_cache, previous_durations);
        }
    }

    (partitions, benchmarks)
}

/// Assign shuffled tests by stable identity-derived worker index.
pub(super) fn partition_shuffled_tests(
    test_infos: Vec<TestInfo>,
    num_workers: usize,
    seed: u64,
) -> Vec<Partition> {
    let mut partitions: Vec<Partition> = (0..num_workers).map(|_| Partition::new()).collect();
    if partitions.is_empty() {
        return partitions;
    }

    for test_info in test_infos {
        let key = seeded_order_key(seed, &test_info.qualified_name);
        let partition_index = seeded_partition_index(key, partitions.len());
        partitions[partition_index].add_test(test_info, 1);
    }
    partitions
}

fn estimate_unmeasured_weight(tests: &[TestInfo]) -> u128 {
    let (total, count) = tests
        .iter()
        .filter_map(|test| test.duration)
        .fold((0u128, 0u128), |(total, count), duration| {
            (total + duration.as_micros(), count + 1)
        });
    total.checked_div(count).unwrap_or(1).max(1)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "the remainder is below a worker count that already fits usize"
)]
fn seeded_partition_index(key: u64, num_workers: usize) -> usize {
    (key % num_workers as u64) as usize
}

fn find_lightest_partition(partitions: &[Partition]) -> usize {
    partitions
        .iter()
        .enumerate()
        .min_by_key(|(_, partition)| partition.weight())
        .map_or(0, |(index, _)| index)
}

/// Matches exact cases and legacy function keys, including runtime-expanded
/// failures for functions whose case count is opaque to the controller. The
/// borrowed function index is built only when exact and function keys miss.
fn is_cached_failure<'a>(
    test: &TestInfo,
    last_failed: &'a HashSet<TestCacheKey>,
    failed_function_roots: &OnceCell<HashSet<&'a str>>,
) -> bool {
    last_failed.contains(test.qualified_name.as_str())
        || last_failed.contains(test.identity.function_root.as_ref())
        || (test.qualified_name == test.identity.function_root.as_ref()
            && failed_function_roots
                .get_or_init(|| {
                    last_failed
                        .iter()
                        .map(TestCacheKey::test_function_name)
                        .collect()
                })
                .contains(test.identity.function_root.as_ref()))
}
