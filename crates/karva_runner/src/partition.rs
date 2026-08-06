use std::collections::{HashMap, HashSet};
use std::hash::Hasher;
use std::time::Duration;

use karva_cli::PartitionSelection;
use karva_python_semantic::TestCacheKey;
use siphasher::sip::SipHasher13;

/// Ordering strategy for partition inputs.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TestOrdering {
    /// Randomize unmeasured tests to avoid sticky first-run imbalance.
    RandomizeUnmeasured,

    /// Randomize every selected test reproducibly and ignore duration history.
    SeededShuffle(u64),

    /// Use qualified-name ordering for deterministic benchmark inputs.
    Stable,
}

/// Test metadata needed to filter, group, weight, and dispatch one test.
#[derive(Debug, Clone)]
struct TestInfo {
    /// Importable module name used to keep cheap modules together.
    module_name: String,

    /// The qualified name of the test (e.g., `test_a::test_1`), used for last-failed filtering.
    qualified_name: String,

    /// Worker CLI selector for this exact test.
    path: String,

    /// Wall-clock runtime from the previous run, when cached.
    duration: Option<Duration>,
    /// Qualified name without any `[idx]` suffix. Cases of the same
    /// parametrized function share this key so they can be shuffled and
    /// reasoned about as a single unit.
    function_root: String,
}

/// Calculate the weight of a test for partitioning.
///
/// Uses the actual duration in microseconds if available, otherwise defaults to 1.
fn test_weight(duration: Option<Duration>) -> u128 {
    duration.map_or(1, |d| d.as_micros())
}

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

/// Worker assignment produced by module-aware load balancing.
#[derive(Debug, Clone)]
pub struct Partition {
    /// Worker CLI selectors in execution order.
    tests: Vec<String>,

    /// Stable identities parallel to `tests`, used for crash recovery.
    test_keys: Vec<TestCacheKey>,

    /// Function identities represented by the selectors.
    function_roots: HashSet<String>,

    /// Cumulative historical microseconds, using one per unknown test.
    weight: u128,
}

impl Partition {
    fn new() -> Self {
        Self {
            tests: Vec::new(),
            test_keys: Vec::new(),
            function_roots: HashSet::new(),
            weight: 0,
        }
    }

    fn add_test(&mut self, test: TestInfo, test_weight: u128) {
        self.function_roots.insert(test.function_root);
        self.test_keys
            .push(TestCacheKey::function_name(&test.qualified_name));
        self.tests.push(test.path);
        self.weight += test_weight;
    }

    fn weight(&self) -> u128 {
        self.weight
    }

    pub(super) fn tests(&self) -> &[String] {
        &self.tests
    }

    pub(super) fn function_roots(&self) -> impl Iterator<Item = &str> {
        self.function_roots.iter().map(String::as_str)
    }

    /// First scheduled identity without a committed result.
    pub(super) fn first_unfinished(
        &self,
        completed: &HashSet<TestCacheKey>,
    ) -> Option<TestCacheKey> {
        self.test_keys
            .iter()
            .find(|cache_key| !completed.contains(*cache_key))
            .cloned()
    }

    /// Returns work not committed before a worker crash, excluding the test
    /// that terminated the worker.
    pub(super) fn pending_after_crash(
        &self,
        completed: &HashSet<TestCacheKey>,
        crashed: &TestCacheKey,
    ) -> Self {
        let mut pending = Self::new();
        for (path, cache_key) in self.tests.iter().zip(&self.test_keys) {
            let contains_crashed_dynamic_case = !cache_key.is_parameter_case()
                && cache_key.test_function_name() == crashed.test_function_name();
            if !completed.contains(cache_key)
                && cache_key != crashed
                && !contains_crashed_dynamic_case
            {
                pending.tests.push(path.clone());
                pending.test_keys.push(cache_key.clone());
                pending
                    .function_roots
                    .insert(cache_key.test_function_name().to_string());
            }
        }
        pending
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
/// - **With seeded shuffling**: Ignores duration history and derives ordering
///   and worker assignment from each test's identity so filters commute with it
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
        test_infos.retain(|info| {
            last_failed.contains(info.qualified_name.as_str())
                || last_failed.contains(info.function_root.as_str())
        });
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

    if let TestOrdering::SeededShuffle(seed) = test_ordering {
        return partition_shuffled_tests(test_infos, num_workers, seed);
    }

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

/// Assigns each shuffled test from its stable random key, independent of sibling tests.
fn partition_shuffled_tests(
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

#[expect(
    clippy::cast_possible_truncation,
    reason = "the remainder is below a worker count that already fits usize"
)]
fn seeded_partition_index(key: u64, num_workers: usize) -> usize {
    (key % num_workers as u64) as usize
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

/// Shuffles tests that have no historical duration data, treating cases of
/// the same parametrized function as a single unit.
///
/// Without this grouping, parametrize cases for one function would be
/// reordered relative to one another, making test output order
/// non-deterministic in the common single-worker case.
fn shuffle_tests_without_durations(test_infos: &mut Vec<TestInfo>) {
    let mut groups: Vec<Vec<TestInfo>> = Vec::new();
    let mut group_index: HashMap<String, usize> = HashMap::new();

    for info in test_infos.drain(..) {
        if let Some(idx) = group_index.get(&info.function_root) {
            groups[*idx].push(info);
        } else {
            group_index.insert(info.function_root.clone(), groups.len());
            groups.push(vec![info]);
        }
    }

    let no_duration_groups: Vec<usize> = groups
        .iter()
        .enumerate()
        .filter(|(_, g)| g.iter().any(|t| t.duration.is_none()))
        .map(|(i, _)| i)
        .collect();

    for i in (1..no_duration_groups.len()).rev() {
        let j = fastrand::usize(..=i);
        let idx_a = no_duration_groups[i];
        let idx_b = no_duration_groups[j];
        groups.swap(idx_a, idx_b);
    }

    for group in groups {
        test_infos.extend(group);
    }
}

fn order_tests_for_partitioning(test_infos: &mut Vec<TestInfo>, ordering: TestOrdering) {
    test_infos.sort_by(|a, b| a.qualified_name.cmp(&b.qualified_name));

    match ordering {
        TestOrdering::RandomizeUnmeasured => shuffle_tests_without_durations(test_infos),
        TestOrdering::SeededShuffle(seed) => {
            test_infos.sort_by_cached_key(|test| seeded_order_key(seed, &test.qualified_name));
        }
        TestOrdering::Stable => {}
    }
}

/// Produces a stable pseudo-random priority from one run seed and test identity.
fn seeded_order_key(seed: u64, qualified_name: &str) -> u64 {
    let mut hasher = SipHasher13::new_with_keys(seed, !seed);
    hasher.write(qualified_name.as_bytes());
    hasher.finish()
}

/// Recursively collects test information from a package and all its subpackages.
///
/// For each test function whose `@parametrize` decorators can be statically
/// counted, emits one `TestInfo` per case so that the partitioner can split
/// individual cases across workers. Cases of the same function share a
/// `function_root` key so they can be reordered as a unit.
fn collect_test_paths_recursive(
    package: &karva_collector::CollectedPackage,
    test_infos: &mut Vec<TestInfo>,
    previous_durations: &HashMap<TestCacheKey, Duration>,
) {
    for module in package.modules.values() {
        for test_fn_def in &module.test_function_defs {
            let module_name = module.path.module_name();
            let module_path = module.path.path();
            let function_name = test_fn_def.name.as_str();
            let function_root = format!("{module_name}::{function_name}");
            let case_count = karva_collector::count_parametrize_cases(test_fn_def);

            if let Some(case_count) = case_count
                && case_count > 0
            {
                for idx in 0..case_count {
                    let qualified_name = format!("{function_root}[{idx}]");
                    let duration = previous_durations
                        .get(qualified_name.as_str())
                        .copied()
                        .or_else(|| {
                            u32::try_from(case_count).ok().and_then(|case_count| {
                                previous_durations
                                    .get(function_root.as_str())
                                    .and_then(|duration| duration.checked_div(case_count))
                            })
                        });
                    test_infos.push(TestInfo {
                        module_name: module_name.to_string(),
                        qualified_name,
                        path: format!("{module_path}::{function_name}[{idx}]"),
                        duration,
                        function_root: function_root.clone(),
                    });
                }
            } else {
                let duration = previous_durations.get(function_root.as_str()).copied();
                test_infos.push(TestInfo {
                    module_name: module_name.to_string(),
                    qualified_name: function_root.clone(),
                    path: format!("{module_path}::{function_name}"),
                    duration,
                    function_root,
                });
            }
        }
    }

    for subpackage in package.packages.values() {
        collect_test_paths_recursive(subpackage, test_infos, previous_durations);
    }
}

/// Counts controller scheduling units, expanding only statically countable cases.
pub fn scheduled_test_count(package: &karva_collector::CollectedPackage) -> usize {
    let direct = package
        .modules
        .values()
        .flat_map(|module| &module.test_function_defs)
        .map(|test| {
            karva_collector::count_parametrize_cases(test)
                .unwrap_or(1)
                .max(1)
        })
        .fold(0usize, usize::saturating_add);
    package
        .packages
        .values()
        .map(scheduled_test_count)
        .fold(direct, usize::saturating_add)
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
            function_root: qualified_name.to_string(),
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
    fn crash_recovery_keeps_only_unstarted_static_parameter_cases() {
        let mut partition = Partition::new();
        for name in [
            "test_module::test_case[0]",
            "test_module::test_case[1]",
            "test_module::test_case[2]",
        ] {
            partition.add_test(test_info(name), 1);
        }
        let completed = HashSet::from([TestCacheKey::function_name("test_module::test_case[0]")]);

        let pending = partition.pending_after_crash(
            &completed,
            &TestCacheKey::function_name("test_module::test_case[1]"),
        );

        assert_eq!(pending.tests(), &["test_module::test_case[2]"]);
    }

    #[test]
    fn crash_recovery_does_not_rerun_dynamic_parameter_functions() {
        let mut partition = Partition::new();
        partition.add_test(test_info("test_module::test_case"), 1);

        let pending = partition.pending_after_crash(
            &HashSet::new(),
            &TestCacheKey::function_name("test_module::test_case[1]"),
        );

        assert!(pending.tests().is_empty());
    }

    #[test]
    fn duration_backed_partitioning_starts_from_qualified_name_order() {
        let duration = Some(Duration::from_millis(1));
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

        let first = partition_shuffled_tests(tests.clone(), 2, 170_938);
        let repeated = partition_shuffled_tests(tests, 2, 170_938);

        assert_eq!(first[0].tests(), repeated[0].tests());
        assert_eq!(first[1].tests(), repeated[1].tests());
        assert_eq!(
            first
                .iter()
                .map(|partition| partition.tests().len())
                .sum::<usize>(),
            6
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
            TestCacheKey::function_name("test_sample::test_b"),
            TestCacheKey::function_name("test_sample::test_c"),
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
        assert!(
            partitions
                .iter()
                .all(|partition| !partition.tests().is_empty())
        );
        assert_eq!(
            partitions
                .iter()
                .map(|partition| partition.tests().len())
                .sum::<usize>(),
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
                .flat_map(Partition::tests)
                .collect::<Vec<_>>(),
            [&format!("{test_path}::test_value")]
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
            partitions[0].tests(),
            &[format!("{test_path}::test_value[0]")]
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
