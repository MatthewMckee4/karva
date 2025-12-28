/// Information about a test including its path and complexity
#[derive(Debug, Clone)]
struct TestInfo {
    path: String,
    body_length: usize,
}

/// A partition of tests for a worker
#[derive(Debug)]
pub struct Partition {
    tests: Vec<String>,
}

impl Partition {
    const fn new() -> Self {
        Self { tests: Vec::new() }
    }

    fn add_test(&mut self, test: TestInfo) {
        self.tests.push(test.path);
    }

    pub(crate) fn tests(&self) -> &[String] {
        &self.tests
    }
}

/// Partition collected tests into N groups by cycling through partitions with sorted tests
pub fn partition_collected_tests(
    package: &karva_collector::CollectedPackage,
    num_workers: usize,
) -> Vec<Partition> {
    let mut test_infos = Vec::new();

    // Recursively collect test paths with body lengths from the package
    collect_test_paths_recursive(package, &mut test_infos);

    // Sort by body length (descending) to distribute larger tests first
    test_infos.sort_by(|a, b| b.body_length.cmp(&a.body_length));

    // Create partitions
    let mut partitions: Vec<Partition> = (0..num_workers).map(|_| Partition::new()).collect();

    // Distribute tests in round-robin fashion: largest to partition 0, second to partition 1, etc.
    for (i, test_info) in test_infos.into_iter().enumerate() {
        partitions[i % num_workers].add_test(test_info);
    }

    partitions
}

fn collect_test_paths_recursive(
    package: &karva_collector::CollectedPackage,
    test_infos: &mut Vec<TestInfo>,
) {
    for module in package.modules.values() {
        for test_fn_def in &module.test_function_defs {
            test_infos.push(TestInfo {
                path: format!("{}::{}", module.path.path(), test_fn_def.name),
                body_length: test_fn_def.body.len(),
            });
        }
    }

    for subpackage in package.packages.values() {
        collect_test_paths_recursive(subpackage, test_infos);
    }
}
