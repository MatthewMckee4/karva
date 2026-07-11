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
    module_name: String,
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

    fn add_test(&mut self, test: TestInfo, test_weight: u128) {
        self.tests.push(test);
        self.total_weight += test_weight;
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
    let mut test_infos = Vec::new();
    collect_test_paths_recursive(package, &mut test_infos, previous_durations);

    if !last_failed.is_empty() {
        test_infos.retain(|info| last_failed.contains(&info.qualified_name));
    }

    // Explicit partitioning runs on a deterministic ordering of the
    // post-filter test set so that `slice:M/N` is stable across runs and
    // machines. `hash:M/N` does not depend on the position, but sharing the
    // same ordering keeps the selected worker input stable too.
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

    // Step 1: Group tests by module and calculate module weights, preserving
    // the order chosen above for the first test seen from each module.
    let mut module_groups: Vec<ModuleGroup> = Vec::new();
    let mut module_indices: HashMap<String, usize> = HashMap::new();

    for test_info in test_infos {
        let weight = test_weight(test_info.duration);

        if let Some(&index) = module_indices.get(&test_info.module_name) {
            module_groups[index].add_test(test_info, weight);
        } else {
            module_indices.insert(test_info.module_name.clone(), module_groups.len());
            module_groups.push(ModuleGroup::new(vec![test_info], weight));
        }
    }

    // Step 2: Calculate threshold for splitting decision
    let total_weight: u128 = module_groups.iter().map(ModuleGroup::weight).sum();
    let target_partition_weight = total_weight / num_workers.max(1) as u128;
    let split_threshold = target_partition_weight / 2;

    // Step 3: Classify modules as small (keep together) or large (allow splitting)
    let mut small_modules = Vec::new();
    let mut large_modules = Vec::new();

    for module_group in module_groups {
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

fn order_tests_for_partitioning(test_infos: &mut [TestInfo], ordering: TestOrdering) {
    test_infos.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));

    match ordering {
        TestOrdering::ShuffleUnknownDurations => shuffle_tests_without_durations(test_infos),
        TestOrdering::Stable => {}
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
            let qualified_name = format!("{}::{}", module.path.module_name(), test_fn_def.name);
            let duration = previous_durations.get(&qualified_name).copied();

            test_infos.push(TestInfo {
                module_name: module.path.module_name().to_string(),
                qualified_name,
                path: format!("{}::{}", module.path.path(), test_fn_def.name),
                duration,
            });
        }
    }

    for subpackage in package.packages.values() {
        collect_test_paths_recursive(subpackage, test_infos, previous_durations);
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use karva_collector::{CollectedPackage, CollectionSettings, collect_file};
    use ruff_python_ast::PythonVersion;

    use super::*;

    fn test_info(qualified_name: &str) -> TestInfo {
        test_info_with_duration(qualified_name, None)
    }

    fn test_info_with_duration(qualified_name: &str, duration: Option<Duration>) -> TestInfo {
        TestInfo {
            module_name: "test_module".to_string(),
            qualified_name: qualified_name.to_string(),
            path: qualified_name.to_string(),
            duration,
        }
    }

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
        let duration = Some(Duration::from_millis(1));
        let mut tests = vec![
            test_info_with_duration("test_module::test_c", duration),
            test_info_with_duration("test_module::test_a", duration),
            test_info_with_duration("test_module::test_b", duration),
        ];

        order_tests_for_partitioning(&mut tests, TestOrdering::ShuffleUnknownDurations);

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
    fn partition_selection_filters_after_sorting_by_qualified_name() {
        let (_temp_dir, test_path, package) = collected_package(
            "def test_c(): pass\n\
             def test_a(): pass\n\
             def test_b(): pass\n",
        );
        let selection = "slice:2/3"
            .parse::<PartitionSelection>()
            .expect("valid partition selection");

        let partitions = partition_collected_tests(
            &package,
            1,
            &HashMap::new(),
            &HashSet::new(),
            Some(selection),
            TestOrdering::Stable,
        );

        assert_eq!(partitions[0].tests(), &[format!("{test_path}::test_b")]);
    }

    #[test]
    fn last_failed_filters_before_explicit_partition_selection() {
        let (_temp_dir, test_path, package) = collected_package(
            "def test_c(): pass\n\
             def test_a(): pass\n\
             def test_b(): pass\n\
             def test_d(): pass\n",
        );
        let selection = "slice:2/2"
            .parse::<PartitionSelection>()
            .expect("valid partition selection");
        let last_failed = HashSet::from([
            "test_sample::test_b".to_string(),
            "test_sample::test_c".to_string(),
        ]);

        let partitions = partition_collected_tests(
            &package,
            1,
            &HashMap::new(),
            &last_failed,
            Some(selection),
            TestOrdering::Stable,
        );

        assert_eq!(partitions[0].tests(), &[format!("{test_path}::test_c")]);
    }

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
            partitions[0].tests(),
            &[
                format!("{}::test_1", test_paths["test_a.py"]),
                format!("{}::test_1", test_paths["test_c.py"]),
            ]
        );
        assert_eq!(
            partitions[1].tests(),
            &[format!("{}::test_1", test_paths["test_b.py"])]
        );
    }

    fn collected_package(source: &str) -> (tempfile::TempDir, Utf8PathBuf, CollectedPackage) {
        let (temp_dir, mut test_paths, package) =
            collected_package_with_files([("test_sample.py", source)]);
        let test_path = test_paths
            .remove("test_sample.py")
            .expect("test path should exist");

        (temp_dir, test_path, package)
    }

    fn collected_package_with_files<const N: usize>(
        files: [(&str, &str); N],
    ) -> (
        tempfile::TempDir,
        HashMap<String, Utf8PathBuf>,
        CollectedPackage,
    ) {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf())
            .expect("temp path should be UTF-8");
        let settings = CollectionSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test_",
            respect_ignore_files: true,
            collect_fixtures: false,
        };
        let mut package = CollectedPackage::new(root);
        let mut test_paths = HashMap::new();

        for (name, source) in files {
            let test_path = package.path.join(name);
            std::fs::write(&test_path, source).expect("write test file");
            let module = collect_file(&test_path, &package.path, &settings, &[])
                .expect("collect test file")
                .expect("test file should collect");
            package.add_module(module);
            test_paths.insert(name.to_string(), test_path);
        }

        (temp_dir, test_paths, package)
    }
}
