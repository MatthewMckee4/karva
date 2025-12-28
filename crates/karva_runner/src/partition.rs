use std::collections::HashMap;
use std::time::Duration;

/// Test metadata used for partitioning decisions
#[derive(Debug, Clone)]
struct TestInfo {
    path: String,
    /// Number of AST nodes in the test body (used as fallback heuristic)
    body_length: usize,
    /// Actual runtime from previous test run (if available)
    duration: Option<Duration>,
}

/// A partition of tests assigned to a single worker
#[derive(Debug)]
pub struct Partition {
    tests: Vec<String>,
    /// Cumulative weight (duration in microseconds or body length)
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

/// Partition collected tests into N groups using greedy bin-packing
///
/// # Algorithm: Longest Processing Time First (LPT)
///
/// This implements a greedy approximation algorithm for the bin-packing problem:
///
/// 1. **Sort**: Tests are sorted by weight (`duration` or `body_length`) in descending order
/// 2. **Assign**: Each test is assigned to the worker partition with the smallest current total
/// 3. **Balance**: This minimizes the maximum partition weight, balancing load across workers
///
/// ## Complexity
/// - Time: O(n log n + n*w) where n = tests, w = workers
/// - Space: O(n + w)
/// - Since w is typically small (4-16), this is effectively O(n log n)
///
/// ## Approximation Quality
/// LPT guarantees a solution within 33% of optimal for uniform bin-packing.
/// In practice, with historical duration data, this achieves near-optimal load balancing.
///
/// ## Weighting Strategy
/// - **With historical data**: Uses actual test duration in microseconds
/// - **Without historical data**: Falls back to AST body length as a proxy for complexity
pub fn partition_collected_tests(
    package: &karva_collector::CollectedPackage,
    num_workers: usize,
    previous_durations: &HashMap<String, Duration>,
) -> Vec<Partition> {
    let mut test_infos = Vec::new();
    collect_test_paths_recursive(package, &mut test_infos, previous_durations);

    // Step 1: Sort tests by weight (longest first)
    test_infos.sort_by(|a, b| match (&a.duration, &b.duration) {
        (Some(dur_a), Some(dur_b)) => dur_b.cmp(dur_a),
        (_, _) => b.body_length.cmp(&a.body_length),
    });

    let mut partitions: Vec<Partition> = (0..num_workers).map(|_| Partition::new()).collect();

    // Step 2: Greedy assignment - each test goes to the lightest partition
    for test_info in test_infos {
        let test_weight = test_info
            .duration
            .map_or(test_info.body_length as u128, |d| d.as_micros());

        let min_partition_idx = partitions
            .iter()
            .enumerate()
            .min_by_key(|(_, partition)| partition.weight())
            .map_or(0, |(idx, _)| idx);

        partitions[min_partition_idx].add_test(test_info, test_weight);
    }

    partitions
}

/// Recursively collects test information from a package and all its subpackages
///
/// For each test, looks up its historical duration from `previous_durations` and
/// combines it with the test's AST body length to create a `TestInfo` record.
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
                path: format!("{}::{}", module.path.path(), test_fn_def.name),
                body_length: test_fn_def.body.len(),
                duration,
            });
        }
    }

    for subpackage in package.packages.values() {
        collect_test_paths_recursive(subpackage, test_infos, previous_durations);
    }
}
