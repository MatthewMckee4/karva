//! Human-readable benchmark summaries; structured reports retain raw measurements.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::Duration;

use anyhow::Result;
use colored::Colorize;
use karva_diagnostic::{AggregatedResults, BenchmarkResult};

/// Groups measurements without hiding test parameters or incomplete sampling.
pub(super) fn write_benchmarks(
    stdout: &mut impl Write,
    results: &AggregatedResults,
    verbose: bool,
) -> Result<()> {
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for case in &results.test_cases {
        for benchmark in case.benchmarks() {
            groups
                .entry(benchmark.group.as_deref())
                .or_default()
                .push((case.full_name(), benchmark));
        }
    }
    if groups.is_empty() {
        return Ok(());
    }

    writeln!(stdout, "\nbenchmarks (median per call):")?;
    for (group, mut benchmarks) in groups {
        benchmarks.sort_by_key(|(name, _)| *name);
        writeln!(stdout, "\n  {}", group.unwrap_or("ungrouped").bold())?;
        for (name, benchmark) in benchmarks {
            let label = match benchmark.stop_reason.as_str() {
                "rounds" | "max_time" => format!("{:>12}", "BENCH").green().bold(),
                "exception" => format!("{:>12}", "PARTIAL").yellow().bold(),
                "disabled" => format!("{:>12}", "DISABLED").dimmed(),
                reason => format!("{reason:>12}").normal(),
            };
            writeln!(
                stdout,
                "{label} [{:>9}] {name}",
                format_time(benchmark.median_ns)
            )?;
            if verbose {
                write_details(stdout, benchmark)?;
            }
        }
    }
    writeln!(stdout)?;
    Ok(())
}

/// Uses standard duration units while retaining integer nanoseconds below one µs.
fn format_time(nanoseconds: Option<u64>) -> String {
    match nanoseconds {
        None => "-".to_owned(),
        Some(ns @ 0..=999) => format!("{ns}ns"),
        Some(ns) => format!("{:.2?}", Duration::from_nanos(ns)),
    }
}

/// Aligns details after the same 12-character status column as result lines.
const DETAIL_INDENT: &str = "             ";

/// Adds sampling details directly below their result line.
fn write_details(stdout: &mut impl Write, benchmark: &BenchmarkResult) -> Result<()> {
    let stddev = if benchmark.rounds > 1 {
        benchmark.stddev_ns
    } else {
        None
    };
    writeln!(
        stdout,
        "{DETAIL_INDENT}min={} mean={} stddev={} rounds={} iterations={}",
        format_time(benchmark.min_ns),
        format_time(benchmark.mean_ns),
        format_time(stddev),
        benchmark.rounds,
        benchmark.iterations,
    )?;
    writeln!(
        stdout,
        "{DETAIL_INDENT}warmup={} calibration={} measured={} total={}",
        benchmark.warmup_calls,
        benchmark.calibration_calls,
        benchmark.measurement_calls,
        benchmark.total_calls,
    )?;
    writeln!(
        stdout,
        "{DETAIL_INDENT}elapsed={} stop={}",
        format_time(Some(benchmark.elapsed_ns)),
        benchmark.stop_reason,
    )?;
    writeln!(
        stdout,
        "{DETAIL_INDENT}options: {}",
        serde_json::to_string(&benchmark.options)?
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::format_time;

    #[rstest]
    #[case(None, "-")]
    #[case(Some(0), "0ns")]
    #[case(Some(999), "999ns")]
    #[case(Some(1_500), "1.50µs")]
    #[case(Some(1_500_000), "1.50ms")]
    #[case(Some(1_500_000_000), "1.50s")]
    fn measurement_units(#[case] nanoseconds: Option<u64>, #[case] expected: &str) {
        assert_eq!(format_time(nanoseconds), expected);
    }
}
