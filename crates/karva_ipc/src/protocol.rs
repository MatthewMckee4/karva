//! Wire protocol shared by the Karva controller and worker processes.

use std::sync::Arc;

use karva_diagnostic::{RenderedDiagnostic, TestCaseResult};
use karva_python_semantic::TestCacheKey;
use serde::{Deserialize, Serialize};

/// One runtime state change sent from a worker to the controller.
#[derive(Serialize, Deserialize)]
pub enum WorkerEvent {
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

    /// Crash-durable identity for a test entering setup or execution.
    ///
    /// This direct frame avoids allocating and decoding a dispatch event that
    /// never leaves the connection reader. A later frame may refine the
    /// display identity after fixture-derived parameters resolve. Its compact
    /// serialized keys limit traffic from one durable frame per test case;
    /// source-level field names remain descriptive.
    #[serde(rename = "C")]
    TestCheckpoint {
        /// Rendered parameter list without its function name or parentheses.
        ///
        /// The controller reconstructs the display identity from this suffix
        /// and `cache_key`, avoiding duplicate function names on the wire.
        #[serde(rename = "p", default, skip_serializing_if = "Option::is_none")]
        parameters: Option<String>,

        /// Stable case identity used for completion and crash recovery.
        #[serde(rename = "k")]
        cache_key: TestCacheKey,
    },

    /// Runtime event sent from a worker to its controller.
    Event(Box<WorkerEvent>),
}

/// Work owned by one worker generation.
#[derive(Serialize, Deserialize)]
pub struct WorkerSelection {
    /// Exact test selectors in execution order, shared with controller recovery state.
    pub test_paths: Vec<Arc<str>>,

    /// Runtime-expanded cases already completed by an earlier generation.
    pub resume_skip: Vec<TestCacheKey>,
}
