//! Worker assignment state and crash-recovery filtering.

mod balancing;
mod collection;
mod ordering;

use std::collections::HashSet;
use std::sync::Arc;

pub use balancing::partition_collected_tests;
#[cfg(test)]
use balancing::partition_shuffled_tests;
use collection::TestInfo;
use karva_python_semantic::TestCacheKey;
#[cfg(test)]
use ordering::order_tests_for_partitioning;

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
    /// Worker selectors paired with stable crash-recovery identities.
    tests: Vec<ScheduledTest>,

    /// Runtime-expanded cases a replacement worker must not execute again.
    resume_skip: Vec<TestCacheKey>,

    /// Cumulative historical microseconds, using one per unknown test.
    weight: u128,

    /// Completed cases seen before retrying a crash with no attributable test.
    completed_before_unattributed_retry: Option<usize>,
}

/// One schedulable selector retained while its worker generation runs.
#[derive(Debug, Clone)]
struct ScheduledTest {
    /// Worker CLI selector shared with the outbound IPC selection.
    path: Arc<str>,

    /// Stable identity used to exclude completed or crashed work.
    cache_key: TestCacheKey,
}

impl Partition {
    fn new() -> Self {
        Self {
            tests: Vec::new(),
            resume_skip: Vec::new(),
            weight: 0,
            completed_before_unattributed_retry: None,
        }
    }

    fn add_test(&mut self, test: TestInfo, test_weight: u128) {
        self.tests.push(ScheduledTest {
            path: test.path,
            cache_key: TestCacheKey::function_name(&test.qualified_name),
        });
        self.weight += test_weight;
    }

    fn weight(&self) -> u128 {
        self.weight
    }

    /// Returns worker CLI selectors in execution order.
    #[cfg(test)]
    pub(super) fn test_paths(&self) -> impl ExactSizeIterator<Item = &str> {
        self.tests.iter().map(|test| test.path.as_ref())
    }

    /// Clones shared selector handles for the worker IPC handshake.
    pub(super) fn worker_test_paths(&self) -> Vec<Arc<str>> {
        self.tests
            .iter()
            .map(|test| Arc::clone(&test.path))
            .collect()
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
            .map(|test| test.cache_key.test_function_name())
    }

    /// Runtime-expanded cases already handled by an earlier worker generation.
    pub(super) fn resume_skip(&self) -> &[TestCacheKey] {
        &self.resume_skip
    }

    /// Returns work not committed before a worker crash, excluding the test
    /// that terminated the worker.
    pub(super) fn pending_after_crash(
        &self,
        completed: &HashSet<TestCacheKey>,
        crashed: Option<&TestCacheKey>,
    ) -> Self {
        let mut pending = Self::new();
        pending.resume_skip.clone_from(&self.resume_skip);
        pending.completed_before_unattributed_retry = self.completed_before_unattributed_retry;
        if crashed.is_none() {
            let function_roots = self
                .tests
                .iter()
                .map(|test| test.cache_key.test_function_name())
                .collect::<HashSet<_>>();
            let completed_in_partition = completed
                .iter()
                .filter(|completed| function_roots.contains(completed.test_function_name()))
                .count();
            if self.completed_before_unattributed_retry == Some(completed_in_partition) {
                return pending;
            }
            pending.completed_before_unattributed_retry = Some(completed_in_partition);
        }
        for test in &self.tests {
            let cache_key = &test.cache_key;
            let completed_function = completed
                .iter()
                .any(|completed| completed.test_function_name() == cache_key.test_function_name());
            let contains_crashed_dynamic_case = crashed.is_some_and(|crashed| {
                !cache_key.is_parameter_case()
                    && cache_key.test_function_name() == crashed.test_function_name()
            });
            let contains_completed_dynamic_case = crashed.is_none()
                && !cache_key.is_parameter_case()
                && !completed.contains(cache_key)
                && completed_function;
            // A function-level selector with case-level progress was expanded
            // at runtime. Retry the active function, or every possibly active
            // function after an unattributed crash, and skip handled cases.
            if contains_crashed_dynamic_case || contains_completed_dynamic_case {
                pending.tests.push(test.clone());
                pending.resume_skip.extend(
                    completed
                        .iter()
                        .filter(|completed| {
                            completed.test_function_name() == cache_key.test_function_name()
                        })
                        .cloned(),
                );
                if let Some(crashed) = crashed {
                    pending.resume_skip.push(crashed.clone());
                }
                continue;
            }
            if !completed.contains(cache_key)
                && (!completed_function || cache_key.is_parameter_case())
                && crashed != Some(cache_key)
            {
                pending.tests.push(test.clone());
            }
        }
        pending.resume_skip.sort();
        pending.resume_skip.dedup();
        pending
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
