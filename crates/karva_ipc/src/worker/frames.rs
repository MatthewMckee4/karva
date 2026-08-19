//! Borrowed hot-path frames matching the owned worker protocol.

use std::fmt;
use std::time::Duration;

use karva_diagnostic::{
    CapturedTestOutput, RenderedDiagnostic, TestCaseAttempt, TestCaseOutcome, TestCaseResult,
    TestCaseRetry,
};
use karva_python_semantic::{QualifiedFunctionName, QualifiedTestName, TestCacheKey};
use serde::{Serialize, Serializer};

/// Borrows one checkpoint identity without constructing its cache-key string.
pub(super) fn checkpoint(test_name: &QualifiedTestName) -> impl Serialize + '_ {
    BorrowedCheckpointFrame::TestCheckpoint {
        parameters: test_name.parameters(),
        cache_key: BorrowedTestCacheKey {
            function_name: test_name.function_name(),
            case_index: test_name.case_index(),
        },
    }
}

/// Borrows one completed result without allocating an owned wire event.
pub(super) fn completion<'a>(
    cache_key: &'a TestCacheKey,
    result: &'a TestCaseResult,
) -> impl Serialize + 'a {
    BorrowedEventFrame::Event(BorrowedWorkerEvent::TestFinished {
        cache_key,
        result: BorrowedTestCaseResult::from_result(result),
    })
}

/// Allocation-free checkpoint frame matching the owned protocol variant.
#[derive(Serialize)]
enum BorrowedCheckpointFrame<'a> {
    /// Active test identity borrowed from the execution reporter.
    #[serde(rename = "C")]
    TestCheckpoint {
        /// Rendered parameter list without its function name or parentheses.
        #[serde(rename = "p", skip_serializing_if = "Option::is_none")]
        parameters: Option<&'a str>,

        /// Stable function and case identity serialized as one cache key.
        #[serde(rename = "k")]
        cache_key: BorrowedTestCacheKey<'a>,
    },
}

/// Allocation-free completion frame matching the owned protocol variant.
#[derive(Serialize)]
enum BorrowedEventFrame<'a> {
    /// Runtime event sent from a worker to its controller.
    Event(BorrowedWorkerEvent<'a>),
}

/// Runtime event payload borrowed while serializing one completed result.
#[derive(Serialize)]
enum BorrowedWorkerEvent<'a> {
    /// Test completed with its transport-safe result.
    TestFinished {
        /// Stable identity matching this result to its start checkpoint.
        cache_key: &'a TestCacheKey,

        /// Transport-safe completed test result.
        result: BorrowedTestCaseResult<'a>,
    },
}

/// Compact completed result frame omitting identities derivable from `full_name`.
#[derive(Serialize)]
struct BorrowedTestCaseResult<'a> {
    /// Fully qualified user-visible test name.
    full_name: &'a str,

    /// Final semantic outcome after retry policy has completed.
    outcome: &'a TestCaseOutcome<RenderedDiagnostic>,

    /// Total duration represented by this final result.
    duration: Duration,

    /// Retry policy and attempt count, when the test was retried.
    #[serde(skip_serializing_if = "Option::is_none")]
    retry: Option<&'a TestCaseRetry>,

    /// Output captured during the final attempt, when non-empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    captured_output: Option<&'a CapturedTestOutput>,

    /// Earlier attempts retained when retry policy reran the test.
    #[serde(skip_serializing_if = "slice_is_empty")]
    attempts: &'a [TestCaseAttempt],
}

impl<'a> BorrowedTestCaseResult<'a> {
    fn from_result(result: &'a TestCaseResult) -> Self {
        Self {
            full_name: result.full_name(),
            outcome: result.outcome(),
            duration: result.duration(),
            retry: result.retry(),
            captured_output: result.captured_output(),
            attempts: result.attempts(),
        }
    }
}

fn slice_is_empty<T>(value: &[T]) -> bool {
    value.is_empty()
}

/// Stable cache identity serialized without constructing an owned string.
struct BorrowedTestCacheKey<'a> {
    /// Qualified function portion of the cache key.
    function_name: &'a QualifiedFunctionName,

    /// Optional terminal parameter-case index.
    case_index: Option<usize>,
}

impl fmt::Display for BorrowedTestCacheKey<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.function_name.fmt(formatter)?;
        if let Some(case_index) = self.case_index {
            write!(formatter, "[{case_index}]")?;
        }
        Ok(())
    }
}

impl Serialize for BorrowedTestCacheKey<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use karva_diagnostic::{TestCaseOutcome, TestCaseResult};
    use karva_python_semantic::{ModulePath, QualifiedFunctionName, QualifiedTestName};

    use super::{checkpoint, completion};
    use crate::protocol::{WireMessage, WorkerEvent};

    #[test]
    fn borrowed_checkpoint_matches_owned_wire_format() {
        let test_name = QualifiedTestName::with_parameters(
            QualifiedFunctionName::new(
                "test_example".to_string(),
                ModulePath::new_with_name("test.py", "tests.tést".to_string()),
            ),
            "value='é'".to_string(),
        )
        .with_case_index(Some(12));
        let owned = WireMessage::TestCheckpoint {
            parameters: test_name.parameters().map(str::to_owned),
            cache_key: test_name.cache_key(),
        };

        assert_eq!(
            serde_json::to_value(checkpoint(&test_name)).expect("serialize borrowed checkpoint"),
            serde_json::to_value(owned).expect("serialize owned checkpoint")
        );
    }

    #[test]
    fn borrowed_completion_omits_derived_identities_and_round_trips() {
        let test_name = QualifiedTestName::new(QualifiedFunctionName::new(
            "test_example".to_string(),
            ModulePath::new_with_name("test.py", "tests.test".to_string()),
        ));
        let cache_key = test_name.cache_key();
        let result: TestCaseResult = TestCaseResult::new(
            &test_name,
            TestCaseOutcome::Passed,
            Duration::from_millis(2),
            None,
        );
        let borrowed = serde_json::to_value(completion(&cache_key, &result))
            .expect("serialize borrowed completion");
        assert!(
            borrowed["Event"]["TestFinished"]["result"]
                .get("module_name")
                .is_none()
        );
        assert!(
            borrowed["Event"]["TestFinished"]["result"]
                .get("name")
                .is_none()
        );

        let decoded: WireMessage =
            serde_json::from_value(borrowed).expect("deserialize compact completion");
        let (decoded_cache_key, decoded_result) = (if let WireMessage::Event(event) = decoded
            && let WorkerEvent::TestFinished {
                cache_key: decoded_cache_key,
                result: decoded_result,
            } = *event
        {
            Some((decoded_cache_key, decoded_result))
        } else {
            None
        })
        .expect("compact completion should decode as test result");
        assert_eq!(decoded_cache_key, cache_key);
        assert_eq!(*decoded_result, result);
    }
}
