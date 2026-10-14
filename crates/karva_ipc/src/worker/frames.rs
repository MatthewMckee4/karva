//! Borrowed hot-path frames matching the owned worker protocol.

use std::time::Duration;

use karva_diagnostic::{TestCaseOutcome, TestCaseResult};
use karva_python_semantic::{QualifiedFunctionName, QualifiedTestName, TestCacheKey};
use serde::Serialize;

/// Borrows one checkpoint identity without constructing its cache-key string.
pub(super) fn checkpoint(
    test_name: &QualifiedTestName,
    include_function: bool,
) -> impl Serialize + '_ {
    BorrowedCheckpointFrame::TestCheckpoint {
        parameters: test_name.parameters(),
        function_name: include_function.then(|| test_name.function_name()),
        case_index: test_name.case_index(),
    }
}

/// Borrows one completed result without allocating an owned wire event.
pub(super) fn completion<'a>(
    cache_key: &'a TestCacheKey,
    result: &'a TestCaseResult,
) -> impl Serialize + 'a {
    if matches!(result.outcome(), TestCaseOutcome::Passed)
        && result.retry().is_none()
        && result.captured_output().is_none()
        && result.attempts().is_empty()
    {
        BorrowedEventFrame::TestPassed {
            cache_key,
            module_name: result.module_name(),
            name: result.name(),
            duration: result.duration(),
        }
    } else {
        BorrowedEventFrame::Event(BorrowedWorkerEvent::TestFinished { cache_key, result })
    }
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

        /// Function identity sent only when it changes on this connection.
        #[serde(rename = "f", skip_serializing_if = "Option::is_none")]
        function_name: Option<&'a QualifiedFunctionName>,

        /// Stable parameter expansion index.
        #[serde(rename = "i", skip_serializing_if = "Option::is_none")]
        case_index: Option<usize>,
    },
}

/// Allocation-free completion frame matching the owned protocol variant.
#[derive(Serialize)]
enum BorrowedEventFrame<'a> {
    /// Runtime event sent from a worker to its controller.
    Event(BorrowedWorkerEvent<'a>),

    /// Passing result retaining only non-derivable identity and execution data.
    #[serde(rename = "P")]
    TestPassed {
        /// Stable case identity.
        #[serde(rename = "k")]
        cache_key: &'a TestCacheKey,

        /// Dotted module identity.
        #[serde(rename = "m")]
        module_name: &'a str,

        /// Module-relative display name.
        #[serde(rename = "n")]
        name: &'a str,

        /// Full lifecycle duration.
        #[serde(rename = "d")]
        duration: Duration,
    },
}

/// Runtime event payload borrowed while serializing one completed result.
#[derive(Serialize)]
enum BorrowedWorkerEvent<'a> {
    /// Test completed with its transport-safe result.
    TestFinished {
        /// Stable identity matching this result to its start checkpoint.
        cache_key: &'a TestCacheKey,

        /// Transport-safe completed test result.
        result: &'a TestCaseResult,
    },
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use karva_diagnostic::{CapturedTestOutput, TestCaseOutcome, TestCaseResult};
    use karva_python_semantic::{ModulePath, QualifiedFunctionName, QualifiedTestName};
    use rstest::rstest;

    use super::{checkpoint, completion};
    use crate::protocol::{WireMessage, WorkerEvent};

    #[rstest]
    fn borrowed_checkpoint_matches_owned_wire_format(
        #[values(false, true)] include_function: bool,
    ) {
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
            function_name: include_function.then(|| test_name.function_name().to_string()),
            case_index: test_name.case_index(),
        };

        assert_eq!(
            serde_json::to_value(checkpoint(&test_name, include_function))
                .expect("serialize borrowed checkpoint"),
            serde_json::to_value(owned).expect("serialize owned checkpoint")
        );
    }

    #[rstest]
    fn borrowed_completion_matches_owned_wire_format(
        #[values(TestCaseOutcome::Passed, TestCaseOutcome::Skipped { reason: None })]
        outcome: TestCaseOutcome,
        #[values(false, true)] has_output: bool,
    ) {
        let test_name = QualifiedTestName::new(QualifiedFunctionName::new(
            "test_example".to_string(),
            ModulePath::new_with_name("test.py", "tests.test".to_string()),
        ));
        let cache_key = test_name.cache_key();
        let result: TestCaseResult = TestCaseResult::new(
            &test_name,
            outcome,
            Duration::from_millis(2),
            has_output.then(|| CapturedTestOutput::new("hello".to_string(), String::new())),
        );
        let borrowed = serde_json::to_value(completion(&cache_key, &result))
            .expect("serialize borrowed completion");
        let owned = if matches!(result.outcome(), TestCaseOutcome::Passed) && !has_output {
            WireMessage::TestPassed {
                cache_key,
                module_name: result.module_name().to_string(),
                name: result.name().to_string(),
                duration: result.duration(),
            }
        } else {
            WireMessage::Event(Box::new(WorkerEvent::TestFinished {
                cache_key,
                result: Box::new(result),
            }))
        };

        assert_eq!(
            borrowed,
            serde_json::to_value(owned).expect("serialize owned completion")
        );
    }
}
