use std::collections::HashMap;
use std::time::Duration;

/// Test metadata used for partitioning decisions
#[derive(Debug, Clone)]
struct TestInfo {
    module_name: String,
    path: String,
    /// Actual runtime from previous test run (if available)
    duration: Option<Duration>,
}

/// A group of tests from the same module with calculated weight
#[derive(Debug)]
struct ModuleGroup {
    tests: Vec<TestInfo>,
    /// Total weight of all tests in this module
    total_weight: u128,
}

impl ModuleGroup {
    const fn new(tests: Vec<TestInfo>, total_weight: u128) -> Self {
        Self {
            tests,
            total_weight,
        }
    }

    const fn weight(&self) -> u128 {
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
    const fn new() -> Self {
        Self {
            tests: Vec::new(),
            weight: 0,
        }
    }

    fn add_test(&mut self, test: TestInfo, test_weight: u128) {
        self.tests.push(test.path);
        self.weight += test_weight;
    }

    const fn weight(&self) -> u128 {
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
) -> Vec<Partition> {
    let mut test_infos = Vec::new();
    collect_test_paths_recursive(package, &mut test_infos, previous_durations);

    // Shuffle tests without durations so they distribute randomly across partitions
    shuffle_tests_without_durations(&mut test_infos);

    // Step 1: Group tests by module and calculate module weights
    let mut module_groups: HashMap<String, Vec<TestInfo>> = HashMap::new();
    let mut module_weights: HashMap<String, u128> = HashMap::new();

    for test_info in test_infos {
        let test_weight = test_info.duration.map_or(1, |d| d.as_micros());

        *module_weights
            .entry(test_info.module_name.clone())
            .or_default() += test_weight;
        module_groups
            .entry(test_info.module_name.clone())
            .or_default()
            .push(test_info);
    }

    // Step 2: Calculate threshold for splitting decision
    let total_weight: u128 = module_weights.values().sum();
    let target_partition_weight = total_weight / num_workers.max(1) as u128;
    let split_threshold = target_partition_weight / 2;

    // Step 3: Classify modules as small (keep together) or large (allow splitting)
    let mut small_modules = Vec::new();
    let mut large_modules = Vec::new();

    for (module_name, tests) in module_groups {
        let weight = module_weights[&module_name];
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
            let test_weight = test_info.duration.map_or(1, |d| d.as_micros());
            partitions[min_partition_idx].add_test(test_info, test_weight);
        }
    }

    // Step 5: Split large modules using LPT to prevent imbalance
    for mut module_group in large_modules {
        // Sort tests within large modules by weight (descending)
        module_group.tests.sort_by(compare_test_weights);

        for test_info in module_group.tests {
            let test_weight = test_info.duration.map_or(1, |d| d.as_micros());
            let min_partition_idx = find_lightest_partition(&partitions);
            partitions[min_partition_idx].add_test(test_info, test_weight);
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

/// Recursively collects test information from a package and all its subpackages
fn collect_test_paths_recursive(
    package: &karva_collector::CollectedPackage,
    test_infos: &mut Vec<TestInfo>,
    previous_durations: &HashMap<String, Duration>,
) {
    for module in package.modules.values() {
        for test_fn_def in &module.test_function_defs {
            let path = format!("{}::{}", module.path.module_name(), test_fn_def.name);
            let duration = previous_durations.get(&path).copied();

            test_infos.push(TestInfo {
                module_name: module.path.module_name().to_string(),
                path: format!("{}::{}", module.path.path(), test_fn_def.name),
                duration,
            });
        }
    }

    for subpackage in package.packages.values() {
        collect_test_paths_recursive(subpackage, test_infos, previous_durations);
    }
}
