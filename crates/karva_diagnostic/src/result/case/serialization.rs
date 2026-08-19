//! Stable flat serialization for internally grouped test results.

use std::time::Duration;

use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{
    CapturedTestOutput, TestCaseAttempt, TestCaseIdentity, TestCaseOutcome, TestCaseResult,
    TestCaseResultPayload, TestCaseRetry,
};

/// Flat compatibility shape used to decode persisted and IPC results directly.
///
/// A dedicated shape avoids `serde(flatten)` buffering on every worker result
/// while keeping the public JSON fields unchanged.
#[derive(Deserialize)]
#[serde(bound(deserialize = "D: Deserialize<'de>"))]
struct SerializedTestCaseResult<D> {
    /// Fully qualified user-visible test name.
    full_name: String,

    /// Final semantic outcome after retry policy has completed.
    outcome: TestCaseOutcome<D>,

    /// Total duration represented by this final result.
    duration: Duration,

    /// Retry policy and attempt count, when the test was retried.
    #[serde(default)]
    retry: Option<TestCaseRetry>,

    /// Output captured during the final attempt, when non-empty.
    #[serde(default)]
    captured_output: Option<CapturedTestOutput>,

    /// Earlier attempts retained when retry policy reran the test.
    #[serde(default)]
    attempts: Vec<TestCaseAttempt<D>>,
}

impl<D: Serialize> Serialize for TestCaseResult<D> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let optional_fields = usize::from(self.payload.retry.is_some())
            + usize::from(self.payload.captured_output.is_some())
            + usize::from(!self.payload.attempts.is_empty());
        let mut result = serializer.serialize_struct("TestCaseResult", 5 + optional_fields)?;
        result.serialize_field("module_name", self.module_name())?;
        result.serialize_field("name", self.name())?;
        result.serialize_field("full_name", &self.identity.full_name)?;
        result.serialize_field("outcome", &self.payload.outcome)?;
        result.serialize_field("duration", &self.payload.duration)?;
        if let Some(retry) = self.payload.retry.as_ref() {
            result.serialize_field("retry", retry)?;
        }
        if let Some(captured_output) = self.payload.captured_output.as_ref() {
            result.serialize_field("captured_output", captured_output)?;
        }
        if !self.payload.attempts.is_empty() {
            result.serialize_field("attempts", &self.payload.attempts)?;
        }
        result.end()
    }
}

impl<'de, D: Deserialize<'de>> Deserialize<'de> for TestCaseResult<D> {
    fn deserialize<T>(deserializer: T) -> Result<Self, T::Error>
    where
        T: Deserializer<'de>,
    {
        let result = SerializedTestCaseResult::deserialize(deserializer)?;
        Ok(Self {
            identity: TestCaseIdentity {
                full_name: result.full_name,
            },
            payload: TestCaseResultPayload {
                outcome: result.outcome,
                duration: result.duration,
                retry: result.retry,
                captured_output: result.captured_output,
                attempts: result.attempts,
            },
        })
    }
}
