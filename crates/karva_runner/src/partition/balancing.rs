//! Module-aware worker load balancing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use karva_cli::PartitionSelection;
use karva_python_semantic::TestCacheKey;

use super::collection::{TestInfo, collect_test_paths_recursive};
use super::ordering::{order_tests_for_partitioning, seeded_order_key};
use super::{Partition, TestOrdering};

/// Tests sharing one module import, weighted as a unit before large groups split.
#[derive(Debug)]
struct ModuleGroup {
    /// Tests collected from this module.
    tests: Vec<TestInfo>,

    /// Sum of historical microseconds, using one for each unknown duration.
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
pub fn partition_collected_tests(
    package: &karva_collector::CollectedPackage,
    num_workers: usize,
    previous_durations: &HashMap<TestCacheKey, Duration>,
    last_failed: &HashSet<TestCacheKey>,
    partition_selection: Option<PartitionSelection>,
    test_ordering: TestOrdering,
) -> Vec<Partition> {
    let mut test_infos = Vec::new();
    collect_test_paths_recursive(package, &mut test_infos, previous_durations);

    if !last_failed.is_empty() {
        let failed_function_roots = last_failed
            .iter()
            .map(TestCacheKey::test_function_name)
            .collect::<HashSet<_>>();
        test_infos.retain(|info| {
            last_failed.contains(info.qualified_name.as_str())
                || last_failed.contains(info.function_root.as_ref())
                || (info.qualified_name == info.function_root.as_ref()
                    && failed_function_roots.contains(info.function_root.as_ref()))
        });
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

    order_tests_for_partitioning(&mut test_infos, test_ordering);

    if let TestOrdering::SeededShuffle(seed) = test_ordering {
        return partition_shuffled_tests(test_infos, num_workers, seed);
    }

    let mut module_groups: Vec<ModuleGroup> = Vec::new();
    let mut module_indices: HashMap<Arc<str>, usize> = HashMap::new();
    for test_info in test_infos {
        let weight = test_weight(test_info.duration);
        if let Some(&index) = module_indices.get(&test_info.module_name) {
            module_groups[index].add_test(test_info, weight);
        } else {
            module_indices.insert(Arc::clone(&test_info.module_name), module_groups.len());
            module_groups.push(ModuleGroup::new(vec![test_info], weight));
        }
    }

    let total_weight: u128 = module_groups.iter().map(ModuleGroup::weight).sum();
    let target_partition_weight = total_weight / num_workers.max(1) as u128;
    let split_threshold = target_partition_weight / 2;
    let (mut small_modules, mut large_modules): (Vec<_>, Vec<_>) = module_groups
        .into_iter()
        .partition(|module| module.weight() < split_threshold);

    small_modules.sort_by_key(|module| std::cmp::Reverse(module.weight()));
    let mut partitions: Vec<Partition> = (0..num_workers).map(|_| Partition::new()).collect();

    for module_group in small_modules {
        let min_partition_idx = find_lightest_partition(&partitions);
        for test_info in module_group.tests {
            let weight = test_weight(test_info.duration);
            partitions[min_partition_idx].add_test(test_info, weight);
        }
    }

    for module_group in &mut large_modules {
        module_group.tests.sort_by(compare_test_weights);
        for test_info in module_group.tests.drain(..) {
            let weight = test_weight(test_info.duration);
            let min_partition_idx = find_lightest_partition(&partitions);
            partitions[min_partition_idx].add_test(test_info, weight);
        }
    }

    partitions
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

fn test_weight(duration: Option<Duration>) -> u128 {
    duration.map_or(1, |duration| duration.as_micros())
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

fn compare_test_weights(a: &TestInfo, b: &TestInfo) -> std::cmp::Ordering {
    match (&a.duration, &b.duration) {
        (Some(duration_a), Some(duration_b)) => duration_b.cmp(duration_a),
        (None, None) => std::cmp::Ordering::Equal,
        (None, _) => std::cmp::Ordering::Greater,
        (_, None) => std::cmp::Ordering::Less,
    }
}
