//! Reader-local crash-durable lifecycle state.

use std::time::Instant;

use anyhow::{Result, bail};
use karva_python_semantic::TestCacheKey;

/// Latest test lifecycle state read from one worker connection.
#[derive(Clone, Debug)]
pub struct WorkerCheckpoint {
    /// Best display identity received for the active test.
    pub name: String,

    /// Stable identity used by crash recovery and resume filtering.
    pub cache_key: TestCacheKey,

    /// Time when the controller reader observed the first checkpoint.
    pub started: Instant,
}

/// Active state owned exclusively by one connection reader.
///
/// Keeping updates local avoids synchronization and prevents high-volume
/// checkpoint frames from accumulating in the result-event queue. The reader
/// publishes the final value before its FIFO `Disconnected` notification;
/// controller-driven shutdown joins the reader before inspecting that value.
#[derive(Default)]
pub(super) struct CheckpointState {
    /// Started but unfinished test, if this worker has one.
    checkpoint: Option<ActiveCheckpoint>,
}

/// Compact active identity retained until the reader disconnects.
struct ActiveCheckpoint {
    /// Rendered parameter list without the duplicated function name.
    parameters: Option<String>,

    /// Stable identity used to validate completion and recover crashes.
    cache_key: TestCacheKey,

    /// Time when the controller reader observed the first checkpoint.
    started: Instant,
}

impl CheckpointState {
    /// Stores or refines one active test without changing its start time.
    pub(super) fn record(
        &mut self,
        worker_id: usize,
        parameters: Option<String>,
        cache_key: TestCacheKey,
    ) -> Result<()> {
        if let Some(checkpoint) = self.checkpoint.as_mut() {
            if checkpoint.cache_key != cache_key {
                let name = display_name(&cache_key, parameters.as_deref());
                bail!(
                    "Karva worker {worker_id} started `{name}` before finishing `{}`",
                    checkpoint.display_name()
                );
            }
            checkpoint.parameters = parameters;
            checkpoint.cache_key = cache_key;
        } else {
            self.checkpoint = Some(ActiveCheckpoint {
                parameters,
                cache_key,
                started: Instant::now(),
            });
        }
        Ok(())
    }

    /// Clears a matching active checkpoint when its result reaches the controller.
    pub(super) fn complete(&mut self, worker_id: usize, cache_key: &TestCacheKey) -> Result<()> {
        if let Some(checkpoint) = self.checkpoint.as_ref()
            && checkpoint.cache_key != *cache_key
        {
            bail!(
                "Karva worker {worker_id} started `{}` but finished `{cache_key}`",
                checkpoint.display_name()
            );
        }
        self.checkpoint = None;
        Ok(())
    }

    /// Rejects a terminal lifecycle event while a test remains active.
    pub(super) fn ensure_idle(&self, worker_id: usize) -> Result<()> {
        if let Some(checkpoint) = self.checkpoint.as_ref() {
            bail!(
                "Karva worker {worker_id} completed while `{}` was still running",
                checkpoint.display_name()
            );
        }
        Ok(())
    }

    /// Transfers final reader state for crash or cancellation reporting.
    pub(super) fn into_checkpoint(self) -> Option<WorkerCheckpoint> {
        self.checkpoint.map(ActiveCheckpoint::into_public)
    }
}

impl ActiveCheckpoint {
    /// Reconstructs the user-visible identity only for exceptional paths.
    fn display_name(&self) -> String {
        display_name(&self.cache_key, self.parameters.as_deref())
    }

    /// Expands compact reader state when ownership moves to orchestration.
    fn into_public(self) -> WorkerCheckpoint {
        WorkerCheckpoint {
            name: display_name(&self.cache_key, self.parameters.as_deref()),
            cache_key: self.cache_key,
            started: self.started,
        }
    }
}

/// Combines stable function identity with its optional rendered parameters.
fn display_name(cache_key: &TestCacheKey, parameters: Option<&str>) -> String {
    match parameters {
        Some(parameters) => format!("{}({parameters})", cache_key.test_function_name()),
        None if cache_key.is_parameter_case() => cache_key.test_function_name().to_string(),
        None => cache_key.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refinement_updates_name_without_resetting_start_time() {
        let mut state = CheckpointState::default();
        let cache_key = TestCacheKey::function_name("tests.test_mod::test_case[0]");
        state
            .record(7, None, cache_key.clone())
            .expect("record initial checkpoint");
        let started = state
            .checkpoint
            .as_ref()
            .expect("initial checkpoint")
            .started;

        state
            .record(7, Some("value=1".to_string()), cache_key)
            .expect("refine checkpoint");

        let refined = state.into_checkpoint().expect("refined checkpoint");
        assert_eq!(refined.name, "tests.test_mod::test_case(value=1)");
        assert_eq!(refined.started, started);
    }

    #[test]
    fn refinement_rejects_a_different_parameter_case() {
        let mut state = CheckpointState::default();
        state
            .record(
                7,
                None,
                TestCacheKey::function_name("tests.test_mod::test_case[0]"),
            )
            .expect("record initial checkpoint");

        let error = state
            .record(
                7,
                None,
                TestCacheKey::function_name("tests.test_mod::test_case[1]"),
            )
            .expect_err("overlapping case should fail");

        assert!(error.to_string().contains("before finishing"));
    }

    #[test]
    fn unresolved_parameter_case_uses_its_function_name() {
        let mut state = CheckpointState::default();
        state
            .record(
                7,
                None,
                TestCacheKey::function_name("tests.test_mod::test_case[0]"),
            )
            .expect("record unresolved parameter case");

        let checkpoint = state.into_checkpoint().expect("active checkpoint");
        assert_eq!(checkpoint.name, "tests.test_mod::test_case");
    }
}
