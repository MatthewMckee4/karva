//! Wire protocol shared by the Karva controller and worker processes.

use std::borrow::Cow;
use std::fmt;
use std::sync::Arc;

use karva_diagnostic::{RenderedDiagnostic, TestCaseResult};
use karva_python_semantic::TestCacheKey;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
    /// Compact test selectors in execution order, serialized as exact wire paths.
    pub test_paths: Vec<WorkerPath>,

    /// Runtime-expanded cases already completed by an earlier generation.
    pub resume_skip: Vec<TestCacheKey>,
}

/// Test selector retained compactly until the worker-selection frame is written.
#[derive(Clone, Debug)]
pub struct WorkerPath {
    /// Base selector shared by every static case from one test function.
    selector: Arc<str>,

    /// Static parameter case index, absent for an unindexed selector.
    index: Option<usize>,
}

impl WorkerPath {
    /// Retains a complete selector received from, or ready for, the wire.
    pub fn owned(path: impl Into<Arc<str>>) -> Self {
        Self {
            selector: path.into(),
            index: None,
        }
    }

    /// Retains a base selector and case index without formatting the suffix.
    pub fn indexed(selector: Arc<str>, index: usize) -> Self {
        Self {
            selector,
            index: Some(index),
        }
    }

    /// Borrows an owned selector or formats an indexed selector for a worker API.
    pub fn as_cow(&self) -> Cow<'_, str> {
        match self.index {
            Some(index) => Cow::Owned(format!("{}[{index}]", self.selector)),
            None => Cow::Borrowed(&self.selector),
        }
    }
}

impl fmt::Display for WorkerPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.index {
            Some(index) => write!(formatter, "{}[{index}]", self.selector),
            None => formatter.write_str(&self.selector),
        }
    }
}

impl Serialize for WorkerPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.index {
            Some(_) => serializer.collect_str(self),
            None => serializer.serialize_str(self.selector.as_ref()),
        }
    }
}

impl<'de> Deserialize<'de> for WorkerPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::owned(String::deserialize(deserializer)?))
    }
}

#[cfg(test)]
mod tests {
    use super::{WireMessage, WorkerPath, WorkerSelection};

    #[test]
    fn indexed_paths_keep_exact_wire_shape() {
        let compact = WireMessage::TestSelection(WorkerSelection {
            test_paths: vec![WorkerPath::indexed("mod::test".into(), 3)],
            resume_skip: Vec::new(),
        });
        let materialized = WireMessage::TestSelection(WorkerSelection {
            test_paths: vec![WorkerPath::owned("mod::test[3]")],
            resume_skip: Vec::new(),
        });

        let compact_json = serde_json::to_string(&compact).expect("serialize compact selection");
        let materialized_json =
            serde_json::to_string(&materialized).expect("serialize materialized selection");
        assert_eq!(
            compact_json,
            r#"{"TestSelection":{"test_paths":["mod::test[3]"],"resume_skip":[]}}"#
        );
        assert_eq!(compact_json, materialized_json);

        let decoded: WireMessage =
            serde_json::from_str(&compact_json).expect("deserialize selection");
        assert!(matches!(decoded, WireMessage::TestSelection(_)));

        let decoded: WorkerPath =
            serde_json::from_str(r#""mod::test[3]""#).expect("deserialize worker path");
        assert_eq!(decoded.as_cow(), "mod::test[3]");
    }
}
