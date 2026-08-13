//! Wire protocol shared by the Karva controller and worker processes.

use karva_diagnostic::{RenderedDiagnostic, TestCaseResult};
use karva_python_semantic::TestCacheKey;
use serde::{Deserialize, Serialize};

/// One runtime state change sent from a worker to the controller.
#[derive(Serialize, Deserialize)]
pub enum WorkerEvent {
    /// Worker entered one test's setup-or-execution lifecycle.
    ///
    /// The first checkpoint precedes fixture setup. A later checkpoint may
    /// refine the display name after fixture-derived parameters resolve.
    TestStarted {
        /// Best display identity known when it differs from the stable key.
        ///
        /// Plain function names are omitted to avoid sending the same string
        /// twice; the controller reconstructs them from `cache_key`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<String>,

        /// Stable case identity used for completion and crash recovery.
        cache_key: TestCacheKey,
    },

    /// Test exceeded the configured slow-test threshold.
    TestSlow,

    /// Test completed with its transport-safe result.
    TestFinished {
        /// Stable identity matching this result to its start checkpoint.
        cache_key: TestCacheKey,

        /// Transport-safe completed test result.
        result: Box<TestCaseResult>,
    },

    /// Diagnostic describing the run rather than one test.
    RunDiagnostic(RenderedDiagnostic),

    /// Worker completed normally after sending every result.
    WorkerFinished,
}

/// Message exchanged during the controller-worker handshake and event stream.
#[derive(Serialize, Deserialize)]
pub enum WireMessage {
    /// Identifies a worker connection to one controller run.
    Hello {
        /// Controller run identifier rejecting stale connections.
        run_id: String,

        /// Controller-assigned worker generation identifier.
        worker_id: usize,
    },

    /// Selection and resume state sent by the controller after authentication.
    TestSelection(WorkerSelection),

    /// Runtime event sent from a worker to its controller.
    Event(Box<WorkerEvent>),
}

/// Work owned by one worker generation.
#[derive(Serialize, Deserialize)]
pub struct WorkerSelection {
    /// Exact test selectors in execution order.
    pub test_paths: Vec<String>,

    /// Runtime-expanded cases already completed by an earlier generation.
    pub resume_skip: Vec<TestCacheKey>,
}
