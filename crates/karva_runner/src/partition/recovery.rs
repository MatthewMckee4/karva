//! Assignment filtering and progress tracking after worker exits.

use std::collections::{HashMap, HashSet};

use karva_python_semantic::TestCacheKey;

use super::Partition;

/// Recovery decision for a worker exit without an active test checkpoint.
#[derive(Debug)]
pub enum UnattributedCrashRecovery {
    /// Retry the remaining selectors in a replacement worker.
    Retry {
        /// Filtered assignment for the replacement worker.
        pending: Partition,

        /// Results from this assignment already committed to the controller.
        completed_results: usize,
    },

    /// Every selector in the failed assignment already produced a result.
    Complete {
        /// Results from this assignment already committed to the controller.
        completed_results: usize,
    },

    /// A replacement worker exited without committing another result.
    Stalled {
        /// Results from this assignment already committed to the controller.
        completed_results: usize,
    },
}

/// Completed-result membership indexed once for one crash-recovery batch.
pub struct CompletedTestIndex<'a> {
    /// Exact keys committed through `TestFinished` events.
    exact: &'a HashSet<TestCacheKey>,

    /// Committed keys grouped by their schedulable function identity.
    by_function: HashMap<&'a str, CompletedFunctionResults<'a>>,
}

/// Completed keys for one function without allocating for the common single result.
struct CompletedFunctionResults<'a> {
    /// First result, stored inline for plain non-parameterized tests.
    first: &'a TestCacheKey,

    /// Further runtime or static parameter cases for the same function.
    additional: Vec<&'a TestCacheKey>,
}

impl<'a> CompletedFunctionResults<'a> {
    /// Starts one function group without a secondary heap allocation.
    fn new(first: &'a TestCacheKey) -> Self {
        Self {
            first,
            additional: Vec::new(),
        }
    }

    /// Number of committed results for this function.
    fn len(&self) -> usize {
        self.additional.len().saturating_add(1)
    }

    /// Appends owned keys used to skip completed runtime-expanded cases.
    fn append_to(&self, target: &mut Vec<TestCacheKey>) {
        target.push(self.first.clone());
        target.extend(self.additional.iter().map(|cache_key| (*cache_key).clone()));
    }
}

impl<'a> CompletedTestIndex<'a> {
    /// Builds linear-time membership for every failed assignment in a batch.
    pub fn new(exact: &'a HashSet<TestCacheKey>) -> Self {
        let mut by_function: HashMap<&str, CompletedFunctionResults<'_>> = HashMap::new();
        for cache_key in exact {
            by_function
                .entry(cache_key.test_function_name())
                .and_modify(|results| results.additional.push(cache_key))
                .or_insert_with(|| CompletedFunctionResults::new(cache_key));
        }
        Self { exact, by_function }
    }

    /// Whether this exact static selector or plain function has completed.
    fn contains(&self, cache_key: &TestCacheKey) -> bool {
        self.exact.contains(cache_key)
    }

    /// Results committed for one function, including runtime-expanded cases.
    fn function_results(&self, function_root: &str) -> Option<&CompletedFunctionResults<'_>> {
        self.by_function.get(function_root)
    }
}

impl Partition {
    /// Returns work not committed before an attributable test crash.
    pub fn pending_after_test_crash(
        &self,
        completed: &CompletedTestIndex<'_>,
        crashed: &TestCacheKey,
    ) -> Self {
        self.pending_after_crash(completed, Some(crashed))
            .with_unattributed_retry_baseline(completed)
    }

    /// Decides whether an unattributed exit has useful work left to retry.
    pub fn recover_unattributed_crash(
        &self,
        completed: &CompletedTestIndex<'_>,
    ) -> UnattributedCrashRecovery {
        let completed_results = self.completed_result_count(completed);
        if self.unattributed_retry_baseline == Some(completed_results) {
            return UnattributedCrashRecovery::Stalled { completed_results };
        }

        let pending = self.pending_after_crash(completed, None);
        if pending.is_empty() {
            return UnattributedCrashRecovery::Complete { completed_results };
        }

        UnattributedCrashRecovery::Retry {
            pending: pending.with_unattributed_retry_baseline(completed),
            completed_results,
        }
    }

    /// Returns work not committed before a crash, excluding its active test.
    fn pending_after_crash(
        &self,
        completed: &CompletedTestIndex<'_>,
        crashed: Option<&TestCacheKey>,
    ) -> Self {
        let mut pending = Self::new();
        pending.resume_skip.clone_from(&self.resume_skip);
        for test in &self.tests {
            let cache_key = test.cache_key();
            let completed_function = completed.function_results(test.function_root.as_ref());
            let contains_crashed_dynamic_case = crashed.is_some_and(|crashed| {
                crashed.is_parameter_case()
                    && test.case_index.is_none()
                    && test.function_root.as_ref() == crashed.test_function_name()
            });
            let contains_completed_dynamic_case = crashed.is_none()
                && test.case_index.is_none()
                && !completed.contains(&cache_key)
                && completed_function.is_some();
            // A function-level selector with case-level progress was expanded
            // at runtime. Retry the active function, or every possibly active
            // function after an unattributed crash, and skip handled cases.
            if contains_crashed_dynamic_case || contains_completed_dynamic_case {
                pending.tests.push(test.clone());
                if let Some(completed_function) = completed_function {
                    completed_function.append_to(&mut pending.resume_skip);
                }
                if let Some(crashed) = crashed {
                    pending.resume_skip.push(crashed.clone());
                }
                continue;
            }
            if !completed.contains(&cache_key)
                && (completed_function.is_none() || test.case_index.is_some())
                && crashed != Some(&cache_key)
            {
                pending.tests.push(test.clone());
            }
        }
        pending.resume_skip.sort();
        pending.resume_skip.dedup();
        pending
    }

    /// Counts committed results whose function belongs to this assignment.
    fn completed_result_count(&self, completed: &CompletedTestIndex<'_>) -> usize {
        let mut counted_functions = HashSet::new();
        self.tests
            .iter()
            .map(|test| {
                if test.case_index.is_some() {
                    usize::from(completed.contains(&test.cache_key()))
                } else if counted_functions.insert(test.function_root.as_ref()) {
                    completed
                        .function_results(test.function_root.as_ref())
                        .map_or(0, CompletedFunctionResults::len)
                } else {
                    0
                }
            })
            .sum()
    }

    /// Records the current assignment-local progress for a replacement worker.
    fn with_unattributed_retry_baseline(mut self, completed: &CompletedTestIndex<'_>) -> Self {
        self.unattributed_retry_baseline = Some(self.completed_result_count(completed));
        self
    }
}
