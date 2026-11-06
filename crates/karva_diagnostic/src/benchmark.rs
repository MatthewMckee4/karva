//! Callable-only measurements retained independently of test lifecycle duration.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// One benchmark invocation, including its work budget and partial failure data.
///
/// Times are nanoseconds; round samples are per-call averages rounded to the
/// nearest nanosecond. Statistics exclude warmup and calibration calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BenchmarkResult {
    /// User-defined comparison group, when supplied by the benchmark tag.
    pub group: Option<String>,

    /// Whether only one correctness call ran, without statistical measurement.
    disabled: bool,

    /// Effective sampling controls, including defaults; time options use seconds.
    pub options: BTreeMap<String, serde_json::Value>,

    /// Completed measurement rounds, excluding warmup and calibration.
    pub rounds: u64,

    /// Callable invocations per measurement round.
    pub iterations: u64,

    /// Actual warmup invocations, including an invocation that raised.
    pub warmup_calls: u64,

    /// Actual invocations used to adapt the iteration count.
    pub calibration_calls: u64,

    /// Actual measurement invocations, including an invocation that raised.
    pub measurement_calls: u64,

    /// Total invocations across all measurement phases.
    pub total_calls: u64,

    /// Wall time across warmup, calibration, and measurement, in nanoseconds.
    pub elapsed_ns: u64,

    /// Per-call averages for completed rounds, in execution order.
    samples_ns: Vec<u64>,

    /// Minimum per-call round average; absent when no round completed.
    pub min_ns: Option<u64>,

    /// Median per-call round average; absent when no round completed.
    pub median_ns: Option<u64>,

    /// Arithmetic mean of round averages; absent when no round completed.
    pub mean_ns: Option<u64>,

    /// Sample standard deviation, or zero when only one round completed.
    pub stddev_ns: Option<u64>,

    /// Why sampling stopped: `rounds`, `max_time`, `disabled`, or `exception`.
    pub stop_reason: String,
}
