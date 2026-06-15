use std::collections::{HashMap, HashSet};
use std::time::Duration;

use karva_cli::PartitionSelection;

/// Ordering strategy for partition inputs.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TestOrdering {
    /// Randomize unknown-duration tests to avoid sticky first-run imbalance.
    ShuffleUnknownDurations,
    /// Use qualified-name ordering for deterministic benchmark inputs.
    Stable,
}

/// Test metadata used for partitioning decisions
#[derive(Debug, Clone)]
struct TestInfo {
    module_index: usize,
    /// The qualified name of the test (e.g., `test_a::test_1`), used for last-failed filtering.
    qualified_name: String,
    path: String,
    /// Actual runtime from previous test run (if available)
    duration: Option<Duration>,
}

/// Calculate the weight of a test for partitioning.
///
/// Uses the actual duration in microseconds if available, otherwise defaults to 1.
fn test_weight(duration: Option<Duration>) -> u128 {
    duration.map_or(1, |d| d.as_micros())
}

/// A group of tests from the same module with calculated weight
#[derive(Debug)]
struct ModuleGroup {
    tests: Vec<TestInfo>,
    /// Total weight of all tests in this module
    total_weight: u128,
}

impl ModuleGroup {
    fn new(tests: Vec<TestInfo>, total_weight: u128) -> Self {
        Self {
            tests,
            total_weight,
        }
    }

    fn weight(&self) -> u128 {
        self.total_weight
    }
}

/// A partition of tests assigned to a single worker
#[derive(Debug)]
pub struct Partition {
    tests: Vec<String>,
    /// Cumulative weight (duration in microseconds or 1 for unknown tests)
    weight: u128,
}

impl Partition {
    fn new() -> Self {
        Self {
            tests: Vec::new(),
            weight: 0,
        }
    }

    fn add_test(&mut self, test: TestInfo, test_weight: u128) {
        self.tests.push(test.path);
        self.weight += test_weight;
    }

    fn weight(&self) -> u128 {
        self.weight
    }

    pub(crate) fn tests(&self) -> &[String] {
        &self.tests
    }
}

/// Partition collected tests into N groups using module-aware greedy bin-packing
///
/// # Algorithm: Hybrid Module-Aware LPT (Longest Processing Time First)
///
/// This implements a hybrid approach that balances load while minimizing module imports:
///
/// 1. **Group**: Tests are grouped by module and module weights are calculated
/// 2. **Classify**: Modules are classified as "small" or "large" based on a threshold
/// 3. **Assign Small Modules**: Small modules are assigned atomically to partitions (no splitting)
/// 4. **Split Large Modules**: Large modules are split using LPT to prevent imbalance
///
/// ## Module Grouping Benefits
/// - **Reduced imports**: Tests from the same module stay together in one partition
/// - **Faster startup**: Each partition loads fewer unique modules
/// - **Shared fixtures**: Fixture setup/teardown happens once per module per partition
///
/// ## Threshold Strategy
/// The split threshold is set to `(total_weight / num_workers) / 2`:
/// - Modules below this are kept together (typical case)
/// - Modules above this are split to prevent worker imbalance
///
/// ## Complexity
/// - Time: O(n log n + m log m + n*w) where n = tests, m = modules, w = workers
/// - Space: O(n + m + w)
/// - Since m ≤ n and w is small (4-16), this is effectively O(n log n)
///
/// ## Weighting Strategy
/// - **With historical data**: Uses actual test duration in microseconds
/// - **Without historical data**: Tests are shuffled randomly and assigned with equal weight
pub fn partition_collected_tests(
    package: &karva_collector::CollectedPackage,
    num_workers: usize,
    previous_durations: &HashMap<String, Duration>,
    last_failed: &HashSet<String>,
    partition_selection: Option<PartitionSelection>,
    test_ordering: TestOrdering,
) -> Vec<Partition> {
    let mut test_infos = Vec::with_capacity(package.test_count());
    let mut module_index = 0;
    collect_test_paths_recursive(
        package,
        &mut test_infos,
        previous_durations,
        &mut module_index,
    );

    if !last_failed.is_empty() {
        test_infos.retain(|info| last_failed.contains(&info.qualified_name));
    }

    // Slice partitioning runs on a deterministic ordering of the post-filter
    // test set so that `slice:M/N` is stable across runs and machines (modulo
    // changes to the test set itself).
    if let Some(selection) = partition_selection {
        test_infos.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        let mut position = 0usize;
        test_infos.retain(|_| {
            let keep = selection.contains(position);
            position += 1;
            keep
        });
    }

    order_tests_for_partitioning(&mut test_infos, test_ordering, num_workers);

    if num_workers == 1 {
        return vec![single_partition(test_infos)];
    }

    // Step 1: Group tests by module and calculate module weights
    let mut module_groups: Vec<Vec<TestInfo>> = (0..module_index).map(|_| Vec::new()).collect();
    let mut module_weights = vec![0; module_index];

    for test_info in test_infos {
        let weight = test_weight(test_info.duration);
        let module_index = test_info.module_index;

        module_weights[module_index] += weight;
        module_groups[module_index].push(test_info);
    }

    // Step 2: Calculate threshold for splitting decision
    let total_weight: u128 = module_weights.iter().sum();
    let target_partition_weight = total_weight / num_workers.max(1) as u128;
    let split_threshold = target_partition_weight / 2;

    // Step 3: Classify modules as small (keep together) or large (allow splitting)
    let mut small_modules = Vec::new();
    let mut large_modules = Vec::new();

    for (module_index, tests) in module_groups.into_iter().enumerate() {
        if tests.is_empty() {
            continue;
        }

        let weight = module_weights[module_index];
        let module_group = ModuleGroup::new(tests, weight);

        if module_group.weight() < split_threshold {
            small_modules.push(module_group);
        } else {
            large_modules.push(module_group);
        }
    }

    // Sort small modules by weight (descending) for better bin-packing
    small_modules.sort_by_key(|module| std::cmp::Reverse(module.weight()));

    let mut partitions: Vec<Partition> = (0..num_workers).map(|_| Partition::new()).collect();

    // Step 4: Assign small modules atomically (entire module to one partition)
    for module_group in small_modules {
        let min_partition_idx = find_lightest_partition(&partitions);
        for test_info in module_group.tests {
            let weight = test_weight(test_info.duration);
            partitions[min_partition_idx].add_test(test_info, weight);
        }
    }

    // Step 5: Split large modules using LPT to prevent imbalance
    for mut module_group in large_modules {
        // Sort tests within large modules by weight (descending)
        module_group.tests.sort_by(compare_test_weights);

        for test_info in module_group.tests {
            let weight = test_weight(test_info.duration);
            let min_partition_idx = find_lightest_partition(&partitions);
            partitions[min_partition_idx].add_test(test_info, weight);
        }
    }

    partitions
}

/// Finds the index of the partition with the smallest weight
fn find_lightest_partition(partitions: &[Partition]) -> usize {
    partitions
        .iter()
        .enumerate()
        .min_by_key(|(_, partition)| partition.weight())
        .map_or(0, |(idx, _)| idx)
}

/// Compares two tests by duration descending; tests without durations are considered equal
fn compare_test_weights(a: &TestInfo, b: &TestInfo) -> std::cmp::Ordering {
    match (&a.duration, &b.duration) {
        (Some(dur_a), Some(dur_b)) => dur_b.cmp(dur_a),
        (None, None) => std::cmp::Ordering::Equal,
        (None, _) => std::cmp::Ordering::Greater,
        (_, None) => std::cmp::Ordering::Less,
    }
}

fn single_partition(test_infos: Vec<TestInfo>) -> Partition {
    let mut partition = Partition::new();
    for test_info in test_infos {
        let weight = test_weight(test_info.duration);
        partition.add_test(test_info, weight);
    }
    partition
}

/// Shuffles only the tests that have no historical duration data.
///
/// This ensures tests without timing info are randomly distributed across partitions
/// rather than always landing in the same order.
fn shuffle_tests_without_durations(test_infos: &mut [TestInfo]) {
    let no_duration_indices: Vec<usize> = test_infos
        .iter()
        .enumerate()
        .filter(|(_, t)| t.duration.is_none())
        .map(|(i, _)| i)
        .collect();

    // Fisher-Yates shuffle on the indices
    for i in (1..no_duration_indices.len()).rev() {
        let j = fastrand::usize(..=i);
        let idx_a = no_duration_indices[i];
        let idx_b = no_duration_indices[j];
        test_infos.swap(idx_a, idx_b);
    }
}

fn order_tests_for_partitioning(
    test_infos: &mut [TestInfo],
    ordering: TestOrdering,
    num_workers: usize,
) {
    match ordering {
        TestOrdering::ShuffleUnknownDurations => {
            if num_workers > 1 {
                shuffle_tests_without_durations(test_infos);
            }
        }
        TestOrdering::Stable => {
            test_infos.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));
        }
    }
}

/// Recursively collects test information from a package and all its subpackages
fn collect_test_paths_recursive(
    package: &karva_collector::CollectedPackage,
    test_infos: &mut Vec<TestInfo>,
    previous_durations: &HashMap<String, Duration>,
    next_module_index: &mut usize,
) {
    for module in package.modules.values() {
        let module_index = *next_module_index;
        *next_module_index += 1;

        for test_fn_def in &module.test_function_defs {
            let qualified_name = format!("{}::{}", module.path.module_name(), test_fn_def.name);
            let duration = previous_durations.get(&qualified_name).copied();

            test_infos.push(TestInfo {
                module_index,
                qualified_name,
                path: format!("{}::{}", module.path.path(), test_fn_def.name),
                duration,
            });
        }
    }

    for subpackage in package.packages.values() {
        collect_test_paths_recursive(
            subpackage,
            test_infos,
            previous_durations,
            next_module_index,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_info(qualified_name: &str) -> TestInfo {
        TestInfo {
            module_index: 0,
            qualified_name: qualified_name.to_string(),
            path: qualified_name.to_string(),
            duration: None,
        }
    }

    #[test]
    fn deterministic_partitioning_sorts_by_qualified_name() {
        let mut tests = vec![
            test_info("test_module::test_c"),
            test_info("test_module::test_a"),
            test_info("test_module::test_b"),
        ];

        order_tests_for_partitioning(&mut tests, TestOrdering::Stable, 1);

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
    fn one_worker_shuffle_ordering_preserves_input_order() {
        let mut tests = vec![
            test_info("test_module::test_c"),
            test_info("test_module::test_a"),
            test_info("test_module::test_b"),
        ];

        order_tests_for_partitioning(&mut tests, TestOrdering::ShuffleUnknownDurations, 1);

        let ordered_names: Vec<_> = tests
            .iter()
            .map(|test| test.qualified_name.as_str())
            .collect();
        assert_eq!(
            ordered_names,
            [
                "test_module::test_c",
                "test_module::test_a",
                "test_module::test_b"
            ]
        );
    }
}
