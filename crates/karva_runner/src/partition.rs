//! Worker assignment state and crash-recovery filtering.

mod balancing;
mod collection;
mod ordering;
mod recovery;

use std::sync::Arc;

pub use balancing::partition_collected_tests;
#[cfg(test)]
use balancing::partition_shuffled_tests;
use collection::{TestIdentity, TestInfo};
use karva_ipc::WorkerPath;
use karva_python_semantic::TestCacheKey;
#[cfg(test)]
use ordering::order_tests_for_partitioning;
pub use recovery::{CompletedTestIndex, UnattributedCrashRecovery};

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

/// Worker assignment produced by module-aware load balancing.
#[derive(Debug, Clone)]
pub struct Partition {
    /// Compact test identities paired with stable crash-recovery indices.
    tests: Vec<ScheduledTest>,

    /// Runtime-expanded cases a replacement worker must not execute again.
    resume_skip: Vec<TestCacheKey>,

    /// Cumulative historical microseconds, using one per unknown test.
    weight: u128,

    /// Completed cases in this assignment when its recovery worker was spawned.
    unattributed_retry_baseline: Option<usize>,
}

/// One scheduled test identity retained while its worker generation runs.
#[derive(Debug, Clone)]
struct ScheduledTest {
    /// Function identity shared by every case collected from one test function.
    identity: Arc<TestIdentity>,

    /// Stable expansion index for a statically countable parameter case.
    case_index: Option<usize>,
}

impl ScheduledTest {
    /// Retains exact worker selector without formatting static case suffix.
    fn worker_path(&self) -> WorkerPath {
        self.case_index.map_or_else(
            || WorkerPath::owned(Arc::clone(&self.identity.selector)),
            |index| WorkerPath::indexed(Arc::clone(&self.identity.selector), index),
        )
    }

    /// Materializes the recovery key only on an exceptional worker-crash path.
    fn cache_key(&self) -> TestCacheKey {
        self.case_index.map_or_else(
            || TestCacheKey::function_name(&self.identity.function_root),
            |index| TestCacheKey::parameter_case_name(&self.identity.function_root, index),
        )
    }
}

impl Partition {
    fn new() -> Self {
        Self {
            tests: Vec::new(),
            resume_skip: Vec::new(),
            weight: 0,
            unattributed_retry_baseline: None,
        }
    }

    fn add_test(&mut self, test: TestInfo, test_weight: u128) {
        self.tests.push(ScheduledTest {
            identity: test.identity,
            case_index: test.case_index,
        });
        self.weight += test_weight;
    }

    fn weight(&self) -> u128 {
        self.weight
    }

    /// Returns worker CLI selectors in execution order.
    #[cfg(test)]
    fn test_paths(&self) -> impl ExactSizeIterator<Item = String> + '_ {
        self.tests.iter().map(|test| test.worker_path().to_string())
    }

    /// Materializes scheduled keys for collector-to-recovery parity tests.
    #[cfg(test)]
    fn cache_keys(&self) -> impl Iterator<Item = TestCacheKey> + '_ {
        self.tests.iter().map(ScheduledTest::cache_key)
    }

    /// Retains worker selectors compactly for the outbound IPC handshake.
    pub(super) fn worker_test_paths(&self) -> Vec<WorkerPath> {
        self.tests.iter().map(ScheduledTest::worker_path).collect()
    }

    /// Number of selectors assigned to this worker generation.
    pub(super) fn test_count(&self) -> usize {
        self.tests.len()
    }

    /// Whether this assignment has any worker selectors.
    pub(super) fn is_empty(&self) -> bool {
        self.tests.is_empty()
    }

    /// Returns function identities represented by this assignment.
    pub(super) fn function_roots(&self) -> impl Iterator<Item = &str> {
        self.tests
            .iter()
            .map(|test| test.identity.function_root.as_ref())
    }

    /// Runtime-expanded cases already handled by an earlier worker generation.
    pub(super) fn resume_skip(&self) -> &[TestCacheKey] {
        &self.resume_skip
    }
}

/// Counts controller scheduling units, expanding only statically countable cases.
pub fn scheduled_test_count(package: &karva_collector::CollectedPackage) -> usize {
    let direct = package
        .modules
        .values()
        .map(|module| {
            module
                .test_function_defs
                .iter()
                .map(|test| {
                    karva_collector::count_parametrize_cases(test)
                        .unwrap_or(1)
                        .max(1)
                })
                .fold(module.doctests.len(), usize::saturating_add)
        })
        .fold(0usize, usize::saturating_add);
    package
        .packages
        .values()
        .map(scheduled_test_count)
        .fold(direct, usize::saturating_add)
}

#[cfg(test)]
mod tests;
